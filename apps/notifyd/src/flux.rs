//! Phone notifications relayed by Flux's fluxd (Nyx Connect): which device each came
//! from, and what the phone calls it, so a notification sent again (fluxd restarted, the
//! phone reconnected) updates its row instead of adding one.
//!
//! fluxd names them `App · Device` and puts the device's ID in its button keys
//! (`notif-action:<device>:<id>:<action>`, `notif-dismiss:<device>:<id>`); a
//! notification without buttons is matched to a paired device by name.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Whether the program behind a bus name is fluxd (its process's executable; on NixOS
/// the wrapped `.fluxd-wrapped`).
pub async fn is_fluxd(conn: &zbus::Connection, sender: &str) -> bool {
    static KNOWN: Mutex<Option<HashMap<String, bool>>> = Mutex::new(None);
    if let Some(&known) = KNOWN.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_default().get(sender) {
        return known;
    }
    let Ok(name) = zbus::names::BusName::try_from(sender) else { return false };
    let Ok(dbus) = zbus::fdo::DBusProxy::new(conn).await else { return false };
    let Ok(pid) = dbus.get_connection_unix_process_id(name).await else { return false };
    let flux = std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .is_some_and(|n| program_is_fluxd(&n));
    KNOWN.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_default().insert(sender.to_string(), flux);
    flux
}

pub fn program_is_fluxd(file_name: &str) -> bool {
    file_name.trim_start_matches('.').trim_end_matches("-wrapped") == "fluxd"
}

/// The device and the source key of a notification fluxd sent. `by_name` finds a
/// paired device's ID from its name.
pub fn source(app_name: &str, actions: &[String], by_name: impl FnOnce(&str) -> Option<String>) -> (String, String) {
    for key in actions.iter().step_by(2) {
        for prefix in ["notif-action:", "notif-dismiss:"] {
            if let Some(rest) = key.strip_prefix(prefix) {
                let mut parts = rest.split(':');
                if let (Some(device), Some(id)) = (parts.next(), parts.next())
                    && valid_id(device)
                    && !id.is_empty()
                {
                    return (device.to_string(), format!("flux:{device}:{id}"));
                }
            }
        }
        for prefix in ["pair-accept:", "pair-reject:"] {
            if let Some(device) = key.strip_prefix(prefix).filter(|d| valid_id(d)) {
                return (device.to_string(), format!("flux-pair:{device}"));
            }
        }
    }
    // `App · Device` for a phone's own notification, the device's name for a call.
    let name = app_name.rsplit(" · ").next().unwrap_or(app_name).trim();
    if name.is_empty() || name == "Flux" {
        return (String::new(), String::new());
    }
    (by_name(name).unwrap_or_default(), String::new())
}

fn valid_id(id: &str) -> bool {
    (16..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Paired devices' names and IDs, and when fluxd was asked.
type Named = Option<(Instant, Vec<(String, String)>)>;

/// A paired device's ID by its name, from fluxd (asked at most every 30 seconds).
pub fn device_named(name: &str) -> Option<String> {
    static CACHE: Mutex<Named> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if cache.as_ref().is_none_or(|(at, _)| at.elapsed() > Duration::from_secs(30)) {
        let devices = nyx_connect::Client::new()
            .timeout(Duration::from_millis(500))
            .state()
            .map(|s| s.devices.into_iter().filter(|d| d.paired).map(|d| (d.name, d.id)).collect())
            .unwrap_or_default();
        *cache = Some((Instant::now(), devices));
    }
    let devices = &cache.as_ref()?.1;
    let mut named = devices.iter().filter(|(n, _)| n.eq_ignore_ascii_case(name));
    match (named.next(), named.next()) {
        (Some((_, id)), None) => Some(id.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHONE: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90";

    #[test]
    fn the_device_comes_from_fluxds_button_keys() {
        let actions = vec![
            format!("notif-action:{PHONE}:MTIz:UmVwbHk="),
            "Reply".into(),
            format!("notif-dismiss:{PHONE}:MTIz"),
            "Dismiss on phone".into(),
        ];
        let (device, key) = source("WhatsApp · Pandora", &actions, |_| panic!("not needed"));
        assert_eq!(device, PHONE);
        assert_eq!(key, format!("flux:{PHONE}:MTIz"));
        let (device, key) = source("Flux", &[format!("pair-accept:{PHONE}"), "Accept".into()], |_| None);
        assert_eq!((device.as_str(), key), (PHONE, format!("flux-pair:{PHONE}")));
    }

    #[test]
    fn without_buttons_the_name_is_looked_up() {
        let (device, key) = source("Spotify · Pandora", &[], |n| (n == "Pandora").then(|| PHONE.to_string()));
        assert_eq!((device.as_str(), key.as_str()), (PHONE, ""));
        assert_eq!(source("Flux", &[], |_| panic!("fluxd's own")), (String::new(), String::new()));
        let bad = vec!["notif-action:../x:1:2".to_string(), "Go".into()];
        assert_eq!(source("Pandora", &bad, |_| None).0, "");
    }

    #[test]
    fn fluxd_is_known_by_its_program() {
        assert!(program_is_fluxd("fluxd"));
        assert!(program_is_fluxd(".fluxd-wrapped"));
        assert!(!program_is_fluxd("flux-gui"));
    }
}
