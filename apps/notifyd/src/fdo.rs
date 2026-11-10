//! `org.freedesktop.Notifications` (stage B, Nyx mode): what every Linux app sends
//! notifications to. Each one is stored at once; the panel's pop-ups are told by signal.
//! Pop-ups that time out leave the notification open in the Inbox, so the app's own
//! buttons keep working from there; it's closed (and the app told) when it's clicked,
//! dismissed, cleared, withdrawn by the app, or its explicit timeout runs out.

use crate::state::{self, State};
use nyx_notify_store::{AppAction, Incoming};
use std::collections::HashMap;
use std::sync::Arc;
use zbus::message::Header;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{fdo, interface};

pub const PATH: &str = "/org/freedesktop/Notifications";
pub const NAME: &str = "org.freedesktop.Notifications";

pub struct Notifications {
    pub state: Arc<State>,
}

/// Turn markup into plain text: tags dropped, entities read, links reduced to their text.
pub fn plain(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut in_tag = false;
    for c in body.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    // A lone "<" (as in "a < b") never closed: keep the text as sent.
    if in_tag {
        out = body.to_string();
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

fn string(v: &OwnedValue) -> Option<String> {
    match &**v {
        Value::Str(s) => Some(s.to_string()),
        Value::ObjectPath(p) => Some(p.to_string()),
        _ => None,
    }
}

fn number(v: &OwnedValue) -> Option<i64> {
    match &**v {
        Value::U8(n) => Some(i64::from(*n)),
        Value::I16(n) => Some(i64::from(*n)),
        Value::U16(n) => Some(i64::from(*n)),
        Value::I32(n) => Some(i64::from(*n)),
        Value::U32(n) => Some(i64::from(*n)),
        Value::I64(n) => Some(*n),
        _ => None,
    }
}

fn boolean(v: &OwnedValue) -> bool {
    match &**v {
        Value::Bool(b) => *b,
        _ => number(v).is_some_and(|n| n != 0),
    }
}

/// An argv as Omarchy accepts it: a JSON array of 1–32 strings without NULs.
fn valid_argv(v: &serde_json::Value) -> bool {
    v.as_array().is_some_and(|a| {
        !a.is_empty() && a.len() <= 32 && a.iter().all(|s| s.as_str().is_some_and(|s| !s.contains('\0') && s.len() <= 4096))
    })
}

/// Omarchy's `omarchy-exec-argv` hint, checked; `""` if it isn't usable.
pub fn exec_argv(hint: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(hint) {
        Ok(v) if valid_argv(&v) => v.to_string(),
        _ => String::new(),
    }
}

/// Omarchy's `omarchy-actions` hint (`[{label, argv}]`, at most 3), checked.
pub fn exec_actions(hint: &str) -> String {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(hint) else { return String::new() };
    if items.is_empty() || items.len() > 3 {
        return String::new();
    }
    let ok = items
        .iter()
        .all(|i| i["label"].as_str().is_some_and(|l| !l.trim().is_empty() && l.len() <= 64) && valid_argv(&i["argv"]));
    if ok { serde_json::Value::Array(items).to_string() } else { String::new() }
}

fn raw_image(v: &OwnedValue) -> Option<crate::image::Raw> {
    let Value::Structure(s) = &**v else { return None };
    let f = s.fields();
    if f.len() != 7 {
        return None;
    }
    let int = |i: usize| match &f[i] {
        Value::I32(n) => Some(*n),
        _ => None,
    };
    let alpha = matches!(&f[3], Value::Bool(true));
    let Value::Array(a) = &f[6] else { return None };
    let data: Vec<u8> = a.iter().filter_map(|x| if let Value::U8(b) = x { Some(*b) } else { None }).collect();
    Some((int(0)?, int(1)?, int(2)?, alpha, int(4)?, int(5)?, data))
}

/// What arrived, as the store keeps it.
pub fn incoming(
    app_name: &str,
    app_icon: &str,
    summary: &str,
    body: &str,
    actions: &[String],
    hints: &HashMap<String, OwnedValue>,
    sender: &str,
) -> Incoming {
    let hint = |k: &str| hints.get(k);
    let mut app_actions = Vec::new();
    for pair in actions.chunks(2) {
        if let [key, label] = pair
            && !key.is_empty()
            && app_actions.len() < 6
        {
            app_actions.push(AppAction { key: key.clone(), label: plain(label) });
        }
    }
    let desktop_entry = hint("desktop-entry").and_then(string).unwrap_or_default();
    // A picture: a file named by the sender, else raw pixels, else an icon that's a file.
    let image = ["image-path", "image_path"]
        .iter()
        .find_map(|k| hint(k).and_then(string))
        .map(|p| p.trim_start_matches("file://").to_string())
        .filter(|p| p.starts_with('/') && std::path::Path::new(p).is_file())
        .or_else(|| {
            ["image-data", "image_data", "icon_data"]
                .iter()
                .find_map(|k| hint(k).and_then(raw_image))
                .and_then(|raw| crate::image::save(&raw))
                .map(|p| p.display().to_string())
        })
        .unwrap_or_default();
    let glyph = hint("omarchy-glyph").and_then(string).filter(|g| g.chars().count() <= 4 && !g.chars().any(char::is_control));
    let app = if app_name.trim().is_empty() { "notify-send".to_string() } else { app_name.trim().chars().take(80).collect() };
    let icon = if app_icon.is_empty() { desktop_entry.clone() } else { app_icon.to_string() };
    Incoming {
        app,
        app_icon: icon,
        desktop_entry,
        image,
        glyph: glyph.unwrap_or_default(),
        summary: plain(summary).chars().take(400).collect(),
        body: plain(body).chars().take(8000).collect(),
        urgency: hint("urgency").and_then(number).unwrap_or(1).clamp(0, 2),
        exec_argv: hint("omarchy-exec-argv").and_then(string).map(|s| exec_argv(&s)).unwrap_or_default(),
        exec_actions: hint("omarchy-actions").and_then(string).map(|s| exec_actions(&s)).unwrap_or_default(),
        app_actions,
        sender: sender.to_string(),
        transient: hint("transient").is_some_and(boolean),
        device: String::new(),
        source_key: String::new(),
    }
}

#[interface(name = "org.freedesktop.Notifications")]
impl Notifications {
    fn get_capabilities(&self) -> Vec<&'static str> {
        vec!["actions", "body", "icon-static", "persistence"]
    }

    fn get_server_information(&self) -> (&'static str, &'static str, &'static str, &'static str) {
        ("Nyx Notification Center", "design-nexus", env!("CARGO_PKG_VERSION"), "1.2")
    }

    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> fdo::Result<u32> {
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        let mut inc = incoming(app_name, app_icon, summary, body, &actions, &hints, &sender);
        let mut replaces_id = replaces_id;
        // A phone's notification, relayed by Flux.
        if crate::flux::is_fluxd(conn, &sender).await {
            let (device, key) = crate::flux::source(app_name, &actions, crate::flux::device_named);
            if replaces_id == 0
                && let Ok(Some(live)) = self.state.with(|s| s.open_source(&key))
            {
                replaces_id = live;
            }
            (inc.device, inc.source_key) = (device, key);
        }
        state::arrive(&self.state, replaces_id, inc, expire_timeout).await
    }

    async fn close_notification(&self, id: u32) -> fdo::Result<()> {
        state::close_live(&self.state, id, "closed").await;
        Ok(())
    }

    #[zbus(signal)]
    pub async fn notification_closed(e: &zbus::object_server::SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn action_invoked(e: &zbus::object_server::SignalEmitter<'_>, id: u32, action_key: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn activation_token(e: &zbus::object_server::SignalEmitter<'_>, id: u32, token: &str) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_becomes_text() {
        assert_eq!(plain("<b>Bold</b> &amp; <a href=\"x\">link</a>"), "Bold & link");
        assert_eq!(plain("a < b"), "a < b");
        assert_eq!(plain("5 &lt; 6"), "5 < 6");
    }

    #[test]
    fn omarchy_hints_are_checked() {
        assert_eq!(exec_argv(r#"["omarchy-update"]"#), r#"["omarchy-update"]"#);
        assert_eq!(exec_argv(r#""rm -rf""#), "");
        assert_eq!(exec_argv("[]"), "");
        assert_eq!(exec_argv("[1]"), "");
        assert!(!exec_actions(r#"[{"label":"Later","argv":["true"]}]"#).is_empty());
        assert_eq!(exec_actions(r#"[{"label":"","argv":["true"]}]"#), "");
        assert_eq!(exec_actions(r#"[{"label":"x","argv":"true"}]"#), "");
    }

    #[test]
    fn actions_pair_up_and_names_default() {
        let hints = HashMap::new();
        let i = incoming(
            "",
            "",
            "<i>Hi</i>",
            "",
            &["default".into(), "Open".into(), "reply".into(), "Reply".into(), "odd".into()],
            &hints,
            ":1.5",
        );
        assert_eq!(i.app, "notify-send");
        assert_eq!(i.summary, "Hi");
        assert_eq!(i.app_actions.len(), 2);
        assert_eq!(i.app_actions[1].label, "Reply");
        assert_eq!(i.urgency, 1);
    }
}
