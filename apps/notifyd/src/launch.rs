//! Running what a notification asks for: Omarchy's click command or command buttons, or
//! opening the sending app.

use std::path::PathBuf;

/// Start a program detached from this service, the way Omarchy's launcher does.
pub fn spawn(argv: &[String]) -> bool {
    if argv.is_empty() {
        return false;
    }
    let mut cmd = std::process::Command::new("setsid");
    cmd.arg("-f");
    let uwsm = std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("uwsm-app").is_file()));
    if uwsm {
        cmd.args(["uwsm-app", "--"]);
    }
    cmd.args(argv).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    match cmd.spawn() {
        Ok(mut c) => {
            std::thread::spawn(move || c.wait());
            true
        }
        Err(e) => {
            eprintln!("nyx-notifyd: couldn't start {}: {e}", argv[0]);
            false
        }
    }
}

/// An installed desktop file for `id` (`org.gnome.Nautilus`, `io.github.….Mail`).
pub fn desktop_file(id: &str) -> Option<PathBuf> {
    if id.is_empty() || id.contains('/') || id.contains("..") {
        return None;
    }
    let file = format!("{}.desktop", id.trim_end_matches(".desktop"));
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share"));
    let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    std::iter::once(home)
        .chain(dirs.split(':').filter(|d| !d.is_empty()).map(PathBuf::from))
        .map(|d| d.join("applications").join(&file))
        .find(|p| p.is_file())
}

/// Open the app: `gtk-launch <id>`.
pub fn open_app(id: &str) -> bool {
    let Some(_) = desktop_file(id) else { return false };
    spawn(&["gtk-launch".to_string(), format!("{}.desktop", id.trim_end_matches(".desktop"))])
}
