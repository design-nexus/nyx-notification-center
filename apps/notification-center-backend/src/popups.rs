//! Pop-ups, as the GTK `popups.rs`: nyx-notifyd says when one should show (`Popup`) and
//! when its notification closed (`PopupClosed`). Each stays for its time (0: until
//! dismissed), held while the pointer is on it; at most four, the oldest going first;
//! when one goes, the notification stays in the Inbox. Published as topic `popups`:
//! `{screen, items: [{note, hide_text}]}`, newest first.
//!
//! In Omarchy mode nothing is ever announced, so there are none.

use crate::App;
use glib::prelude::ToVariant;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

const MAX: usize = 4;

struct Toast {
    id: i64,
    note: Value,
    hide_text: bool,
    timeout_ms: u32,
    timer: Option<glib::SourceId>,
    held: bool,
}

#[derive(Default)]
pub struct Popups {
    /// Oldest first.
    toasts: RefCell<Vec<Toast>>,
    screen: RefCell<String>,
}

#[derive(serde::Deserialize)]
struct Payload {
    note: Value,
    #[serde(default)]
    timeout_ms: u32,
    #[serde(default)]
    hide_text: bool,
}

pub fn publish(app: &App) {
    let p = &app.popups;
    let items: Vec<Value> =
        p.toasts.borrow().iter().rev().map(|t| json!({"id": t.id, "note": t.note, "hide_text": t.hide_text})).collect();
    app.link.publish("popups", json!({"screen": *p.screen.borrow(), "items": items}));
}

/// Take one down without telling anyone (its notification is already closed).
pub fn remove(app: &App, id: i64) {
    let gone = {
        let mut t = app.popups.toasts.borrow_mut();
        let at = t.iter().position(|t| t.id == id);
        at.map(|i| t.remove(i))
    };
    if let Some(mut t) = gone {
        if let Some(timer) = t.timer.take() {
            timer.remove();
        }
        publish(app);
    }
}

/// Its time is up, or it was dismissed: it goes; nyx-notifyd hears why.
pub fn end(app: &App, id: i64, reason: &'static str) {
    remove(app, id);
    if app.test {
        return;
    }
    glib::spawn_future_local(async move {
        if let Err(e) = crate::client::call("PopupEnded", Some((id, reason).to_variant())).await {
            eprintln!("nyx-notification-center-backend: {e}");
        }
    });
}

/// Every pop-up goes (the panel opened; they're all in it).
pub fn clear(app: &App) {
    let ids: Vec<i64> = app.popups.toasts.borrow().iter().map(|t| t.id).collect();
    for id in ids {
        end(app, id, "timeout");
    }
}

fn start_timer(app: &Rc<App>, id: i64) {
    let mut toasts = app.popups.toasts.borrow_mut();
    let Some(t) = toasts.iter_mut().find(|t| t.id == id) else { return };
    if let Some(old) = t.timer.take() {
        old.remove();
    }
    if t.timeout_ms == 0 || t.held {
        return;
    }
    let weak = Rc::downgrade(app);
    t.timer = Some(glib::timeout_add_local_once(Duration::from_millis(u64::from(t.timeout_ms)), move || {
        let Some(app) = weak.upgrade() else { return };
        if let Some(t) = app.popups.toasts.borrow_mut().iter_mut().find(|t| t.id == id) {
            t.timer = None;
        }
        end(&app, id, "timeout");
    }));
}

/// The pointer is on it (its time stops) or left it (its time starts again).
pub fn hold(app: &Rc<App>, id: i64, on: bool) {
    if let Some(t) = app.popups.toasts.borrow_mut().iter_mut().find(|t| t.id == id) {
        t.held = on;
    }
    start_timer(app, id);
}

pub fn show(app: &Rc<App>, payload: &str) {
    let Ok(p) = serde_json::from_str::<Payload>(payload) else { return };
    let Some(id) = p.note.get("id").and_then(Value::as_i64) else { return };
    // The panel is open: it shows the notification already.
    if app.panel.borrow().open {
        glib::spawn_future_local(async move {
            let _ = crate::client::call("PopupEnded", Some((id, "timeout").to_variant())).await;
        });
        return;
    }
    let note = crate::note_json(p.note);
    {
        let mut toasts = app.popups.toasts.borrow_mut();
        if toasts.is_empty() {
            *app.popups.screen.borrow_mut() = crate::focused_screen().unwrap_or_default();
        }
        // Replaced: same place, new content. Otherwise the newest.
        if let Some(t) = toasts.iter_mut().find(|t| t.id == id) {
            t.note = note;
            t.hide_text = p.hide_text;
            t.timeout_ms = p.timeout_ms;
        } else {
            toasts.push(Toast { id, note, hide_text: p.hide_text, timeout_ms: p.timeout_ms, timer: None, held: false });
        }
    }
    start_timer(app, id);
    loop {
        let oldest = {
            let t = app.popups.toasts.borrow();
            (t.len() > MAX).then(|| t[0].id)
        };
        match oldest {
            Some(o) => end(app, o, "timeout"),
            None => break,
        }
    }
    publish(app);
}
