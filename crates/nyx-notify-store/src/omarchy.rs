//! Omarchy's notification files (`~/.local/state/omarchy/notifications/`): `live.json`
//! holds the pop-ups on screen, `history.json` the closed ones (newest first, at most
//! 100, for 24 hours). Both are arrays of rows written by the shell's notification
//! service; anything unreadable is skipped, never trusted.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// One stored row, as the shell writes it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Row {
    pub key: String,
    pub app: String,
    pub app_icon: String,
    pub glyph: String,
    pub summary: String,
    pub body: String,
    pub group_key: String,
    pub urgency: i64,
    /// The click command, a JSON array of strings (the `omarchy-exec-argv` hint).
    pub exec_argv: String,
    /// Buttons that run commands, a JSON array of `{label, argv}` (`omarchy-actions`).
    pub exec_actions: String,
    pub ts: f64,
    #[serde(rename = "closed_ts")]
    pub closed_ts: Option<f64>,
    #[serde(rename = "closed_reason")]
    pub closed_reason: Option<String>,
}

impl Row {
    fn valid(&self) -> bool {
        !self.key.is_empty()
            && self.key.len() <= 120
            && self.key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            && self.ts.is_finite()
            && self.ts > 0.0
    }

    /// Omarchy's own feedback ("Layout: Scrolling", a plugin was cloned) and unnamed
    /// `notify-send` messages: the shell treats them as passing, so they skip the Inbox.
    pub fn passing(&self) -> bool {
        self.app == "omarchy-action" || self.app == "notify-send"
    }
}

/// Parse one of the files. A file that isn't an array gives nothing; rows without a
/// usable key or time are dropped.
pub fn parse(text: &str) -> Vec<Row> {
    let Ok(values) = serde_json::from_str::<Vec<serde_json::Value>>(text) else { return Vec::new() };
    values.into_iter().filter_map(|v| serde_json::from_value::<Row>(v).ok()).filter(Row::valid).collect()
}

/// Where Omarchy keeps them.
pub fn dir() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
    state.join("omarchy/notifications")
}

/// Read both files from `dir`: (live, history).
pub fn read(dir: &Path) -> (Vec<Row>, Vec<Row>) {
    let get = |name: &str| std::fs::read_to_string(dir.join(name)).map(|t| parse(&t)).unwrap_or_default();
    (get("live.json"), get("history.json"))
}

/// Do not disturb, from `quiet.json`.
pub fn dnd(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join("quiet.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["dnd"].as_bool())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_rows_and_skips_junk() {
        let rows = parse(
            r#"[{"key":"nmvyonmwf1","originalId":1,"app":"Calendar","appIcon":"io.github.design_nexus.nyx.Calendar",
                 "summary":"Yule","body":"Earlier · 6:00 PM","groupKey":"app:calendar","urgency":1,"ts":1793589126.063,
                 "closed_ts":1793589134.282,"closed_reason":"expired"},
                {"key":"bad key!","ts":1},
                {"key":"nots"},
                42]"#,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].app_icon, "io.github.design_nexus.nyx.Calendar");
        assert_eq!(rows[0].closed_reason.as_deref(), Some("expired"));
        assert!(parse("{}").is_empty());
        assert!(parse("not json").is_empty());
    }
}
