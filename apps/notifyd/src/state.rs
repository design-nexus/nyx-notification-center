//! The service's shared state and the things both interfaces do: take a notification
//! in, close one (telling its app), act on a click, and keep the bar's count current.

use crate::{fdo, launch, quiet};
use nyx_notify_store::{Incoming, Note, Store, now};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Duration;
use zbus::fdo as zfdo;

pub const PATH: &str = "/io/github/design_nexus/nyx/Notifications";
pub const IFACE: &str = "io.github.design_nexus.nyx.Notifications1";

pub struct State {
    pub store: Mutex<Store>,
    /// Stage B: this service is the notification server.
    pub nyx_mode: bool,
    /// Omarchy's notification folder (quiet.json; in Omarchy mode, its history too).
    pub dir: PathBuf,
    /// Wakes the schedule when the settings change.
    pub settings_changed: Mutex<mpsc::Sender<()>>,
    /// Bumped on every arrival, so a replaced notification's old timeout is ignored.
    pub generation: Mutex<HashMap<u32, u64>>,
    pub conn: OnceLock<zbus::Connection>,
}

pub fn err(e: impl std::fmt::Display) -> zfdo::Error {
    zfdo::Error::Failed(e.to_string())
}

impl State {
    pub fn with<T>(&self, f: impl FnOnce(&mut Store) -> nyx_notify_store::Result<T>) -> zfdo::Result<T> {
        let mut s = self.store.lock().map_err(err)?;
        f(&mut s).map_err(err)
    }

    fn conn(&self) -> Option<&zbus::Connection> {
        self.conn.get()
    }

    pub fn dnd(&self) -> bool {
        quiet::get(&self.dir)
    }

    /// Switch Do not disturb: written here in Nyx mode, asked of Omarchy otherwise.
    pub fn set_dnd(&self, on: bool) -> bool {
        if self.dnd() == on {
            return true;
        }
        if self.nyx_mode {
            quiet::write(&self.dir, on).map_err(|e| eprintln!("nyx-notifyd: quiet.json: {e}")).is_ok()
        } else {
            quiet::ask_omarchy(on)
        }
    }
}

/// What the bar button shows, in a small file it watches: `{"unread": n, "inbox": n}`
/// in `$XDG_STATE_HOME/nyx/notifications.json`.
pub fn publish(store: &Store) {
    let Ok(inbox) = store.inbox() else { return };
    let unread = inbox.iter().filter(|n| !n.read).count();
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"))
        .join("nyx");
    let _ = std::fs::create_dir_all(&state);
    let text = format!("{{\"unread\": {unread}, \"inbox\": {}}}\n", inbox.len());
    let path = state.join("notifications.json");
    if std::fs::read_to_string(&path).ok().as_deref() == Some(text.as_str()) {
        return;
    }
    let tmp = state.join(".notifications.json.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

async fn emit<B: serde::Serialize + zbus::zvariant::DynamicType>(st: &State, path: &str, iface: &str, name: &str, body: &B) {
    if let Some(c) = st.conn() {
        let _ = c.emit_signal(None::<()>, path, iface, name, body).await;
    }
}

/// Something changed: update the bar's file and tell the panel.
pub async fn changed(st: &State) {
    if let Ok(s) = st.store.lock() {
        publish(&s);
    }
    emit(st, PATH, IFACE, "Changed", &()).await;
}

/// The freedesktop reason codes: 1 expired, 2 dismissed by the user, 3 closed by the app.
fn reason_code(reason: &str) -> u32 {
    match reason {
        "expired" => 1,
        "dismissed" | "activated" => 2,
        "closed" => 3,
        _ => 4,
    }
}

/// Tell the apps whose notifications were closed, and the pop-ups to go.
async fn closed(st: &State, notes: &[(i64, Option<u32>)], reason: &str) {
    for (id, live) in notes {
        emit(st, PATH, IFACE, "PopupClosed", &(*id,)).await;
        if let (Some(l), true) = (live, st.nyx_mode) {
            emit(st, fdo::PATH, fdo::NAME, "NotificationClosed", &(*l, reason_code(reason))).await;
        }
    }
}

/// Close notification rows (if still open) for `reason`, telling everyone concerned.
pub async fn close_notes(st: &State, ids: &[i64], reason: &str) -> zfdo::Result<bool> {
    let open: Vec<(i64, Option<u32>)> = st.with(|s| {
        let open = s.open_among(ids)?;
        let mut out = Vec::new();
        for id in open {
            out.push((id, s.get(id)?.and_then(|n| n.live_id)));
        }
        s.close(ids, reason, now())?;
        Ok(out)
    })?;
    closed(st, &open, reason).await;
    Ok(!open.is_empty())
}

/// Close the one an app knows as `live` (CloseNotification, or its timeout).
pub async fn close_live(st: &State, live: u32, reason: &str) {
    let id = st.with(|s| s.close_live(live, reason, now())).ok().flatten();
    if let Some(id) = id {
        closed(st, &[(id, Some(live))], reason).await;
        changed(st).await;
    }
}

fn popup_ms(urgency: i64, requested: i32) -> u32 {
    if urgency >= 2 {
        return 0;
    }
    let base: i32 = if urgency == 0 { 5000 } else { 8000 };
    base.max(requested).min(30_000) as u32
}

/// A notification arrives: store it, then show it (unless quiet) and start its timeout.
pub async fn arrive(st: &Arc<State>, replaces: u32, inc: Incoming, expire_timeout: i32) -> zfdo::Result<u32> {
    let (note, settings) = st.with(|s| {
        let live = if replaces > 0 && s.open_live(replaces)?.is_some() { replaces } else { s.next_live_id()? };
        Ok((s.notify(live, &inc, now())?, s.settings()))
    })?;
    let live = note.live_id.unwrap_or_default();
    let generation = {
        let mut g = st.generation.lock().map_err(err)?;
        let e = g.entry(live).or_insert(0);
        *e += 1;
        *e
    };
    // Quiet unless critical or a verification code (asked for a moment ago).
    let loud = !st.dnd() || note.urgency >= 2 || !note.codes.is_empty();
    if loud && !settings.no_popups.contains(&note.app) {
        let payload = serde_json::json!({
            "note": note,
            "timeout_ms": popup_ms(note.urgency, expire_timeout),
            "hide_text": settings.hide_popup_text,
        });
        emit(st, PATH, IFACE, "Popup", &(payload.to_string(),)).await;
    } else {
        let _ = st.with(|s| s.popup_over(live));
        if note.transient {
            let _ = st.with(|s| s.close_live(live, "expired", now()));
        }
    }
    // An explicit timeout closes it (and tells the app) when it runs out.
    if expire_timeout > 0 {
        let wait = Duration::from_millis(u64::from(popup_ms(note.urgency.min(1), expire_timeout)));
        let st2 = st.clone();
        std::thread::spawn(move || {
            std::thread::sleep(wait);
            let current = st2.generation.lock().map(|g| g.get(&live) == Some(&generation)).unwrap_or(false);
            if current {
                zbus::block_on(close_live(&st2, live, "expired"));
            }
        });
    }
    changed(st).await;
    Ok(live)
}

/// A pop-up finished: `timeout` leaves the notification open (in the Inbox; a
/// transient one closes), `dismissed` closes it.
pub async fn popup_ended(st: &State, id: i64, reason: &str) -> zfdo::Result<()> {
    let Some(note) = st.with(|s| s.get(id))? else { return Ok(()) };
    if reason == "dismissed" {
        close_notes(st, &[id], "dismissed").await?;
    } else if let Some(live) = note.live_id {
        st.with(|s| s.popup_over(live))?;
        if note.transient {
            close_notes(st, &[id], "expired").await?;
        }
    } else {
        // Omarchy mode has no pop-ups of ours; nothing to do.
    }
    changed(st).await;
    Ok(())
}

/// Tell the app one of its buttons was clicked (while the notification is open).
pub async fn invoke(st: &State, note: &Note, key: &str) -> bool {
    let (Some(live), true, true) = (note.live_id, note.open(), st.nyx_mode) else { return false };
    if !note.app_actions.iter().any(|a| a.key == key) {
        return false;
    }
    // A token lets the app raise its window under Wayland focus rules.
    let token = format!("nyx-{live}-{}", (now() * 1000.0) as i64);
    emit(st, fdo::PATH, fdo::NAME, "ActivationToken", &(live, token.as_str())).await;
    emit(st, fdo::PATH, fdo::NAME, "ActionInvoked", &(live, key)).await;
    true
}

/// A click on a notification: the app's default action while it's open, else Omarchy's
/// click command, else open the app. Then it's dealt with.
pub async fn activate(st: &State, id: i64) -> zfdo::Result<bool> {
    let Some(note) = st.with(|s| s.get(id))? else { return Ok(false) };
    let ran = invoke(st, &note, "default").await
        || note.exec_argv.as_ref().is_some_and(|a| launch::spawn(a))
        || launch::open_app(&note.desktop_entry)
        || launch::open_app(&note.app_icon);
    if ran {
        close_notes(st, &[id], "activated").await?;
        st.with(|s| s.dismiss(&[id]))?;
        changed(st).await;
    }
    Ok(ran)
}

/// One of the app's own buttons.
pub async fn invoke_action(st: &State, id: i64, key: &str) -> zfdo::Result<bool> {
    let Some(note) = st.with(|s| s.get(id))? else { return Ok(false) };
    let ok = invoke(st, &note, key).await;
    if ok {
        close_notes(st, &[id], "activated").await?;
        st.with(|s| s.dismiss(&[id]))?;
        changed(st).await;
    }
    Ok(ok)
}

/// One of Omarchy's command buttons (`omarchy-actions`), by position.
pub async fn run_action(st: &State, id: i64, index: u32) -> zfdo::Result<bool> {
    let Some(note) = st.with(|s| s.get(id))? else { return Ok(false) };
    let Some(a) = note.actions.get(index as usize) else { return Ok(false) };
    let ok = launch::spawn(&a.argv);
    if ok {
        close_notes(st, &[id], "activated").await?;
        st.with(|s| s.dismiss(&[id]))?;
        changed(st).await;
    }
    Ok(ok)
}
