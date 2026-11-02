//! Nyx notification history: every notification kept in SQLite, with read, dismissed and
//! pinned modelled separately.
//!
//! - **Inbox**: not dismissed. A pop-up that timed out stays; one you clicked, closed, or
//!   that its app withdrew goes straight to history, and so do Omarchy's own passing
//!   messages.
//! - **Pinned**: kept, with its text, until unpinned or deleted. Clear All, dismissing,
//!   retention and history deletion never touch it.
//! - **History**: everything, searchable, kept for the retention period (7, 30 or 90
//!   days). Apps can be left out of history entirely.
//!
//! Stage A fills it from Omarchy's notification files ([`omarchy`]); a deleted row
//! leaves a short-lived marker so the next read of those files doesn't bring it back.

pub mod omarchy;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub type Result<T> = rusqlite::Result<T>;

/// Seconds since the epoch, as Omarchy writes them.
pub fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub label: String,
    pub argv: Vec<String>,
}

/// A notification as the panel shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: i64,
    pub key: String,
    pub app: String,
    pub app_icon: String,
    pub glyph: String,
    pub summary: String,
    pub body: String,
    pub urgency: i64,
    /// What clicking it runs, if the sender said.
    pub exec_argv: Option<Vec<String>>,
    pub actions: Vec<Action>,
    pub ts: f64,
    pub closed_ts: Option<f64>,
    pub closed_reason: Option<String>,
    /// Its pop-up is on screen now.
    pub live: bool,
    pub read: bool,
    pub dismissed: bool,
    pub pinned: bool,
}

/// Do not disturb by the clock: quiet from `from` to `to`, minutes after midnight (it
/// may run past midnight).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Schedule {
    pub enabled: bool,
    pub from: u16,
    pub to: u16,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule { enabled: false, from: 22 * 60, to: 7 * 60 }
    }
}

impl Schedule {
    /// Whether `minute` (after midnight) falls in the quiet hours.
    pub fn quiet_at(&self, minute: u16) -> bool {
        if !self.enabled || self.from == self.to {
            return false;
        }
        if self.from < self.to { (self.from..self.to).contains(&minute) } else { minute >= self.from || minute < self.to }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 7, 30 or 90.
    pub retention_days: u32,
    pub schedule: Schedule,
    /// Apps whose notifications aren't kept (pinned ones excepted).
    pub excluded: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { retention_days: 30, schedule: Schedule::default(), excluded: Vec::new() }
    }
}

impl Settings {
    fn tidy(mut self) -> Settings {
        self.retention_days = match self.retention_days {
            0..=7 => 7,
            8..=30 => 30,
            _ => 90,
        };
        self.schedule.from %= 24 * 60;
        self.schedule.to %= 24 * 60;
        self.excluded.sort();
        self.excluded.dedup();
        self
    }
}

/// An app that has sent something, for the history filter and the per-app switches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct App {
    pub app: String,
    pub app_icon: String,
    pub count: i64,
    pub excluded: bool,
}

pub struct Store {
    db: Connection,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS notifications (
  id INTEGER PRIMARY KEY,
  key TEXT NOT NULL UNIQUE,
  app TEXT NOT NULL DEFAULT '',
  app_icon TEXT NOT NULL DEFAULT '',
  glyph TEXT NOT NULL DEFAULT '',
  summary TEXT NOT NULL DEFAULT '',
  body TEXT NOT NULL DEFAULT '',
  group_key TEXT NOT NULL DEFAULT '',
  urgency INTEGER NOT NULL DEFAULT 1,
  exec_argv TEXT NOT NULL DEFAULT '',
  exec_actions TEXT NOT NULL DEFAULT '',
  ts REAL NOT NULL,
  closed_ts REAL,
  closed_reason TEXT,
  live INTEGER NOT NULL DEFAULT 0,
  read INTEGER NOT NULL DEFAULT 0,
  dismissed INTEGER NOT NULL DEFAULT 0,
  pinned INTEGER NOT NULL DEFAULT 0,
  pinned_ts REAL
);
CREATE INDEX IF NOT EXISTS notifications_ts ON notifications(ts);
CREATE TABLE IF NOT EXISTS gone (key TEXT PRIMARY KEY, ts REAL NOT NULL);
CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK (id = 1), json TEXT NOT NULL);
";

const COLUMNS: &str = "id, key, app, app_icon, glyph, summary, body, urgency, exec_argv, exec_actions, ts, closed_ts, \
                       closed_reason, live, read, dismissed, pinned";

fn note(r: &rusqlite::Row) -> Result<Note> {
    let argv: String = r.get(8)?;
    let actions: String = r.get(9)?;
    Ok(Note {
        id: r.get(0)?,
        key: r.get(1)?,
        app: r.get(2)?,
        app_icon: r.get(3)?,
        glyph: r.get(4)?,
        summary: r.get(5)?,
        body: r.get(6)?,
        urgency: r.get(7)?,
        exec_argv: serde_json::from_str::<Vec<String>>(&argv).ok().filter(|a| !a.is_empty()),
        actions: serde_json::from_str(&actions).unwrap_or_default(),
        ts: r.get(10)?,
        closed_ts: r.get(11)?,
        closed_reason: r.get(12)?,
        live: r.get(13)?,
        read: r.get(14)?,
        dismissed: r.get(15)?,
        pinned: r.get(16)?,
    })
}

/// A closing that means it was dealt with: clicked, closed by you, or withdrawn by its app.
fn handled(reason: Option<&str>) -> bool {
    matches!(reason, Some("dismissed" | "activated" | "closed"))
}

/// What a search looks for.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Query {
    /// Words found in the title, the text or the app's name (all of them).
    pub text: String,
    /// Only this app.
    pub app: String,
    /// Only the last this many days (0: all).
    pub days: u32,
    pub limit: u32,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let db = Connection::open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Store::init(db)
    }

    pub fn memory() -> Result<Store> {
        Store::init(Connection::open_in_memory()?)
    }

    fn init(db: Connection) -> Result<Store> {
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.busy_timeout(std::time::Duration::from_secs(2))?;
        db.execute_batch(SCHEMA)?;
        Ok(Store { db })
    }

    /// Where it lives: `~/.local/share/nyx/notifications.db`.
    pub fn default_path() -> std::path::PathBuf {
        let data = std::env::var_os("XDG_DATA_HOME").map(std::path::PathBuf::from).unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
        data.join("nyx/notifications.db")
    }

    pub fn settings(&self) -> Settings {
        self.db
            .query_row("SELECT json FROM settings WHERE id = 1", [], |r| r.get::<_, String>(0))
            .ok()
            .and_then(|j| serde_json::from_str::<Settings>(&j).ok())
            .unwrap_or_default()
            .tidy()
    }

    /// Save settings. Apps newly left out lose their (unpinned) history, and a shorter
    /// retention applies straight away.
    pub fn set_settings(&mut self, settings: Settings) -> Result<Settings> {
        let settings = settings.tidy();
        let json = serde_json::to_string(&settings).expect("settings serialize");
        let tx = self.db.transaction()?;
        tx.execute("INSERT INTO settings (id, json) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET json = ?1", [json])?;
        for app in &settings.excluded {
            tx.execute("DELETE FROM notifications WHERE app = ?1 AND pinned = 0", [app])?;
        }
        tx.commit()?;
        self.prune(now())?;
        Ok(settings)
    }

    /// Bring in what Omarchy's files say now. `live` is every pop-up on screen; `history`
    /// the closed ones. Returns whether anything changed.
    pub fn ingest(&mut self, live: &[omarchy::Row], history: &[omarchy::Row], now: f64) -> Result<bool> {
        let excluded = self.settings().excluded;
        let tx = self.db.transaction()?;
        let mut changed = false;
        let gone = |key: &str| -> Result<bool> {
            tx.query_row("SELECT 1 FROM gone WHERE key = ?1", [key], |_| Ok(())).optional().map(|o| o.is_some())
        };
        // Pop-ups on screen.
        let mut on_screen = Vec::new();
        for row in live {
            if excluded.contains(&row.app) || gone(&row.key)? {
                continue;
            }
            on_screen.push(row.key.clone());
            let existing: Option<(String, String)> = tx
                .query_row("SELECT summary, body FROM notifications WHERE key = ?1", [&row.key], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .optional()?;
            match existing {
                None => {
                    insert(&tx, row, true)?;
                    changed = true;
                }
                // Replaced in place (same notification, new text): unread again.
                Some((s, b)) if s != row.summary || b != row.body => {
                    tx.execute(
                        "UPDATE notifications SET summary = ?2, body = ?3, app_icon = ?4, glyph = ?5, urgency = ?6, \
                         exec_argv = ?7, exec_actions = ?8, ts = ?9, live = 1, read = 0 WHERE key = ?1",
                        params![
                            row.key,
                            row.summary,
                            row.body,
                            row.app_icon,
                            row.glyph,
                            row.urgency,
                            row.exec_argv,
                            row.exec_actions,
                            row.ts
                        ],
                    )?;
                    changed = true;
                }
                Some(_) => {
                    changed |= tx.execute("UPDATE notifications SET live = 1 WHERE key = ?1 AND live = 0", [&row.key])? > 0;
                }
            }
        }
        // Closed ones: record how, once.
        for row in history {
            if excluded.contains(&row.app) || gone(&row.key)? {
                continue;
            }
            let known: Option<Option<String>> = tx
                .query_row("SELECT closed_reason FROM notifications WHERE key = ?1", [&row.key], |r| r.get(0))
                .optional()?;
            match known {
                None => {
                    insert(&tx, row, false)?;
                    changed = true;
                }
                Some(None) => {
                    let reason = row.closed_reason.clone().unwrap_or_else(|| "closed".into());
                    tx.execute(
                        "UPDATE notifications SET closed_ts = ?2, closed_reason = ?3, live = 0, \
                         dismissed = MAX(dismissed, ?4) WHERE key = ?1",
                        params![row.key, row.closed_ts.unwrap_or(now), reason, handled(Some(&reason))],
                    )?;
                    changed = true;
                }
                Some(Some(_)) => {}
            }
        }
        // Gone from the screen without a history row (yet): no longer live.
        let mut stmt = tx.prepare("SELECT key FROM notifications WHERE live = 1")?;
        let was_live: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<Result<_>>()?;
        drop(stmt);
        for key in was_live.iter().filter(|k| !on_screen.contains(k)) {
            tx.execute("UPDATE notifications SET live = 0 WHERE key = ?1", [key])?;
            changed = true;
        }
        tx.commit()?;
        Ok(changed)
    }

    fn select(&self, filter: &str, order: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<Note>> {
        let sql = format!("SELECT {COLUMNS} FROM notifications WHERE {filter} ORDER BY {order}");
        let mut stmt = self.db.prepare(&sql)?;
        stmt.query_map(args, note)?.collect()
    }

    /// Not dismissed, newest first.
    pub fn inbox(&self) -> Result<Vec<Note>> {
        self.select("dismissed = 0", "ts DESC", &[])
    }

    /// Pinned, most recently pinned first.
    pub fn pinned(&self) -> Result<Vec<Note>> {
        self.select("pinned = 1", "pinned_ts DESC, ts DESC", &[])
    }

    /// History, newest first.
    pub fn history(&self, q: &Query, now: f64) -> Result<Vec<Note>> {
        let mut filter = String::from("1");
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        for word in q.text.split_whitespace().take(8) {
            let like = format!("%{}%", word.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
            filter.push_str(&format!(
                " AND (summary LIKE ?{n} ESCAPE '\\' OR body LIKE ?{n} ESCAPE '\\' OR app LIKE ?{n} ESCAPE '\\')",
                n = args.len() + 1
            ));
            args.push(Box::new(like));
        }
        if !q.app.is_empty() {
            filter.push_str(&format!(" AND app = ?{}", args.len() + 1));
            args.push(Box::new(q.app.clone()));
        }
        if q.days > 0 {
            filter.push_str(&format!(" AND ts >= ?{}", args.len() + 1));
            args.push(Box::new(now - f64::from(q.days) * 86_400.0));
        }
        let limit = if q.limit == 0 { 500 } else { q.limit.min(2000) };
        let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
        self.select(&filter, &format!("ts DESC LIMIT {limit}"), &refs)
    }

    /// Every app that has something stored, and every left-out one.
    pub fn apps(&self) -> Result<Vec<App>> {
        let excluded = self.settings().excluded;
        let mut stmt = self.db.prepare(
            "SELECT app, (SELECT app_icon FROM notifications n2 WHERE n2.app = n.app ORDER BY ts DESC LIMIT 1), COUNT(*) \
             FROM notifications n GROUP BY app ORDER BY app COLLATE NOCASE",
        )?;
        let mut apps: Vec<App> = stmt
            .query_map([], |r| {
                Ok(App { app: r.get(0)?, app_icon: r.get::<_, Option<String>>(1)?.unwrap_or_default(), count: r.get(2)?, excluded: false })
            })?
            .collect::<Result<_>>()?;
        for a in &mut apps {
            a.excluded = excluded.contains(&a.app);
        }
        for e in excluded {
            if !apps.iter().any(|a| a.app == e) {
                apps.push(App { app: e, app_icon: String::new(), count: 0, excluded: true });
            }
        }
        Ok(apps)
    }

    pub fn set_pinned(&mut self, id: i64, pinned: bool) -> Result<bool> {
        let n = if pinned {
            self.db.execute("UPDATE notifications SET pinned = 1, pinned_ts = ?2 WHERE id = ?1 AND pinned = 0", params![id, now()])?
        } else {
            self.db.execute("UPDATE notifications SET pinned = 0, pinned_ts = NULL WHERE id = ?1 AND pinned = 1", [id])?
        };
        Ok(n > 0)
    }

    pub fn dismiss(&mut self, ids: &[i64]) -> Result<bool> {
        self.each(ids, "UPDATE notifications SET dismissed = 1, read = 1 WHERE id = ?1 AND dismissed = 0")
    }

    /// Clear the Inbox (pins stay pinned).
    pub fn dismiss_all(&mut self) -> Result<bool> {
        Ok(self.db.execute("UPDATE notifications SET dismissed = 1, read = 1 WHERE dismissed = 0", [])? > 0)
    }

    pub fn mark_read(&mut self, ids: &[i64]) -> Result<bool> {
        self.each(ids, "UPDATE notifications SET read = 1 WHERE id = ?1 AND read = 0")
    }

    /// Everything in the Inbox, read.
    pub fn mark_all_read(&mut self) -> Result<bool> {
        Ok(self.db.execute("UPDATE notifications SET read = 1 WHERE read = 0 AND dismissed = 0", [])? > 0)
    }

    /// Delete for good, pinned or not.
    pub fn delete(&mut self, ids: &[i64]) -> Result<bool> {
        let tx = self.db.transaction()?;
        let mut changed = false;
        let t = now();
        for id in ids {
            if let Some(key) =
                tx.query_row("SELECT key FROM notifications WHERE id = ?1", [id], |r| r.get::<_, String>(0)).optional()?
            {
                tx.execute("INSERT OR REPLACE INTO gone (key, ts) VALUES (?1, ?2)", params![key, t])?;
                tx.execute("DELETE FROM notifications WHERE id = ?1", [id])?;
                changed = true;
            }
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Delete all history except pinned notifications.
    pub fn delete_history(&mut self) -> Result<bool> {
        let tx = self.db.transaction()?;
        tx.execute("INSERT OR REPLACE INTO gone (key, ts) SELECT key, ?1 FROM notifications WHERE pinned = 0", [now()])?;
        let n = tx.execute("DELETE FROM notifications WHERE pinned = 0", [])?;
        tx.commit()?;
        Ok(n > 0)
    }

    /// Drop what's past the retention period (never pinned ones), and deletion markers
    /// older than Omarchy's own history.
    pub fn prune(&mut self, now: f64) -> Result<bool> {
        let days = self.settings().retention_days;
        let n = self
            .db
            .execute("DELETE FROM notifications WHERE pinned = 0 AND ts < ?1", [now - f64::from(days) * 86_400.0])?;
        self.db.execute("DELETE FROM gone WHERE ts < ?1", [now - 3.0 * 86_400.0])?;
        Ok(n > 0)
    }

    pub fn get(&self, id: i64) -> Result<Option<Note>> {
        Ok(self.select("id = ?1", "id", &[&id])?.into_iter().next())
    }

    fn each(&mut self, ids: &[i64], sql: &str) -> Result<bool> {
        let tx = self.db.transaction()?;
        let mut changed = false;
        for id in ids {
            changed |= tx.execute(sql, [id])? > 0;
        }
        tx.commit()?;
        Ok(changed)
    }
}

fn insert(tx: &rusqlite::Transaction, row: &omarchy::Row, live: bool) -> Result<()> {
    let dismissed = row.passing() || (!live && handled(row.closed_reason.as_deref()));
    let reason = if live { None } else { Some(row.closed_reason.clone().unwrap_or_else(|| "closed".into())) };
    tx.execute(
        "INSERT INTO notifications (key, app, app_icon, glyph, summary, body, group_key, urgency, exec_argv, exec_actions, \
         ts, closed_ts, closed_reason, live, read, dismissed) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)",
        params![
            row.key,
            row.app,
            row.app_icon,
            row.glyph,
            row.summary,
            row.body,
            row.group_key,
            row.urgency,
            row.exec_argv,
            row.exec_actions,
            row.ts,
            if live { None } else { row.closed_ts },
            reason,
            live,
            dismissed,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use omarchy::Row;

    fn row(key: &str, app: &str, ts: f64, closed: Option<&str>) -> Row {
        Row {
            key: key.into(),
            app: app.into(),
            summary: format!("{key} title"),
            body: format!("{key} text"),
            urgency: 1,
            ts,
            closed_ts: closed.map(|_| ts + 5.0),
            closed_reason: closed.map(String::from),
            ..Default::default()
        }
    }

    fn keys(notes: &[Note]) -> Vec<&str> {
        notes.iter().map(|n| n.key.as_str()).collect()
    }

    #[test]
    fn live_then_closed() {
        let mut s = Store::memory().unwrap();
        let t = now();
        assert!(s.ingest(&[row("a", "Chat", t, None)], &[], t).unwrap());
        let inbox = s.inbox().unwrap();
        assert!(inbox[0].live && !inbox[0].read && inbox[0].closed_reason.is_none());
        assert!(!s.ingest(&[row("a", "Chat", t, None)], &[], t).unwrap(), "nothing new");
        // Timed out: off screen, still in the Inbox.
        assert!(s.ingest(&[], &[row("a", "Chat", t, Some("expired"))], t).unwrap());
        let n = &s.inbox().unwrap()[0];
        assert!(!n.live && n.closed_reason.as_deref() == Some("expired"));
        // Seen again in history: recorded once.
        assert!(!s.ingest(&[], &[row("a", "Chat", t, Some("expired"))], t).unwrap());
    }

    #[test]
    fn handled_ones_and_passing_messages_skip_the_inbox() {
        let mut s = Store::memory().unwrap();
        let t = now();
        s.ingest(
            &[row("toast", "omarchy-action", t, None)],
            &[
                row("clicked", "Mail", t - 9.0, Some("activated")),
                row("closed", "Mail", t - 8.0, Some("dismissed")),
                row("timed", "Mail", t - 7.0, Some("expired")),
            ],
            t,
        )
        .unwrap();
        assert_eq!(keys(&s.inbox().unwrap()), vec!["timed"]);
        assert_eq!(s.history(&Query::default(), t).unwrap().len(), 4, "all of them are in history");
        // A live one closed by hand after the fact.
        s.ingest(&[row("x", "Chat", t, None)], &[], t).unwrap();
        s.ingest(&[], &[row("x", "Chat", t, Some("dismissed"))], t).unwrap();
        assert_eq!(keys(&s.inbox().unwrap()), vec!["timed"]);
    }

    #[test]
    fn replaced_text_is_unread_again() {
        let mut s = Store::memory().unwrap();
        let t = now();
        s.ingest(&[row("a", "Transfer", t, None)], &[], t).unwrap();
        s.mark_all_read().unwrap();
        let mut r = row("a", "Transfer", t + 1.0, None);
        r.body = "Done".into();
        assert!(s.ingest(&[r], &[], t).unwrap());
        let n = &s.inbox().unwrap()[0];
        assert_eq!(n.body, "Done");
        assert!(!n.read);
        assert_eq!(s.history(&Query::default(), t).unwrap().len(), 1, "replaced in place");
    }

    #[test]
    fn pins_survive_clear_all_retention_and_history_deletion() {
        let mut s = Store::memory().unwrap();
        let t = now();
        let old = t - 40.0 * 86_400.0;
        s.ingest(&[], &[row("old", "Mail", old, Some("expired")), row("keep", "Mail", old, Some("expired"))], t).unwrap();
        let keep = s.history(&Query { text: "keep".into(), ..Default::default() }, t).unwrap()[0].id;
        assert!(s.set_pinned(keep, true).unwrap());
        s.dismiss_all().unwrap();
        assert!(s.inbox().unwrap().is_empty());
        assert_eq!(keys(&s.pinned().unwrap()), vec!["keep"], "Clear All leaves pins");
        s.prune(t).unwrap();
        assert_eq!(keys(&s.history(&Query::default(), t).unwrap()), vec!["keep"], "retention spares pins");
        s.ingest(&[], &[row("new", "Chat", t, Some("expired"))], t).unwrap();
        s.delete_history().unwrap();
        assert_eq!(keys(&s.history(&Query::default(), t).unwrap()), vec!["keep"]);
        // Unpinned, it's ordinary again.
        s.set_pinned(keep, false).unwrap();
        s.prune(t).unwrap();
        assert!(s.history(&Query::default(), t).unwrap().is_empty());
    }

    #[test]
    fn deleted_rows_stay_deleted() {
        let mut s = Store::memory().unwrap();
        let t = now();
        let h = [row("a", "Mail", t, Some("expired"))];
        s.ingest(&[], &h, t).unwrap();
        let id = s.inbox().unwrap()[0].id;
        s.delete(&[id]).unwrap();
        assert!(!s.ingest(&[], &h, t).unwrap(), "Omarchy's copy doesn't bring it back");
        assert!(s.history(&Query::default(), t).unwrap().is_empty());
    }

    #[test]
    fn search_and_filters() {
        let mut s = Store::memory().unwrap();
        let t = now();
        let mut a = row("a", "Chat", t, Some("expired"));
        a.body = "Bring the hinge boards".into();
        let mut b = row("b", "Mail", t - 10.0 * 86_400.0, Some("expired"));
        b.summary = "Quote for hinge tooling".into();
        let mut c = row("c", "Mail", t, Some("expired"));
        c.summary = "100% off_sale".into();
        s.ingest(&[], &[a, b, c], t).unwrap();
        let q = |text: &str, app: &str, days| Query { text: text.into(), app: app.into(), days, limit: 0 };
        assert_eq!(keys(&s.history(&q("HINGE", "", 0), t).unwrap()), vec!["a", "b"], "case-insensitive, newest first");
        assert_eq!(keys(&s.history(&q("hinge", "Mail", 0), t).unwrap()), vec!["b"]);
        assert_eq!(keys(&s.history(&q("hinge", "", 7), t).unwrap()), vec!["a"]);
        assert_eq!(keys(&s.history(&q("hinge boards", "", 0), t).unwrap()), vec!["a"], "every word");
        assert_eq!(keys(&s.history(&q("100%", "", 0), t).unwrap()), vec!["c"], "% is literal");
        assert_eq!(keys(&s.history(&q("o_f", "", 0), t).unwrap()), Vec::<&str>::new(), "_ is literal");
        assert_eq!(keys(&s.history(&q("chat", "", 0), t).unwrap()), vec!["a"], "matches the app");
    }

    #[test]
    fn left_out_apps() {
        let mut s = Store::memory().unwrap();
        let t = now();
        s.ingest(&[], &[row("p", "Passwords", t, Some("expired")), row("m", "Mail", t, Some("expired"))], t).unwrap();
        let p = s.inbox().unwrap().into_iter().find(|n| n.app == "Passwords").unwrap().id;
        s.ingest(&[], &[row("p2", "Passwords", t, Some("expired"))], t).unwrap();
        s.set_pinned(p, true).unwrap();
        let mut set = s.settings();
        set.excluded.push("Passwords".into());
        s.set_settings(set).unwrap();
        assert_eq!(keys(&s.history(&Query::default(), t).unwrap()), vec!["m", "p"], "only the pinned one is kept");
        s.ingest(&[row("p3", "Passwords", t, None)], &[row("p4", "Passwords", t, Some("expired"))], t).unwrap();
        assert_eq!(s.history(&Query::default(), t).unwrap().len(), 2, "new ones aren't stored");
        let apps = s.apps().unwrap();
        assert!(apps.iter().any(|a| a.app == "Passwords" && a.excluded));
        assert!(apps.iter().any(|a| a.app == "Mail" && !a.excluded && a.count == 1));
    }

    #[test]
    fn settings_round_trip_and_tidy() {
        let mut s = Store::memory().unwrap();
        assert_eq!(s.settings().retention_days, 30);
        let saved = s
            .set_settings(Settings {
                retention_days: 12,
                schedule: Schedule { enabled: true, from: 1500, to: 420 },
                excluded: vec!["B".into(), "A".into(), "B".into()],
            })
            .unwrap();
        assert_eq!(saved.retention_days, 30);
        assert_eq!(saved.schedule.from, 60);
        assert_eq!(saved.excluded, vec!["A", "B"]);
        assert_eq!(s.settings(), saved);
    }

    #[test]
    fn schedule_hours() {
        let night = Schedule { enabled: true, from: 22 * 60, to: 7 * 60 };
        assert!(night.quiet_at(23 * 60) && night.quiet_at(0) && night.quiet_at(6 * 60 + 59));
        assert!(!night.quiet_at(7 * 60) && !night.quiet_at(12 * 60));
        let day = Schedule { enabled: true, from: 9 * 60, to: 17 * 60 };
        assert!(day.quiet_at(9 * 60) && !day.quiet_at(17 * 60) && !day.quiet_at(8 * 60));
        assert!(!Schedule { enabled: false, ..day }.quiet_at(10 * 60));
    }

    #[test]
    fn click_command_and_buttons() {
        let mut s = Store::memory().unwrap();
        let t = now();
        let mut r = row("a", "System", t, Some("expired"));
        r.exec_argv = r#"["omarchy-update"]"#.into();
        r.exec_actions = r#"[{"label":"Later","argv":["true"]}]"#.into();
        s.ingest(&[], &[r], t).unwrap();
        let n = &s.inbox().unwrap()[0];
        assert_eq!(n.exec_argv.as_deref(), Some(&["omarchy-update".to_string()][..]));
        assert_eq!(n.actions, vec![Action { label: "Later".into(), argv: vec!["true".into()] }]);
    }
}
