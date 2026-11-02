//! `io.github.design_nexus.nyx.Notifications1`: the history for the panel (JSON in and
//! out), the pop-ups' side of things, and what Omarchy's shortcuts need.

use crate::state::{self, State, err};
use nyx_notify_store::{Query, Settings, now};
use std::sync::Arc;
use zbus::object_server::SignalEmitter;
use zbus::{fdo, interface};

pub struct Service {
    pub state: Arc<State>,
}

fn json<T: serde::Serialize>(v: &T) -> fdo::Result<String> {
    serde_json::to_string(v).map_err(err)
}

impl Service {
    /// The newest pop-up's row id, if one is up.
    fn newest_id(&self) -> fdo::Result<Option<i64>> {
        let open = self.state.with(|s| s.open_notes())?;
        Ok(open.into_iter().filter(|n| n.live).max_by(|a, b| a.ts.total_cmp(&b.ts)).map(|n| n.id))
    }
}

#[interface(name = "io.github.design_nexus.nyx.Notifications1")]
impl Service {
    /// "nyx" when this service receives notifications itself (stage B), else "omarchy".
    fn mode(&self) -> &'static str {
        if self.state.nyx_mode { "nyx" } else { "omarchy" }
    }

    /// Not dismissed, newest first (JSON list).
    fn inbox(&self) -> fdo::Result<String> {
        json(&self.state.with(|s| s.inbox())?)
    }

    /// Pinned, most recently pinned first.
    fn pinned(&self) -> fdo::Result<String> {
        json(&self.state.with(|s| s.pinned())?)
    }

    /// History matching `query` (JSON `{text, app, days, limit}`; `""` for everything).
    fn history(&self, query: &str) -> fdo::Result<String> {
        let q: Query = if query.trim().is_empty() { Query::default() } else { serde_json::from_str(query).map_err(err)? };
        json(&self.state.with(|s| s.history(&q, now()))?)
    }

    /// Apps with stored notifications, and the ones left out of history.
    fn apps(&self) -> fdo::Result<String> {
        json(&self.state.with(|s| s.apps())?)
    }

    fn settings(&self) -> fdo::Result<String> {
        json(&self.state.with(|s| Ok(s.settings()))?)
    }

    /// Save settings (JSON); returns them as stored.
    async fn set_settings(&self, settings: &str) -> fdo::Result<String> {
        let parsed: Settings = serde_json::from_str(settings).map_err(err)?;
        let saved = self.state.with(|s| s.set_settings(parsed))?;
        if let Ok(tx) = self.state.settings_changed.lock() {
            let _ = tx.send(());
        }
        state::changed(&self.state).await;
        json(&saved)
    }

    async fn set_pinned(&self, id: i64, pinned: bool) -> fdo::Result<bool> {
        let c = self.state.with(|s| s.set_pinned(id, pinned))?;
        if c {
            state::changed(&self.state).await;
        }
        Ok(c)
    }

    /// Out of the Inbox (closing any still open, and telling their apps).
    async fn dismiss(&self, ids: Vec<i64>) -> fdo::Result<bool> {
        state::close_notes(&self.state, &ids, "dismissed").await?;
        let c = self.state.with(|s| s.dismiss(&ids))?;
        state::changed(&self.state).await;
        Ok(c)
    }

    /// Clear the Inbox. Pinned notifications stay pinned.
    async fn dismiss_all(&self) -> fdo::Result<bool> {
        let ids: Vec<i64> = self.state.with(|s| s.inbox())?.iter().map(|n| n.id).collect();
        self.dismiss(ids).await?;
        Ok(true)
    }

    async fn mark_read(&self, ids: Vec<i64>) -> fdo::Result<bool> {
        let c = self.state.with(|s| s.mark_read(&ids))?;
        if c {
            state::changed(&self.state).await;
        }
        Ok(c)
    }

    async fn mark_all_read(&self) -> fdo::Result<bool> {
        let c = self.state.with(|s| s.mark_all_read())?;
        if c {
            state::changed(&self.state).await;
        }
        Ok(c)
    }

    /// Delete for good, pinned or not.
    async fn delete(&self, ids: Vec<i64>) -> fdo::Result<bool> {
        state::close_notes(&self.state, &ids, "dismissed").await?;
        let c = self.state.with(|s| s.delete(&ids))?;
        state::changed(&self.state).await;
        Ok(c)
    }

    /// Delete all history except pinned notifications (open ones are closed first).
    async fn delete_history(&self) -> fdo::Result<bool> {
        let open: Vec<i64> = self.state.with(|s| s.open_notes())?.iter().filter(|n| !n.pinned).map(|n| n.id).collect();
        state::close_notes(&self.state, &open, "dismissed").await?;
        let c = self.state.with(|s| s.delete_history())?;
        state::changed(&self.state).await;
        Ok(c)
    }

    // ---- Pop-ups (stage B)

    /// The pop-ups that should be on screen (JSON list), for a panel that starts late.
    fn popups(&self) -> fdo::Result<String> {
        let open = self.state.with(|s| s.open_notes())?;
        json(&open.into_iter().filter(|n| n.live).collect::<Vec<_>>())
    }

    /// A pop-up went away: `timeout` (the notification stays in the Inbox) or
    /// `dismissed` (closed).
    async fn popup_ended(&self, id: i64, reason: &str) -> fdo::Result<()> {
        state::popup_ended(&self.state, id, reason).await
    }

    /// A click: the app's default action, Omarchy's click command, or open the app.
    async fn activate(&self, id: i64) -> fdo::Result<bool> {
        state::activate(&self.state, id).await
    }

    /// One of the app's own buttons, by its key.
    async fn invoke_action(&self, id: i64, key: &str) -> fdo::Result<bool> {
        state::invoke_action(&self.state, id, key).await
    }

    /// One of Omarchy's command buttons, by position.
    async fn run_action(&self, id: i64, index: u32) -> fdo::Result<bool> {
        state::run_action(&self.state, id, index).await
    }

    // ---- Do not disturb

    fn dnd(&self) -> bool {
        self.state.dnd()
    }

    async fn set_dnd(&self, on: bool) -> bool {
        let ok = self.state.set_dnd(on);
        state::changed(&self.state).await;
        ok && self.state.dnd() == on
    }

    /// Returns the new state.
    async fn toggle_dnd(&self) -> bool {
        let on = !self.state.dnd();
        self.set_dnd(on).await;
        self.state.dnd()
    }

    // ---- For Omarchy's shortcuts (through the shell plugin)

    /// The newest pop-up on screen (JSON), or `""`.
    fn newest(&self) -> fdo::Result<String> {
        let open = self.state.with(|s| s.open_notes())?;
        match open.into_iter().filter(|n| n.live).max_by(|a, b| a.ts.total_cmp(&b.ts)) {
            Some(n) => json(&n),
            None => Ok(String::new()),
        }
    }

    /// Super + comma: act on the newest pop-up (as a click would).
    async fn act_newest(&self) -> fdo::Result<bool> {
        match self.newest_id()? {
            Some(id) => state::activate(&self.state, id).await,
            None => Ok(false),
        }
    }

    /// Dismiss the newest pop-up.
    async fn dismiss_newest(&self) -> fdo::Result<bool> {
        match self.newest_id()? {
            Some(id) => self.dismiss(vec![id]).await,
            None => Ok(false),
        }
    }

    /// The newest pop-up's `n`th button (from 1): the app's own, then Omarchy's.
    async fn action_newest(&self, n: u32) -> fdo::Result<bool> {
        let Some(id) = self.newest_id()? else { return Ok(false) };
        let Some(note) = self.state.with(|s| s.get(id))? else { return Ok(false) };
        let own: Vec<String> = note.app_actions.iter().filter(|a| a.key != "default").map(|a| a.key.clone()).collect();
        let i = n.saturating_sub(1) as usize;
        if let Some(key) = own.get(i) {
            state::invoke_action(&self.state, id, key).await
        } else {
            state::run_action(&self.state, id, (i - own.len()) as u32).await
        }
    }

    /// Close every pop-up on screen (they're dismissed, as Omarchy does).
    async fn dismiss_popups(&self) -> fdo::Result<u32> {
        let ids: Vec<i64> = self.state.with(|s| s.open_notes())?.into_iter().filter(|n| n.live).map(|n| n.id).collect();
        self.dismiss(ids.clone()).await?;
        Ok(ids.len() as u32)
    }

    /// Dismiss open notifications whose title contains `summary`.
    async fn dismiss_matching(&self, summary: &str) -> fdo::Result<bool> {
        if summary.is_empty() {
            return Ok(false);
        }
        let ids: Vec<i64> =
            self.state.with(|s| s.open_notes())?.into_iter().filter(|n| n.summary.contains(summary)).map(|n| n.id).collect();
        if ids.is_empty() {
            return Ok(false);
        }
        self.dismiss(ids).await
    }

    /// Something changed: read again.
    #[zbus(signal)]
    async fn changed(e: &SignalEmitter<'_>) -> zbus::Result<()>;

    /// Show a pop-up: `{note, timeout_ms (0: until dismissed), hide_text}` (JSON). The
    /// same note again means it was replaced.
    #[zbus(signal)]
    async fn popup(e: &SignalEmitter<'_>, payload: &str) -> zbus::Result<()>;

    /// Take a pop-up down: its notification was closed.
    #[zbus(signal)]
    async fn popup_closed(e: &SignalEmitter<'_>, id: i64) -> zbus::Result<()>;
}
