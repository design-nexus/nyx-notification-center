//! nyx-notifyd: keeps every notification in Nyx's history.
//!
//! Stage A: Omarchy's shell still receives notifications and shows the pop-ups. This
//! service watches its notification files and copies each notification into the store,
//! then serves the store on the session bus as `io.github.design_nexus.nyx.Notifications1`
//! (JSON in and out, a `Changed` signal after every change). It also runs the Do not
//! disturb schedule, switching Omarchy's own setting at the start and end of the quiet
//! hours, and drops history past its retention period once an hour.

use nyx_notify_store::{Query, Settings, Store, now, omarchy};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::object_server::SignalEmitter;
use zbus::{fdo, interface};

const BUS: &str = "io.github.design_nexus.nyx.Notifications";
const PATH: &str = "/io/github/design_nexus/nyx/Notifications";

type Shared = Arc<Mutex<Store>>;

struct Service {
    store: Shared,
    /// Wakes the schedule when the settings change.
    settings_changed: mpsc::Sender<()>,
}

fn err(e: impl std::fmt::Display) -> fdo::Error {
    fdo::Error::Failed(e.to_string())
}

fn json<T: serde::Serialize>(v: &T) -> fdo::Result<String> {
    serde_json::to_string(v).map_err(err)
}

impl Service {
    fn with<T>(&self, f: impl FnOnce(&mut Store) -> nyx_notify_store::Result<T>) -> fdo::Result<T> {
        let mut s = self.store.lock().map_err(err)?;
        f(&mut s).map_err(err)
    }

    async fn changed_if(&self, changed: bool, e: &SignalEmitter<'_>) -> fdo::Result<bool> {
        if changed {
            let _ = Self::changed(e).await;
        }
        Ok(changed)
    }
}

#[interface(name = "io.github.design_nexus.nyx.Notifications1")]
impl Service {
    /// Not dismissed, newest first (JSON list).
    fn inbox(&self) -> fdo::Result<String> {
        json(&self.with(|s| s.inbox())?)
    }

    /// Pinned, most recently pinned first.
    fn pinned(&self) -> fdo::Result<String> {
        json(&self.with(|s| s.pinned())?)
    }

    /// History matching `query` (JSON `{text, app, days, limit}`; `""` for everything).
    fn history(&self, query: &str) -> fdo::Result<String> {
        let q: Query = if query.trim().is_empty() { Query::default() } else { serde_json::from_str(query).map_err(err)? };
        json(&self.with(|s| s.history(&q, now()))?)
    }

    /// Apps with stored notifications, and the ones left out of history.
    fn apps(&self) -> fdo::Result<String> {
        json(&self.with(|s| s.apps())?)
    }

    fn settings(&self) -> fdo::Result<String> {
        json(&self.with(|s| Ok(s.settings()))?)
    }

    /// Save settings (JSON); returns them as stored.
    async fn set_settings(&self, settings: &str, #[zbus(signal_emitter)] e: SignalEmitter<'_>) -> fdo::Result<String> {
        let parsed: Settings = serde_json::from_str(settings).map_err(err)?;
        let saved = self.with(|s| s.set_settings(parsed))?;
        let _ = self.settings_changed.send(());
        let _ = Self::changed(&e).await;
        json(&saved)
    }

    async fn set_pinned(&self, id: i64, pinned: bool, #[zbus(signal_emitter)] e: SignalEmitter<'_>) -> fdo::Result<bool> {
        let c = self.with(|s| s.set_pinned(id, pinned))?;
        self.changed_if(c, &e).await
    }

    async fn dismiss(&self, ids: Vec<i64>, #[zbus(signal_emitter)] e: SignalEmitter<'_>) -> fdo::Result<bool> {
        let c = self.with(|s| s.dismiss(&ids))?;
        self.changed_if(c, &e).await
    }

    /// Clear the Inbox. Pinned notifications stay pinned.
    async fn dismiss_all(&self, #[zbus(signal_emitter)] e: SignalEmitter<'_>) -> fdo::Result<bool> {
        let c = self.with(|s| s.dismiss_all())?;
        self.changed_if(c, &e).await
    }

    async fn mark_read(&self, ids: Vec<i64>, #[zbus(signal_emitter)] e: SignalEmitter<'_>) -> fdo::Result<bool> {
        let c = self.with(|s| s.mark_read(&ids))?;
        self.changed_if(c, &e).await
    }

    async fn mark_all_read(&self, #[zbus(signal_emitter)] e: SignalEmitter<'_>) -> fdo::Result<bool> {
        let c = self.with(|s| s.mark_all_read())?;
        self.changed_if(c, &e).await
    }

    /// Delete for good, pinned or not.
    async fn delete(&self, ids: Vec<i64>, #[zbus(signal_emitter)] e: SignalEmitter<'_>) -> fdo::Result<bool> {
        let c = self.with(|s| s.delete(&ids))?;
        self.changed_if(c, &e).await
    }

    /// Delete all history except pinned notifications.
    async fn delete_history(&self, #[zbus(signal_emitter)] e: SignalEmitter<'_>) -> fdo::Result<bool> {
        let c = self.with(|s| s.delete_history())?;
        self.changed_if(c, &e).await
    }

    /// Something changed: read again.
    #[zbus(signal)]
    async fn changed(e: &SignalEmitter<'_>) -> zbus::Result<()>;
}

fn ingest(store: &Shared, dir: &Path) -> bool {
    let (live, history) = omarchy::read(dir);
    match store.lock() {
        Ok(mut s) => s.ingest(&live, &history, now()).unwrap_or_else(|e| {
            eprintln!("nyx-notifyd: {e}");
            false
        }),
        Err(_) => false,
    }
}

fn announce(conn: &zbus::blocking::Connection) {
    if let Ok(iface) = conn.object_server().interface::<_, Service>(PATH) {
        let _ = zbus::block_on(Service::changed(iface.signal_emitter()));
    }
}

/// Copy new notifications in whenever Omarchy writes its files.
fn watch(store: Shared, conn: zbus::blocking::Connection, dir: PathBuf) {
    use inotify::{Inotify, WatchMask};
    loop {
        let _ = std::fs::create_dir_all(&dir);
        let Ok(mut ino) = Inotify::init() else { return };
        if ino.watches().add(&dir, WatchMask::CLOSE_WRITE | WatchMask::MOVED_TO | WatchMask::DELETE_SELF).is_err() {
            std::thread::sleep(Duration::from_secs(10));
            continue;
        }
        if ingest(&store, &dir) {
            announce(&conn);
        }
        let mut buf = [0u8; 4096];
        'events: loop {
            let Ok(events) = ino.read_events_blocking(&mut buf) else { break };
            let mut relevant = false;
            for ev in events {
                if ev.mask.contains(inotify::EventMask::DELETE_SELF) || ev.mask.contains(inotify::EventMask::IGNORED) {
                    break 'events;
                }
                relevant |= ev.name.is_some_and(|n| n == "live.json" || n == "history.json");
            }
            if !relevant {
                continue;
            }
            // The shell writes in bursts: wait for it to settle, then read once.
            std::thread::sleep(Duration::from_millis(150));
            while ino.read_events(&mut buf).is_ok_and(|mut e| e.next().is_some()) {}
            if ingest(&store, &dir) {
                announce(&conn);
            }
        }
        // The folder went away (or the watch failed): start over.
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// Minutes after local midnight, and seconds into that minute.
fn local_minute() -> (u16, u64) {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        ((tm.tm_hour * 60 + tm.tm_min) as u16, tm.tm_sec.max(0) as u64)
    }
}

fn set_dnd(on: bool, dir: &Path) {
    if omarchy::dnd(dir) == on {
        return;
    }
    let ok = std::process::Command::new("omarchy-shell")
        .args(["-q", "notifications", "setDnd", if on { "on" } else { "off" }])
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        let _ = std::process::Command::new("omarchy-shell").args(["-q", "omarchy.indicators", "refresh"]).status();
    } else {
        eprintln!("nyx-notifyd: couldn't switch Do not disturb {}", if on { "on" } else { "off" });
    }
}

/// The Do not disturb schedule and hourly retention. DND is switched only when the
/// quiet hours start or end (or the schedule is changed), so switching it by hand in
/// between sticks.
fn schedule(store: Shared, conn: zbus::blocking::Connection, dir: PathBuf, wake: mpsc::Receiver<()>) {
    let mut last: Option<bool> = None;
    let mut last_prune = 0.0;
    loop {
        let settings = store.lock().map(|s| s.settings()).unwrap_or_default();
        let (minute, sec) = local_minute();
        let quiet = settings.schedule.quiet_at(minute);
        if settings.schedule.enabled && last != Some(quiet) {
            set_dnd(quiet, &dir);
        }
        last = settings.schedule.enabled.then_some(quiet);
        if now() - last_prune > 3600.0 {
            last_prune = now();
            if store.lock().map(|mut s| s.prune(now()).unwrap_or(false)).unwrap_or(false) {
                announce(&conn);
            }
        }
        // Next minute boundary while a schedule is on; otherwise just the hourly prune.
        let wait = if settings.schedule.enabled { 60 - sec.min(59) } else { 3600 };
        if let Ok(()) = wake.recv_timeout(Duration::from_secs(wait)) {
            last = None;
        }
    }
}

fn main() -> std::process::ExitCode {
    let path = Store::default_path();
    let store = match Store::open(&path) {
        Ok(s) => Arc::new(Mutex::new(s)),
        Err(e) => {
            eprintln!("nyx-notifyd: can't open {}: {e}", path.display());
            return std::process::ExitCode::FAILURE;
        }
    };
    let dir = omarchy::dir();
    let (tx, rx) = mpsc::channel();
    let conn = match zbus::blocking::connection::Builder::session()
        .and_then(|b| b.name(BUS))
        .and_then(|b| b.serve_at(PATH, Service { store: store.clone(), settings_changed: tx }))
        .and_then(|b| b.build())
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("nyx-notifyd: session bus: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    {
        let (store, conn, dir) = (store.clone(), conn.clone(), dir.clone());
        std::thread::spawn(move || schedule(store, conn, dir, rx));
    }
    watch(store, conn, dir);
    std::process::ExitCode::FAILURE
}
