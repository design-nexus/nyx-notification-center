//! Do not disturb lives in Omarchy's `quiet.json` either way, so the bar, Omarchy's
//! menu and switching back to Omarchy's notifications all see the same setting. In Nyx
//! mode this service writes it (keeping Omarchy's other fields); in Omarchy mode it asks
//! Omarchy's shell.

use std::path::Path;

pub fn get(dir: &Path) -> bool {
    nyx_notify_store::omarchy::dnd(dir)
}

/// Write `dnd` into quiet.json, leaving the rest of it as it was.
pub fn write(dir: &Path, on: bool) -> std::io::Result<()> {
    let path = dir.join("quiet.json");
    let mut v: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({ "snoozes": {}, "codesBypassQuiet": true }));
    v["dnd"] = on.into();
    v["silencedSince"] = if on { (nyx_notify_store::now() as i64).into() } else { 0.into() };
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(".quiet.json.nyx");
    std::fs::write(&tmp, serde_json::to_string(&v).unwrap_or_default())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(tmp, path)
}

/// Switch it through Omarchy's shell (Omarchy mode).
pub fn ask_omarchy(on: bool) -> bool {
    let ok = std::process::Command::new("omarchy-shell")
        .args(["-q", "notifications", "setDnd", if on { "on" } else { "off" }])
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        let _ = std::process::Command::new("omarchy-shell").args(["-q", "omarchy.indicators", "refresh"]).status();
    }
    ok
}

#[cfg(test)]
mod tests {
    #[test]
    fn keeps_omarchys_fields() {
        let dir = std::env::temp_dir().join(format!("nyx-quiet-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("quiet.json"),
            r#"{"snoozes":{"Mail":5},"dnd":false,"silencedSince":0,"codesBypassQuiet":false}"#,
        )
        .unwrap();
        super::write(&dir, true).unwrap();
        assert!(super::get(&dir));
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("quiet.json")).unwrap()).unwrap();
        assert_eq!(v["snoozes"]["Mail"], 5);
        assert_eq!(v["codesBypassQuiet"], false);
        assert!(v["silencedSince"].as_i64().unwrap() > 0);
        super::write(&dir, false).unwrap();
        assert!(!super::get(&dir));
        let _ = std::fs::remove_dir_all(dir);
    }
}
