//! Talking to nyx-notifyd (`io.github.design_nexus.nyx.Notifications1`), as the GTK
//! panel's `client.rs` does, without GTK. Replies stay JSON: they go on to the
//! interface as they came.

use serde_json::Value;

const BUS: &str = "io.github.design_nexus.nyx.Notifications";
const PATH: &str = "/io/github/design_nexus/nyx/Notifications";
const IFACE: &str = "io.github.design_nexus.nyx.Notifications1";

fn bus() -> Result<gio::DBusConnection, String> {
    gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).map_err(|e| e.to_string())
}

pub async fn call(method: &str, args: Option<glib::Variant>) -> Result<glib::Variant, String> {
    bus()?.call_future(Some(BUS), PATH, IFACE, method, args.as_ref(), None, gio::DBusCallFlags::NONE, 10_000).await.map_err(|e| {
        if e.matches(gio::DBusError::ServiceUnknown) || e.matches(gio::DBusError::NameHasNoOwner) {
            "The notification service (nyx-notifyd) isn't running".to_string()
        } else {
            e.message().to_string()
        }
    })
}

/// A method answering one JSON string.
pub async fn json(method: &str, args: Option<glib::Variant>) -> Result<Value, String> {
    let v = call(method, args).await?;
    let text = v.child_value(0).str().map(str::to_string).ok_or("unexpected reply")?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// A method answering a flag (whether anything happened).
pub async fn flag(method: &str, args: Option<glib::Variant>) -> Result<bool, String> {
    let v = call(method, args).await?;
    Ok(v.child_value(0).get::<bool>().unwrap_or(false))
}

/// "nyx" when Nyx receives notifications and shows the pop-ups, else "omarchy".
pub async fn mode() -> Result<String, String> {
    let v = call("Mode", None).await?;
    v.child_value(0).str().map(str::to_string).ok_or_else(|| "unexpected reply".to_string())
}

/// Call `f` on every `Changed` until the returned subscription is dropped.
pub fn on_changed(f: impl Fn() + 'static) -> Option<gio::SignalSubscription> {
    let bus = bus().ok()?;
    Some(bus.subscribe_to_signal(Some(BUS), Some(IFACE), Some("Changed"), Some(PATH), None, gio::DBusSignalFlags::NONE, move |_| f()))
}

/// `Popup(payload)` and `PopupClosed(id)`, for as long as the subscriptions are kept.
pub fn on_popups(show: impl Fn(String) + 'static, closed: impl Fn(i64) + 'static) -> Vec<gio::SignalSubscription> {
    let Ok(bus) = bus() else { return Vec::new() };
    let a = bus.subscribe_to_signal(Some(BUS), Some(IFACE), Some("Popup"), Some(PATH), None, gio::DBusSignalFlags::NONE, move |s| {
        if let Some(p) = s.parameters.child_value(0).str() {
            show(p.to_string());
        }
    });
    let b =
        bus.subscribe_to_signal(Some(BUS), Some(IFACE), Some("PopupClosed"), Some(PATH), None, gio::DBusSignalFlags::NONE, move |s| {
            if let Some(id) = s.parameters.child_value(0).get::<i64>() {
                closed(id);
            }
        });
    vec![a, b]
}
