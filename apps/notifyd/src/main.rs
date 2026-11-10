//! nyx-notifyd: keeps every notification in Nyx's history.
//!
//! - **Omarchy mode** (stage A): Omarchy's shell receives notifications and shows the
//!   pop-ups. This service watches its notification files and copies each one into the
//!   store.
//! - **Nyx mode** (stage B): this service is `org.freedesktop.Notifications`. It stores
//!   each notification as it arrives and asks the Notification Center to show the pop-up.
//!
//! Either way it serves the store as `io.github.design_nexus.nyx.Notifications1` (JSON
//! in and out, a `Changed` signal), runs the Do not disturb schedule and drops history
//! past its retention period once an hour.
//!
//! `nyx-notifyd --mode` prints the mode; `--set-mode nyx|omarchy` changes it for the
//! next start (switching is done by `nyx-notification-center --use-nyx-popups` /
//! `--use-omarchy-popups`, which also hands Omarchy's part over).

mod fdo;
mod flux;
mod image;
mod launch;
mod quiet;
mod service;
mod state;

use nyx_notify_store::{Store, now, omarchy};
use state::State;
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const BUS: &str = "io.github.design_nexus.nyx.Notifications";

fn ingest(st: &State, dir: &Path) -> bool {
    let (live, history) = omarchy::read(dir);
    match st.store.lock() {
        Ok(mut s) => s.ingest(&live, &history, now()).unwrap_or_else(|e| {
            eprintln!("nyx-notifyd: {e}");
            false
        }),
        Err(_) => false,
    }
}

fn announce(st: &State) {
    zbus::block_on(state::changed(st));
}

/// Copy new notifications in whenever Omarchy writes its files.
fn watch(st: Arc<State>) {
    let dir = st.dir.clone();
    use inotify::{Inotify, WatchMask};
    loop {
        let _ = std::fs::create_dir_all(&dir);
        let Ok(mut ino) = Inotify::init() else { return };
        if ino.watches().add(&dir, WatchMask::CLOSE_WRITE | WatchMask::MOVED_TO | WatchMask::DELETE_SELF).is_err() {
            std::thread::sleep(Duration::from_secs(10));
            continue;
        }
        if ingest(&st, &dir) {
            announce(&st);
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
            if ingest(&st, &dir) {
                announce(&st);
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

/// The Do not disturb schedule and hourly retention. DND is switched only when the
/// quiet hours start or end (or the schedule is changed), so switching it by hand in
/// between sticks.
fn schedule(st: Arc<State>, wake: mpsc::Receiver<()>) {
    let mut last: Option<bool> = None;
    let mut last_prune = 0.0;
    loop {
        let settings = st.store.lock().map(|s| s.settings()).unwrap_or_default();
        let (minute, sec) = local_minute();
        let quiet = settings.schedule.quiet_at(minute);
        if settings.schedule.enabled && last != Some(quiet) {
            st.set_dnd(quiet);
        }
        last = settings.schedule.enabled.then_some(quiet);
        if now() - last_prune > 3600.0 {
            last_prune = now();
            if st.store.lock().map(|mut s| s.prune(now()).unwrap_or(false)).unwrap_or(false) {
                announce(&st);
            }
            image::prune(settings.retention_days);
        }
        // Codes in history are masked once they're surely used or expired.
        if st.store.lock().map(|mut s| s.mask_codes(now() - 600.0).unwrap_or(false)).unwrap_or(false) {
            announce(&st);
        }
        // Next minute boundary while a schedule is on; otherwise just the hourly prune.
        let wait = if settings.schedule.enabled { 60 - sec.min(59) } else { 3600 };
        let wait = if settings.keep_codes { wait } else { wait.min(300) };
        if let Ok(()) = wake.recv_timeout(Duration::from_secs(wait)) {
            last = None;
        }
    }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = Store::default_path();
    let mut store = match Store::open(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("nyx-notifyd: can't open {}: {e}", path.display());
            return std::process::ExitCode::FAILURE;
        }
    };
    if args.first().map(String::as_str) == Some("--mode") {
        println!("{}", store.meta("mode").unwrap_or_else(|| "omarchy".into()));
        return std::process::ExitCode::SUCCESS;
    }
    if args.first().map(String::as_str) == Some("--set-mode") {
        return match args.get(1).map(String::as_str) {
            Some(m @ ("nyx" | "omarchy")) => match store.set_meta("mode", m) {
                Ok(()) => std::process::ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("nyx-notifyd: {e}");
                    std::process::ExitCode::FAILURE
                }
            },
            _ => {
                eprintln!("Usage: nyx-notifyd --set-mode nyx|omarchy");
                std::process::ExitCode::FAILURE
            }
        };
    }
    if !args.is_empty() {
        eprintln!("Usage: nyx-notifyd [--mode | --set-mode nyx|omarchy]");
        return std::process::ExitCode::FAILURE;
    }
    let nyx_mode = match std::env::var("NYX_NOTIFYD_MODE").ok().as_deref() {
        Some("nyx") => true,
        Some("omarchy") => false,
        _ => store.meta("mode").as_deref() == Some("nyx"),
    };
    // Whatever was open when the service stopped can't be answered any more: the apps
    // that sent it have forgotten the ids.
    if nyx_mode && let Ok(open) = store.open_notes() {
        let ids: Vec<i64> = open.iter().map(|n| n.id).collect();
        let _ = store.close(&ids, "expired", now());
    }
    state::publish(&store);
    let (tx, rx) = mpsc::channel();
    let st = Arc::new(State {
        store: Mutex::new(store),
        nyx_mode,
        dir: omarchy::dir(),
        settings_changed: Mutex::new(tx),
        generation: Mutex::default(),
        conn: OnceLock::new(),
        offered: Mutex::new(now()),
    });
    let built = zbus::blocking::connection::Builder::session()
        .and_then(|b| b.serve_at(state::PATH, service::Service { state: st.clone() }))
        .and_then(|b| if nyx_mode { b.serve_at(fdo::PATH, fdo::Notifications { state: st.clone() }) } else { Ok(b) })
        .and_then(|b| b.build());
    let conn = match built {
        Ok(c) => c,
        Err(e) => {
            eprintln!("nyx-notifyd: session bus: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    // One at a time: a second copy leaves the running one alone.
    match conn.request_name_with_flags(BUS, zbus::fdo::RequestNameFlags::DoNotQueue.into()) {
        Ok(zbus::fdo::RequestNameReply::PrimaryOwner) => {}
        Ok(_) => {
            eprintln!("nyx-notifyd: already running");
            return std::process::ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("nyx-notifyd: {BUS}: {e}");
            return std::process::ExitCode::FAILURE;
        }
    }
    let _ = st.conn.set(conn.inner().clone());
    if nyx_mode {
        // Queued if Omarchy's shell still has it; ours as soon as it lets go.
        match conn.request_name_with_flags(fdo::NAME, zbus::fdo::RequestNameFlags::AllowReplacement.into()) {
            Ok(zbus::fdo::RequestNameReply::PrimaryOwner | zbus::fdo::RequestNameReply::AlreadyOwner) => {}
            Ok(r) => eprintln!("nyx-notifyd: {} is taken ({r:?}); waiting for it", fdo::NAME),
            Err(e) => eprintln!("nyx-notifyd: {}: {e}", fdo::NAME),
        }
    }
    {
        let st = st.clone();
        std::thread::spawn(move || schedule(st, rx));
    }
    if nyx_mode {
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
    watch(st);
    std::process::ExitCode::FAILURE
}
