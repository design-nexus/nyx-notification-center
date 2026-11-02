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

/// Switch Do not disturb (nyx-notifyd writes it, or asks Omarchy, by mode).
pub fn toggle_dnd() {
    crate::ui::act(async { call("ToggleDnd", None).await.map(|_| ()) });
}

async fn flag(method: &str, args: Option<glib::Variant>) -> Result<bool, String> {
    let v = call(method, args).await?;
    Ok(v.child_value(0).get::<bool>().unwrap_or(false))
}

/// A click: whatever the notification asks for. Whether anything happened.
pub async fn activate(id: i64) -> Result<bool, String> {
    flag("Activate", Some((id,).to_variant())).await
}

/// One of the app's own buttons.
pub async fn invoke_action(id: i64, key: &str) -> Result<bool, String> {
    flag("InvokeAction", Some((id, key).to_variant())).await
}

/// One of Omarchy's command buttons, by position.
pub async fn run_action(id: i64, index: u32) -> Result<bool, String> {
    flag("RunAction", Some((id, index).to_variant())).await
}

/// A pop-up went: `timeout` or `dismissed`.
pub async fn popup_ended(id: i64, reason: &str) -> Result<(), String> {
    call("PopupEnded", Some((id, reason).to_variant())).await.map(|_| ())
}

/// `Popup(payload)` and `PopupClosed(id)` from nyx-notifyd, for as long as the
/// returned subscriptions are kept.
pub fn on_popups(show: impl Fn(String) + 'static, closed: impl Fn(i64) + 'static) -> Vec<gio::SignalSubscription> {
    let Ok(bus) = bus() else { return Vec::new() };
    let a =
        bus.subscribe_to_signal(Some(BUS), Some(IFACE), Some("Popup"), Some(PATH), None, gio::DBusSignalFlags::NONE, move |s| {
            if let Some(p) = s.parameters.child_value(0).str() {
                show(p.to_string());
            }
        });
    let b = bus.subscribe_to_signal(
        Some(BUS),
        Some(IFACE),
        Some("PopupClosed"),
        Some(PATH),
        None,
        gio::DBusSignalFlags::NONE,
        move |s| {
            if let Some(id) = s.parameters.child_value(0).get::<i64>() {
                closed(id);
            }
        },
    );
    vec![a, b]
}
