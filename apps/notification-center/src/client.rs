//! Talking to nyx-notifyd (`io.github.design_nexus.nyx.Notifications1`), and Omarchy's
//! Do not disturb.

use gtk::prelude::*;
use gtk::{gio, glib};
use nyx_notify_store::{App, Note, Query, Settings};
use serde::de::DeserializeOwned;

const BUS: &str = "io.github.design_nexus.nyx.Notifications";
const PATH: &str = "/io/github/design_nexus/nyx/Notifications";
const IFACE: &str = "io.github.design_nexus.nyx.Notifications1";

fn bus() -> Result<gio::DBusConnection, String> {
    gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).map_err(|e| e.to_string())
}

async fn call(method: &str, args: Option<glib::Variant>) -> Result<glib::Variant, String> {
    bus()?.call_future(Some(BUS), PATH, IFACE, method, args.as_ref(), None, gio::DBusCallFlags::NONE, 10_000).await.map_err(|e| {
        if e.matches(gio::DBusError::ServiceUnknown) || e.matches(gio::DBusError::NameHasNoOwner) {
            "The notification service (nyx-notifyd) isn't running".to_string()
        } else {
            e.message().to_string()
        }
    })
}

async fn get<T: DeserializeOwned>(method: &str, args: Option<glib::Variant>) -> Result<T, String> {
    let v = call(method, args).await?;
    let text = v.child_value(0).str().map(str::to_string).ok_or("unexpected reply")?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

pub async fn inbox() -> Result<Vec<Note>, String> {
    get("Inbox", None).await
}

pub async fn pinned() -> Result<Vec<Note>, String> {
    get("Pinned", None).await
}

pub async fn history(q: &Query) -> Result<Vec<Note>, String> {
    get("History", Some((serde_json::to_string(q).unwrap_or_default(),).to_variant())).await
}

pub async fn apps() -> Result<Vec<App>, String> {
    get("Apps", None).await
}

pub async fn settings() -> Result<Settings, String> {
    get("Settings", None).await
}

pub async fn set_settings(s: &Settings) -> Result<Settings, String> {
    get("SetSettings", Some((serde_json::to_string(s).unwrap_or_default(),).to_variant())).await
}

pub async fn set_pinned(id: i64, pinned: bool) -> Result<(), String> {
    call("SetPinned", Some((id, pinned).to_variant())).await.map(|_| ())
}

pub async fn dismiss(ids: Vec<i64>) -> Result<(), String> {
    call("Dismiss", Some((ids,).to_variant())).await.map(|_| ())
}

pub async fn dismiss_all() -> Result<(), String> {
    call("DismissAll", None).await.map(|_| ())
}

pub async fn mark_all_read() -> Result<(), String> {
    call("MarkAllRead", None).await.map(|_| ())
}

pub async fn delete(ids: Vec<i64>) -> Result<(), String> {
    call("Delete", Some((ids,).to_variant())).await.map(|_| ())
}

pub async fn delete_history() -> Result<(), String> {
    call("DeleteHistory", None).await.map(|_| ())
}

/// Call `f` on every `Changed` until the returned subscription is dropped.
pub fn on_changed(f: impl Fn() + 'static) -> Option<gio::SignalSubscription> {
    let bus = bus().ok()?;
    Some(bus.subscribe_to_signal(
        Some(BUS),
        Some(IFACE),
        Some("Changed"),
        Some(PATH),
        None,
        gio::DBusSignalFlags::NONE,
        move |_| f(),
    ))
}

/// Omarchy's notification state folder.
pub fn omarchy_dir() -> std::path::PathBuf {
    nyx_notify_store::omarchy::dir()
}

pub fn dnd() -> bool {
    nyx_notify_store::omarchy::dnd(&omarchy_dir())
}

/// Switch Do not disturb the way Omarchy's keybinding does, so the bar follows.
pub fn toggle_dnd() {
    crate::ui::spawn(&["omarchy-toggle-notification-silencing"]);
}
