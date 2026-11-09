//! `nyx-notification-center-backend`: the Notification Center without GTK, for its
//! Quickshell interface (`qml/NotificationCenter` in this repository).
//!
//! nyx-notifyd stays as it is: this talks to it over `Notifications1` as the GTK panel
//! did. It owns `io.github.design_nexus.nyx.NotificationCenter` and serves `Panel1`
//! (`notification-center`) there, so keybinds, the bar button, Shell Core and the
//! Control Center see no difference. The interface talks to it over
//! `$XDG_RUNTIME_DIR/nyx-notification-center/ui.sock` (nyx-ui-link):
//!
//! - topic `panel`: `{open, view, settings, serial, screen}`;
//! - topic `lists`: `{inbox, pinned}`, read from nyx-notifyd only while the interface
//!   watches it (while the panel is open), again on every `Changed`, which also goes on
//!   as event `Changed`;
//! - topic `dnd`: Do not disturb; topic `mode`: `nyx` or `omarchy` (who shows pop-ups);
//! - topic `popups`: the pop-ups showing (see `popups.rs`);
//! - operations: `Show`, `Hide`, `Toggle`, and nyx-notifyd's calls (`History`,
//!   `Settings`, `Dismiss`, `Activate`, …) by the same names.
//!
//! Notes go out as nyx-notifyd gives them, plus `name` (the app's display name) and
//! `clickable`. Only the user's own processes can connect (nyx-ui-link).

mod client;
mod popups;

use gio::prelude::*;
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

pub const APP_ID: &str = "io.github.design_nexus.nyx.NotificationCenter";

#[derive(Default)]
pub struct PanelState {
    pub open: bool,
    pub view: String,
    pub settings: bool,
    pub serial: u64,
    pub screen: String,
}

pub struct App {
    pub link: nyx_ui_link::Link,
    pub panel: RefCell<PanelState>,
    pub panel1: RefCell<Option<nyx_desktop::panel::Panel>>,
    pub popups: popups::Popups,
    lists: RefCell<Option<gio::SignalSubscription>>,
    lists_pending: Cell<bool>,
    /// A test copy: its own socket, no bus name, and it changes nothing in nyx-notifyd
    /// that the user would notice (read marks, pop-ups ending).
    pub test: bool,
}

/// A display name: Omarchy's own messages say "Omarchy", not "omarchy-action".
pub fn app_name(app: &str) -> String {
    match app {
        "omarchy-action" => "Omarchy".into(),
        "notify-send" | "" => "Notification".into(),
        a => a.to_string(),
    }
}

/// The app's icon name is its desktop id (as Nyx apps and most others send it).
fn has_desktop(id: &str) -> bool {
    !id.is_empty() && !id.contains('/') && gio_unix::DesktopAppInfo::new(&format!("{id}.desktop")).is_some()
}

fn str_field<'a>(n: &'a Value, k: &str) -> &'a str {
    n.get(k).and_then(Value::as_str).unwrap_or_default()
}

/// A note for the interface: as nyx-notifyd gave it, with its app's name and whether a
/// click does anything.
pub fn note_json(mut n: Value) -> Value {
    let open = n.get("closed_reason").is_none_or(Value::is_null);
    let clickable = n.get("exec_argv").is_some_and(|v| !v.is_null())
        || (open
            && n.get("app_actions")
                .and_then(Value::as_array)
                .is_some_and(|a| a.iter().any(|a| a.get("key").and_then(Value::as_str) == Some("default"))))
        || has_desktop(str_field(&n, "desktop_entry"))
        || has_desktop(str_field(&n, "app_icon"));
    let name = app_name(str_field(&n, "app"));
    let image = str_field(&n, "image");
    let image_ok = !image.is_empty() && std::path::Path::new(image).is_file();
    if let Some(o) = n.as_object_mut() {
        o.insert("name".into(), json!(name));
        o.insert("clickable".into(), json!(clickable));
        o.insert("open".into(), json!(open));
        if !image_ok {
            o.insert("image".into(), json!(""));
        }
    }
    n
}

fn notes(v: Value) -> Value {
    match v {
        Value::Array(a) => Value::Array(a.into_iter().map(note_json).collect()),
        other => other,
    }
}

/// The focused screen's name.
pub fn focused_screen() -> Option<String> {
    nyx_desktop::hypr::Instance::find()
        .ok()?
        .json::<Vec<Value>>("monitors")
        .ok()?
        .into_iter()
        .find(|m| m["focused"].as_bool() == Some(true))
        .and_then(|m| m["name"].as_str().map(str::to_string))
}

impl App {
    pub fn publish_panel(&self) {
        let p = self.panel.borrow();
        self.link.publish(
            "panel",
            json!({"open": p.open, "view": p.view, "settings": p.settings, "serial": p.serial, "screen": p.screen}),
        );
    }

    /// Open (on `view`, or settings); the Control Center closes (Panel1).
    pub fn show(&self, view: Option<String>, settings: bool, announce: bool) {
        {
            let mut p = self.panel.borrow_mut();
            p.open = true;
            if let Some(v) = view {
                p.view = v;
            }
            p.settings = settings;
            p.serial += 1;
            p.screen = focused_screen().unwrap_or_default();
        }
        // Pop-ups make way: everything they showed is in here.
        popups::clear(self);
        self.publish_panel();
        if announce && let Some(p) = self.panel1.borrow().as_ref() {
            p.announce();
        }
        publish_mode(self);
    }

    pub fn hide(&self) {
        let was = std::mem::replace(&mut self.panel.borrow_mut().open, false);
        if was {
            self.panel.borrow_mut().settings = false;
            self.publish_panel();
            // What was on screen has been seen.
            if !self.test {
                glib::spawn_future_local(async {
                    let _ = client::call("MarkAllRead", None).await;
                });
            }
        }
    }

    /// `--toggle [view]`: open on that view; close if it's what's showing.
    pub fn toggle(&self, view: Option<String>) {
        let (open, current, settings) = {
            let p = self.panel.borrow();
            (p.open, p.view.clone(), p.settings)
        };
        if open && !settings && view.as_ref().is_none_or(|v| *v == current) {
            self.hide();
        } else {
            self.show(view, false, true);
        }
    }
}

fn publish_mode(app: &App) {
    let link = app.link.clone();
    glib::spawn_future_local(async move {
        let mode = client::mode().await.unwrap_or_else(|_| "omarchy".into());
        link.publish("mode", json!(mode));
    });
}

/// Read the Inbox and the pins (once, however many changes arrive together).
fn refresh_lists(app: &Rc<App>) {
    if app.lists_pending.replace(true) {
        return;
    }
    let weak = Rc::downgrade(app);
    glib::spawn_future_local(async move {
        glib::timeout_future(Duration::from_millis(30)).await;
        let Some(app) = weak.upgrade() else { return };
        app.lists_pending.set(false);
        if app.lists.borrow().is_none() {
            return;
        }
        let (inbox, pinned) = (client::json("Inbox", None).await, client::json("Pinned", None).await);
        let v = match inbox {
            Ok(inbox) => json!({"inbox": notes(inbox), "pinned": notes(pinned.unwrap_or(json!([]))), "error": null}),
            Err(e) => json!({"inbox": [], "pinned": [], "error": e}),
        };
        app.link.publish("lists", v);
        app.link.emit("Changed", json!({}));
    });
}

fn watched(app: &Rc<App>, topic: &str, n: usize) {
    if topic != "lists" {
        return;
    }
    if n == 0 {
        app.lists.borrow_mut().take();
        return;
    }
    if app.lists.borrow().is_none() {
        let weak = Rc::downgrade(app);
        let sub = client::on_changed(move || {
            if let Some(app) = weak.upgrade() {
                refresh_lists(&app);
            }
        });
        *app.lists.borrow_mut() = sub;
        refresh_lists(app);
    }
}

/// Omarchy's quiet.json says whether Do not disturb is on, whoever receives notifications.
fn follow_dnd(app: &Rc<App>) {
    let dir = nyx_notify_store::omarchy::dir();
    app.link.publish("dnd", json!(nyx_notify_store::omarchy::dnd(&dir)));
    let _ = std::fs::create_dir_all(&dir);
    let Ok(m) = gio::File::for_path(&dir).monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) else {
        return;
    };
    let link = app.link.clone();
    m.connect_changed(move |_, _, _, _| {
        link.publish("dnd", json!(nyx_notify_store::omarchy::dnd(&nyx_notify_store::omarchy::dir())));
    });
    std::mem::forget(m);
}

/// Keep the theme snapshot current for Quickshell parts where no theme service runs, as
/// the GTK panel did through `nyx_theme::use_shell_scope()`.
fn follow_theme() {
    nyx_theme_core::notify_changed("system", "shell");
    let pending = Rc::new(Cell::new(false));
    let mut monitors = Vec::new();
    for dir in nyx_theme_core::paths::watched_dirs() {
        let Ok(m) = gio::File::for_path(&dir).monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) else {
            continue;
        };
        let pending = pending.clone();
        m.connect_changed(move |_, file, _, _| {
            if file.basename().is_some_and(|b| b.to_string_lossy().ends_with(".tmp")) || pending.replace(true) {
                return;
            }
            let pending = pending.clone();
            glib::timeout_add_local_once(Duration::from_millis(120), move || {
                pending.set(false);
                nyx_theme_core::notify_changed("system", "shell");
            });
        });
        monitors.push(m);
    }
    std::mem::forget(monitors);
}

/// A code goes to `nyx-notification-center --copy-code` on its standard input: Nyx
/// Clipboard takes codes only from the Notification Center's own program, and a command
/// line would show the code to every process.
fn copy_code(code: &str) -> Result<(), String> {
    use std::io::Write;
    let program = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("nyx-notification-center")))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| "nyx-notification-center".into());
    let mut child = std::process::Command::new(program)
        .arg("--copy-code")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    child.stdin.take().ok_or("no stdin")?.write_all(code.as_bytes()).map_err(|e| e.to_string())?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

fn ids(b: &Value) -> Vec<i64> {
    b.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default()
}

fn handle(app: &Rc<App>, req: nyx_ui_link::Request) {
    let b = req.body.clone();
    let id = b.get("id").and_then(Value::as_i64).unwrap_or(0);
    let view = b.get("view").and_then(Value::as_str).filter(|v| ["inbox", "pinned", "history"].contains(v)).map(String::from);
    match req.op.as_str() {
        "Show" => {
            app.show(view, b.get("settings").and_then(Value::as_bool).unwrap_or(false), true);
            req.ok(json!({}))
        }
        "Hide" => {
            app.hide();
            req.ok(json!({}))
        }
        "Toggle" => {
            app.toggle(view);
            req.ok(json!({}))
        }
        "Settings" => {
            app.panel.borrow_mut().settings = b.get("open").and_then(Value::as_bool).unwrap_or(true);
            app.publish_panel();
            req.ok(json!({}))
        }
        "PopupHold" => {
            popups::hold(app, id, b.get("on").and_then(Value::as_bool).unwrap_or(false));
            req.ok(json!({}))
        }
        "PopupDismiss" => {
            if app.test {
                popups::remove(app, id);
            } else {
                popups::end(app, id, "dismissed");
            }
            req.ok(json!({}))
        }
        "CopyCode" => match copy_code(b.get("code").and_then(Value::as_str).unwrap_or_default()) {
            Ok(()) => req.ok(json!({})),
            Err(e) => req.err("failed", e),
        },
        op => {
            let op = op.to_string();
            glib::spawn_future_local(async move {
                let r: Result<Value, String> = match op.as_str() {
                    "GetSettings" => client::json("Settings", None).await,
                    "SetSettings" => {
                        let s = b.get("settings").cloned().unwrap_or(Value::Null).to_string();
                        client::json("SetSettings", Some((s,).to_variant())).await
                    }
                    "History" => {
                        let q = json!({
                            "text": b.get("text").and_then(Value::as_str).unwrap_or_default(),
                            "app": b.get("app").and_then(Value::as_str).unwrap_or_default(),
                            "days": b.get("days").and_then(Value::as_u64).unwrap_or(0),
                            "limit": b.get("limit").and_then(Value::as_u64).unwrap_or(300),
                        });
                        client::json("History", Some((q.to_string(),).to_variant())).await.map(notes)
                    }
                    "Apps" => client::json("Apps", None).await.map(|v| match v {
                        Value::Array(a) => Value::Array(
                            a.into_iter()
                                .map(|mut x| {
                                    let name = app_name(str_field(&x, "app"));
                                    if let Some(o) = x.as_object_mut() {
                                        o.insert("name".into(), json!(name));
                                    }
                                    x
                                })
                                .collect(),
                        ),
                        o => o,
                    }),
                    "Mode" => client::mode().await.map(Value::from),
                    "SetPinned" => {
                        let on = b.get("pinned").and_then(Value::as_bool).unwrap_or(false);
                        client::call("SetPinned", Some((id, on).to_variant())).await.map(|_| json!({}))
                    }
                    "Dismiss" | "Delete" => client::call(&op, Some((ids(&b),).to_variant())).await.map(|_| json!({})),
                    "DismissAll" | "MarkAllRead" | "DeleteHistory" | "ToggleDnd" => {
                        client::call(&op, None).await.map(|_| json!({}))
                    }
                    "Activate" => client::flag("Activate", Some((id,).to_variant())).await.map(Value::from),
                    "InvokeAction" => {
                        let key = b.get("key").and_then(Value::as_str).unwrap_or_default().to_string();
                        client::flag("InvokeAction", Some((id, key).to_variant())).await.map(Value::from)
                    }
                    "RunAction" => {
                        let index = b.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
                        client::flag("RunAction", Some((id, index).to_variant())).await.map(Value::from)
                    }
                    _ => {
                        req.err("unknown_op", op);
                        return;
                    }
                };
                match r {
                    Ok(v) => req.ok(v),
                    Err(e) => req.err("failed", e),
                }
            });
        }
    }
}

/// `NYX_NC_TEST_POPUP=1` in a test copy: a pop-up to look at, without a notification.
fn test_popup(app: &Rc<App>) {
    let note = json!({
        "id": -1, "key": "", "app": "Nyx Mail", "app_icon": "io.github.design_nexus.nyx.Mail", "glyph": "",
        "summary": "Ada Lovelace", "body": "The engine is ready for its first run. Your code is 482913, valid for ten minutes.",
        "urgency": 1, "exec_argv": null, "actions": [], "ts": nyx_notify_store::now(), "closed_ts": null,
        "closed_reason": null, "live": true, "read": false, "dismissed": false, "pinned": false,
        "app_actions": [{"key": "reply", "label": "Reply"}, {"key": "read", "label": "Mark as read"}],
        "desktop_entry": "", "image": "", "codes": ["482913"], "live_id": 1, "transient": false
    });
    popups::show(app, &json!({"note": note, "timeout_ms": 0, "hide_text": false}).to_string());
}

fn main() -> std::process::ExitCode {
    if std::env::args().any(|a| a == "-h" || a == "--help") {
        println!(
            "Usage: nyx-notification-center-backend\n\nThe Notification Center's services for its Quickshell interface (nyx-notification-center --ui qml)."
        );
        return std::process::ExitCode::SUCCESS;
    }
    let test = std::env::var_os("NYX_NOTIFICATION_CENTER_UI_SOCKET").is_some();
    let ctx = glib::MainContext::default();
    let _guard = ctx.acquire().expect("main context");

    let (req_tx, req_rx) = async_channel::unbounded::<nyx_ui_link::Request>();
    let (watch_tx, watch_rx) = async_channel::unbounded::<(String, usize)>();
    let mut options = nyx_ui_link::Options::new("nyx-notification-center");
    options.on_watch = Some(std::sync::Arc::new(move |t: &str, n| {
        let _ = watch_tx.send_blocking((t.to_string(), n));
    }));
    let link = match nyx_ui_link::serve(options, move |r| {
        let _ = req_tx.send_blocking(r);
    }) {
        Ok(l) => l,
        Err(nyx_ui_link::Error::AlreadyRunning(p)) => {
            eprintln!("nyx-notification-center-backend: already running at {}", p.display());
            return std::process::ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("nyx-notification-center-backend: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let app = Rc::new(App {
        link,
        panel: RefCell::new(PanelState { view: "inbox".into(), ..Default::default() }),
        panel1: RefCell::default(),
        popups: popups::Popups::default(),
        lists: RefCell::default(),
        lists_pending: Cell::new(false),
        test,
    });
    app.publish_panel();
    app.link.publish("lists", json!({"inbox": [], "pinned": [], "error": null}));
    popups::publish(&app);
    publish_mode(&app);
    follow_dnd(&app);
    follow_theme();

    let (a1, a2) = (Rc::downgrade(&app), Rc::downgrade(&app));
    let subs = client::on_popups(
        move |payload| {
            if let Some(app) = a1.upgrade() {
                popups::show(&app, &payload);
            }
        },
        move |id| {
            if let Some(app) = a2.upgrade() {
                popups::remove(&app, id);
            }
        },
    );
    std::mem::forget(subs);
    if test && std::env::var_os("NYX_NC_TEST_POPUP").is_some() {
        test_popup(&app);
    }

    if !test {
        match gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
            Ok(conn) => {
                let (s, h, o) = (Rc::downgrade(&app), Rc::downgrade(&app), Rc::downgrade(&app));
                let handlers = nyx_desktop::panel::Handlers {
                    // Panel1 announces itself after `show`.
                    show: Box::new(move || {
                        if let Some(app) = s.upgrade() {
                            app.show(None, false, false);
                        }
                    }),
                    hide: Box::new(move || {
                        if let Some(app) = h.upgrade() {
                            app.hide();
                        }
                    }),
                    is_open: Box::new(move || o.upgrade().is_some_and(|app| app.panel.borrow().open)),
                };
                match nyx_desktop::panel::Panel::serve(&conn, "notification-center", handlers) {
                    Ok(p) => *app.panel1.borrow_mut() = Some(p),
                    Err(e) => eprintln!("nyx-notification-center-backend: Panel1: {e}"),
                }
                gio::bus_own_name_on_connection(
                    &conn,
                    APP_ID,
                    gio::BusNameOwnerFlags::REPLACE | gio::BusNameOwnerFlags::ALLOW_REPLACEMENT,
                    |_, _| {},
                    |_, name| eprintln!("nyx-notification-center-backend: {name} is taken (is the GTK panel running?)"),
                );
                std::mem::forget(conn);
            }
            Err(e) => eprintln!("nyx-notification-center-backend: no session bus: {e}"),
        }
    }

    let a = app.clone();
    ctx.spawn_local(async move {
        while let Ok((topic, n)) = watch_rx.recv().await {
            watched(&a, &topic, n);
        }
    });
    let a = app.clone();
    ctx.spawn_local(async move {
        while let Ok(req) = req_rx.recv().await {
            handle(&a, req);
        }
    });
    glib::MainLoop::new(Some(&ctx), false).run();
    std::process::ExitCode::SUCCESS
}
