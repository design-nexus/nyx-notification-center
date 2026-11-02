//! Compatibility suite for stage B: nyx-notifyd as `org.freedesktop.Notifications`, on a
//! private bus with scratch folders, against what real senders do — the spec's calls,
//! libnotify's `notify-send`, and Omarchy's own `omarchy-notification-send` (its click
//! command, buttons and glyph hints) — plus the Nyx side: pop-ups, Do not disturb,
//! buttons, timeouts, and closing that tells the app.
//!
//! Needs `dbus-daemon`; skips (passing) without it.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::blocking::Connection;
use zbus::zvariant::{Structure, Value};

const FDO: (&str, &str, &str) =
    ("org.freedesktop.Notifications", "/org/freedesktop/Notifications", "org.freedesktop.Notifications");
const NYX: (&str, &str, &str) = (
    "io.github.design_nexus.nyx.Notifications",
    "/io/github/design_nexus/nyx/Notifications",
    "io.github.design_nexus.nyx.Notifications1",
);

struct Bus {
    daemon: Child,
    notifyd: Child,
    address: String,
    root: PathBuf,
    conn: Connection,
    signals: Arc<Mutex<Vec<(String, zbus::Message)>>>,
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.notifyd.kill();
        let _ = self.daemon.kill();
        let _ = self.notifyd.wait();
        let _ = self.daemon.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn bus(name: &str) -> Option<Bus> {
    if Command::new("dbus-daemon").arg("--version").stdout(Stdio::null()).status().is_err() {
        eprintln!("dbus-daemon not found; skipping");
        return None;
    }
    let root = std::env::temp_dir().join(format!("nyx-notifyd-compat-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for d in ["state/omarchy/notifications", "data", "cache", "config"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    std::fs::write(
        root.join("state/omarchy/notifications/quiet.json"),
        r#"{"snoozes":{},"dnd":false,"silencedSince":0,"codesBypassQuiet":true}"#,
    )
    .unwrap();
    let mut daemon =
        Command::new("dbus-daemon").args(["--session", "--nofork", "--print-address=1"]).stdout(Stdio::piped()).spawn().unwrap();
    let mut line = String::new();
    BufReader::new(daemon.stdout.take().unwrap()).read_line(&mut line).unwrap();
    let address = line.trim().to_string();
    let notifyd = Command::new(env!("CARGO_BIN_EXE_nyx-notifyd"))
        .env("DBUS_SESSION_BUS_ADDRESS", &address)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("NYX_NOTIFYD_MODE", "nyx")
        .spawn()
        .unwrap();
    let conn = zbus::blocking::connection::Builder::address(address.as_str()).unwrap().build().unwrap();
    // Wait for both names.
    let dbus = zbus::blocking::fdo::DBusProxy::new(&conn).unwrap();
    let start = Instant::now();
    while !(dbus.name_has_owner(FDO.0.try_into().unwrap()).unwrap_or(false)
        && dbus.name_has_owner(NYX.0.try_into().unwrap()).unwrap_or(false))
    {
        assert!(start.elapsed() < Duration::from_secs(10), "nyx-notifyd didn't start");
        std::thread::sleep(Duration::from_millis(50));
    }
    let signals: Arc<Mutex<Vec<(String, zbus::Message)>>> = Arc::default();
    for iface in [FDO.2, NYX.2] {
        let rule = zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal).interface(iface).unwrap().build();
        let it = zbus::blocking::MessageIterator::for_match_rule(rule, &conn, Some(256)).unwrap();
        let sig = signals.clone();
        std::thread::spawn(move || {
            for m in it.flatten() {
                let name = m.header().member().map(|m| m.to_string()).unwrap_or_default();
                sig.lock().unwrap().push((name, m));
            }
        });
    }
    std::thread::sleep(Duration::from_millis(100));
    Some(Bus { daemon, notifyd, address, root, conn, signals })
}

impl Bus {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app: &str,
        replaces: u32,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, Value>,
        timeout: i32,
    ) -> u32 {
        let r = self
            .conn
            .call_method(Some(FDO.0), FDO.1, Some(FDO.2), "Notify", &(app, replaces, "", summary, body, actions, hints, timeout))
            .unwrap();
        r.body().deserialize::<u32>().unwrap()
    }

    fn nyx<B: serde::Serialize + zbus::zvariant::DynamicType>(&self, method: &str, body: &B) -> zbus::Message {
        self.conn.call_method(Some(NYX.0), NYX.1, Some(NYX.2), method, body).unwrap()
    }

    fn nyx_json(&self, method: &str) -> serde_json::Value {
        let s: String = self.nyx(method, &()).body().deserialize().unwrap();
        serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)
    }

    /// Wait for a signal matching `f`, up to `secs`.
    fn wait(&self, secs: f64, f: impl Fn(&str, &zbus::Message) -> bool) -> Option<zbus::Message> {
        let start = Instant::now();
        while start.elapsed().as_secs_f64() < secs {
            if let Some((_, m)) = self.signals.lock().unwrap().iter().find(|(n, m)| f(n, m)) {
                return Some(m.clone());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }

    fn closed(&self, secs: f64, id: u32) -> Option<u32> {
        self.wait(secs, |n, m| n == "NotificationClosed" && m.body().deserialize::<(u32, u32)>().is_ok_and(|(i, _)| i == id))
            .map(|m| m.body().deserialize::<(u32, u32)>().unwrap().1)
    }

    fn popup(&self, secs: f64, summary: &str) -> Option<serde_json::Value> {
        self.wait(secs, |n, m| {
            n == "Popup"
                && m.body()
                    .deserialize::<String>()
                    .is_ok_and(|s| serde_json::from_str::<serde_json::Value>(&s).is_ok_and(|v| v["note"]["summary"] == summary))
        })
        .map(|m| serde_json::from_str(&m.body().deserialize::<String>().unwrap()).unwrap())
    }

    fn inbox(&self) -> Vec<serde_json::Value> {
        self.nyx_json("Inbox").as_array().cloned().unwrap_or_default()
    }

    fn find(&self, summary: &str) -> serde_json::Value {
        let h: String = self.nyx("History", &("",)).body().deserialize().unwrap();
        let v: Vec<serde_json::Value> = serde_json::from_str(&h).unwrap();
        v.into_iter().find(|n| n["summary"] == summary).unwrap_or(serde_json::Value::Null)
    }

    fn run(&self, cmd: &str, args: &[&str]) -> Option<String> {
        let out = Command::new(cmd)
            .args(args)
            .env("DBUS_SESSION_BUS_ADDRESS", &self.address)
            .env_remove("DBUS_SYSTEM_BUS_ADDRESS")
            .output()
            .ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
}

fn which(cmd: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
}

#[test]
fn server_information_and_capabilities() {
    let Some(b) = bus("info") else { return };
    let r = b.conn.call_method(Some(FDO.0), FDO.1, Some(FDO.2), "GetServerInformation", &()).unwrap();
    let (name, vendor, _version, spec): (String, String, String, String) = r.body().deserialize().unwrap();
    assert_eq!((name.as_str(), vendor.as_str(), spec.as_str()), ("Nyx Notification Center", "design-nexus", "1.2"));
    let r = b.conn.call_method(Some(FDO.0), FDO.1, Some(FDO.2), "GetCapabilities", &()).unwrap();
    let caps: Vec<String> = r.body().deserialize().unwrap();
    for c in ["actions", "body", "persistence"] {
        assert!(caps.iter().any(|x| x == c), "missing capability {c}");
    }
    let mode: String = b.nyx("Mode", &()).body().deserialize().unwrap();
    assert_eq!(mode, "nyx");
}

#[test]
fn ids_replacement_and_closing() {
    let Some(b) = bus("ids") else { return };
    let a = b.notify("Chat", 0, "First", "", &[], HashMap::new(), -1);
    let c = b.notify("Chat", 0, "Second", "", &[], HashMap::new(), -1);
    assert!(a > 0 && c > a, "ids are new and increasing");
    // replaces_id keeps the id and replaces the text in place.
    let r = b.notify("Chat", a, "First, edited", "", &[], HashMap::new(), -1);
    assert_eq!(r, a);
    let inbox = b.inbox();
    assert_eq!(inbox.len(), 2);
    assert!(inbox.iter().any(|n| n["summary"] == "First, edited"));
    assert!(b.popup(2.0, "First, edited").is_some(), "the replacement shows again");
    // An unknown replaces_id gets a fresh id.
    let fresh = b.notify("Chat", 9999, "Third", "", &[], HashMap::new(), -1);
    assert!(fresh != 9999 && fresh > c);
    // CloseNotification: reason 3, out of the Inbox, still in history.
    b.conn.call_method(Some(FDO.0), FDO.1, Some(FDO.2), "CloseNotification", &(c,)).unwrap();
    assert_eq!(b.closed(2.0, c), Some(3));
    assert!(!b.inbox().iter().any(|n| n["summary"] == "Second"));
    assert_eq!(b.find("Second")["closed_reason"], "closed");
}

#[test]
fn popups_follow_urgency_and_do_not_disturb() {
    let Some(b) = bus("popups") else { return };
    let low = HashMap::from([("urgency", Value::U8(0))]);
    let crit = || HashMap::from([("urgency", Value::U8(2))]);
    b.notify("A", 0, "Normal", "", &[], HashMap::new(), -1);
    b.notify("A", 0, "Low", "", &[], low, -1);
    b.notify("A", 0, "Critical", "", &[], crit(), -1);
    b.notify("A", 0, "Asked longer", "", &[], HashMap::new(), 20_000);
    assert_eq!(b.popup(2.0, "Normal").unwrap()["timeout_ms"], 8000);
    assert_eq!(b.popup(2.0, "Low").unwrap()["timeout_ms"], 5000);
    assert_eq!(b.popup(2.0, "Critical").unwrap()["timeout_ms"], 0, "critical stays up");
    assert_eq!(b.popup(2.0, "Asked longer").unwrap()["timeout_ms"], 20000);
    // Do not disturb: stored, no pop-up, except critical and verification codes.
    let on: bool = b.nyx("SetDnd", &(true,)).body().deserialize().unwrap();
    assert!(on);
    let quiet = std::fs::read_to_string(b.root.join("state/omarchy/notifications/quiet.json")).unwrap();
    assert!(quiet.contains("\"dnd\":true") && quiet.contains("codesBypassQuiet"), "Omarchy's file, its fields kept: {quiet}");
    b.notify("A", 0, "Hushed", "", &[], HashMap::new(), -1);
    b.notify("A", 0, "Urgent", "", &[], crit(), -1);
    b.notify("Bank", 0, "Your verification code", "482913", &[], HashMap::new(), -1);
    assert!(b.popup(1.0, "Urgent").is_some());
    assert!(b.popup(1.0, "Your verification code").is_some());
    assert!(b.popup(0.5, "Hushed").is_none());
    assert!(b.inbox().iter().any(|n| n["summary"] == "Hushed"), "still arrives in the Inbox");
    let now: bool = b.nyx("ToggleDnd", &()).body().deserialize().unwrap();
    assert!(!now);
}

#[test]
fn app_buttons_and_clicks_reach_the_app() {
    let Some(b) = bus("actions") else { return };
    let id = b.notify("Chat", 0, "Maya", "Are we on?", &["default", "Open", "reply", "Reply"], HashMap::new(), -1);
    let note = b.popup(2.0, "Maya").unwrap()["note"].clone();
    assert_eq!(note["app_actions"][1]["label"], "Reply");
    let row = note["id"].as_i64().unwrap();
    // The pop-up times out: still open, so its buttons still work from the Inbox.
    b.nyx("PopupEnded", &(row, "timeout"));
    assert_eq!(b.closed(0.5, id), None, "a pop-up timing out doesn't close the notification");
    let ok: bool = b.nyx("InvokeAction", &(row, "reply")).body().deserialize().unwrap();
    assert!(ok);
    let inv = b.wait(2.0, |n, _| n == "ActionInvoked").unwrap();
    assert_eq!(inv.body().deserialize::<(u32, String)>().unwrap(), (id, "reply".to_string()));
    assert!(b.wait(1.0, |n, _| n == "ActivationToken").is_some());
    assert_eq!(b.closed(2.0, id), Some(2));
    assert!(b.inbox().is_empty());
    // A click on another uses its default action.
    let id2 = b.notify("Chat", 0, "Leo", "", &["default", "Open"], HashMap::new(), -1);
    let row2 = b.popup(2.0, "Leo").unwrap()["note"]["id"].as_i64().unwrap();
    let ok: bool = b.nyx("Activate", &(row2,)).body().deserialize().unwrap();
    assert!(ok);
    assert!(
        b.wait(2.0, |n, m| n == "ActionInvoked"
            && m.body().deserialize::<(u32, String)>().is_ok_and(|(i, k)| i == id2 && k == "default"))
            .is_some()
    );
    // Dismissing from the panel tells the app too.
    let id3 = b.notify("Mail", 0, "Invoice", "", &[], HashMap::new(), -1);
    let row3 = b.popup(2.0, "Invoice").unwrap()["note"]["id"].as_i64().unwrap();
    b.nyx("Dismiss", &(vec![row3],));
    assert_eq!(b.closed(2.0, id3), Some(2));
}

#[test]
fn explicit_timeouts_close_and_transient_ones_skip_the_inbox() {
    let Some(b) = bus("timeouts") else { return };
    let low = HashMap::from([("urgency", Value::U8(0))]);
    let id = b.notify("Build", 0, "Done", "", &[], low, 1000);
    // Shown at least as long as Omarchy's minimum (5 s), then closed as expired.
    assert_eq!(b.closed(3.0, id), None);
    assert_eq!(b.closed(4.0, id), Some(1));
    let t = b.notify("Files", 0, "Copied", "", &[], HashMap::from([("transient", Value::Bool(true))]), -1);
    let row = b.popup(2.0, "Copied").unwrap()["note"]["id"].as_i64().unwrap();
    assert!(!b.inbox().iter().any(|n| n["summary"] == "Copied"));
    b.nyx("PopupEnded", &(row, "timeout"));
    assert_eq!(b.closed(2.0, t), Some(1), "a transient one closes with its pop-up");
}

#[test]
fn markup_images_and_hints() {
    let Some(b) = bus("hints") else { return };
    // 2×2 red pixels as image-data.
    let px: Vec<u8> = [255u8, 0, 0, 255].repeat(4);
    let img = Value::Structure(Structure::from((2i32, 2i32, 8i32, true, 8i32, 4i32, px)));
    let hints = HashMap::from([
        ("image-data", img),
        ("desktop-entry", Value::from("org.example.Chat")),
        ("omarchy-glyph", Value::from("󰍡")),
    ]);
    b.notify("Chat", 0, "<b>Maya</b>", "Hi &amp; <i>bye</i>", &[], hints, -1);
    let n = b.find("Maya");
    assert_eq!(n["body"], "Hi & bye");
    assert_eq!(n["desktop_entry"], "org.example.Chat");
    assert_eq!(n["glyph"], "󰍡");
    let image = n["image"].as_str().unwrap();
    assert!(image.ends_with("-2x2.png") && Path::new(image).is_file(), "saved as a PNG: {image}");
    // Omarchy's click command and buttons, checked.
    let hints = HashMap::from([
        ("omarchy-exec-argv", Value::from(r#"["omarchy-update"]"#)),
        ("omarchy-actions", Value::from(r#"[{"label":"Later","argv":["true"]}]"#)),
    ]);
    b.notify("omarchy-action", 0, "Update ready", "", &[], hints, -1);
    let n = b.find("Update ready");
    assert_eq!(n["exec_argv"][0], "omarchy-update");
    assert_eq!(n["actions"][0]["label"], "Later");
    assert!(!b.inbox().iter().any(|n| n["summary"] == "Update ready"), "Omarchy's passing messages skip the Inbox");
    let bad = HashMap::from([("omarchy-exec-argv", Value::from("rm -rf ~"))]);
    b.notify("x", 0, "Bad hint", "", &[], bad, -1);
    assert!(b.find("Bad hint")["exec_argv"].is_null(), "a malformed command is dropped");
}

#[test]
fn libnotify_notify_send() {
    let Some(b) = bus("notify-send") else { return };
    if !which("notify-send") {
        eprintln!("notify-send not installed; skipping");
        return;
    }
    let id = b.run("notify-send", &["--print-id", "-a", "Tester", "-u", "critical", "Hello", "From libnotify"]).unwrap();
    let id: u32 = id.parse().unwrap();
    let n = b.find("Hello");
    assert_eq!(n["app"], "Tester");
    assert_eq!(n["urgency"], 2);
    assert_eq!(n["live_id"], id);
    // --replace-id
    b.run("notify-send", &["--replace-id", &id.to_string(), "-a", "Tester", "Hello again"]).unwrap();
    assert!(b.inbox().iter().any(|n| n["summary"] == "Hello again" && n["live_id"] == id));
    assert!(!b.inbox().iter().any(|n| n["summary"] == "Hello"));
    // -e (transient)
    b.run("notify-send", &["-e", "Passing"]).unwrap();
    assert!(!b.inbox().iter().any(|n| n["summary"] == "Passing"));
}

#[test]
fn omarchy_notification_send() {
    let Some(b) = bus("omarchy-send") else { return };
    if !which("omarchy-notification-send") || !which("busctl") {
        eprintln!("omarchy-notification-send not installed; skipping");
        return;
    }
    let out = b
        .run(
            "omarchy-notification-send",
            &[
                "-p",
                "-g",
                "󰚰",
                "--app-name",
                "Omarchy Update",
                "Update ready",
                "12 packages",
                "--action",
                "Update",
                "omarchy-update",
            ],
        )
        .expect("omarchy-notification-send works against nyx-notifyd");
    let id: u32 = out.parse().unwrap();
    let n = b.find("Update ready");
    assert_eq!(n["live_id"], id);
    assert_eq!(n["glyph"], "󰚰");
    assert_eq!(n["actions"][0]["label"], "Update");
    assert_eq!(n["exec_argv"][0], "omarchy-update", "the first button is the click");
    // -r replaces in place.
    b.run("omarchy-notification-send", &["--app-name", "Omarchy Update", "-r", &id.to_string(), "Updating…"]).unwrap();
    assert!(b.find("Updating…")["live_id"] == id);
}

#[test]
fn omarchy_wait_sees_the_server() {
    let Some(b) = bus("wait") else { return };
    if !which("busctl") {
        return;
    }
    // omarchy-notification-wait's own check (the IPC half needs the shell plugin).
    assert!(b.run("busctl", &["--user", "call", FDO.0, FDO.1, FDO.2, "GetServerInformation"]).is_some());
}
