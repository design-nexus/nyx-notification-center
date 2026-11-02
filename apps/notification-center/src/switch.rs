//! Who receives notifications: Omarchy's shell (stage A, the default) or nyx-notifyd
//! with the Notification Center's pop-ups (stage B). One command each way:
//!
//! - `--use-nyx-popups`: nyx-notifyd switches to Nyx mode, Omarchy's notification
//!   service is turned off (`disabledPlugins`), and the bridge plugin that keeps
//!   Omarchy's shortcuts and scripts working is turned on.
//! - `--use-omarchy-popups`: all of that undone.
//!
//! Nothing is deleted either way; history carries on in the same database.

use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

const BRIDGE: &str = "design-nexus.notifications";

fn run(args: &[&str]) -> bool {
    match Command::new(args[0]).args(&args[1..]).output() {
        Ok(o) if o.status.success() => true,
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            eprintln!("  {}: {}", args.join(" "), err.trim());
            false
        }
        Err(e) => {
            eprintln!("  {}: {e}", args[0]);
            false
        }
    }
}

/// Who answers as `org.freedesktop.Notifications` now.
pub fn server() -> Option<String> {
    let o = Command::new("busctl")
        .args([
            "--user",
            "call",
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "GetServerInformation",
        ])
        .output()
        .ok()?;
    if !o.status.success() {
        return None;
    }
    // `ssss "Name" "vendor" "version" "spec"`
    String::from_utf8_lossy(&o.stdout).split('"').nth(1).map(str::to_string)
}

fn wait_for(name_starts: &str, secs: u64) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        if server().is_some_and(|s| s.starts_with(name_starts)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    false
}

/// The running nyx-notifyd's mode (asked over D-Bus; never starts anything).
fn mode() -> Option<String> {
    let o = Command::new("busctl")
        .args([
            "--user",
            "call",
            "io.github.design_nexus.nyx.Notifications",
            "/io/github/design_nexus/nyx/Notifications",
            "io.github.design_nexus.nyx.Notifications1",
            "Mode",
        ])
        .output()
        .ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).split('"').nth(1).unwrap_or_default().to_string())
}

pub fn status() -> ExitCode {
    let mode = mode();
    println!("Pop-ups: {}", if mode.as_deref() == Some("nyx") { "Nyx Notification Center" } else { "Omarchy" });
    println!("Notification server: {}", server().unwrap_or_else(|| "none".into()));
    ExitCode::SUCCESS
}

pub fn use_nyx() -> ExitCode {
    println!("Switching notifications to Nyx Notification Center…");
    if !run(&["nyx-notifyd", "--set-mode", "nyx"]) {
        return ExitCode::FAILURE;
    }
    run(&["omarchy-plugin-enable", BRIDGE]);
    run(&["omarchy-plugin-disable", "omarchy.notifications"]);
    run(&["systemctl", "--user", "restart", "nyx-notifyd.service"]);
    if wait_for("Nyx", 6) {
        println!("Done: Nyx shows the pop-ups. Switch back with: nyx-notification-center --use-omarchy-popups");
        return ExitCode::SUCCESS;
    }
    // Omarchy's shell may keep the bus name until it restarts.
    println!("Omarchy's shell still holds notifications; restarting it…");
    run(&["omarchy-restart-shell"]);
    if wait_for("Nyx", 15) {
        println!("Done: Nyx shows the pop-ups. Switch back with: nyx-notification-center --use-omarchy-popups");
        ExitCode::SUCCESS
    } else {
        eprintln!("Nyx didn't get the notification service; switching back.");
        use_omarchy();
        ExitCode::FAILURE
    }
}

pub fn use_omarchy() -> ExitCode {
    println!("Switching notifications back to Omarchy…");
    run(&["nyx-notifyd", "--set-mode", "omarchy"]);
    run(&["omarchy-plugin-disable", BRIDGE]);
    run(&["omarchy-plugin-enable", "omarchy.notifications"]);
    run(&["systemctl", "--user", "restart", "nyx-notifyd.service"]);
    if wait_for("", 8) && !server().is_some_and(|s| s.starts_with("Nyx")) {
        println!("Done: Omarchy shows the pop-ups again; Nyx keeps the history.");
        ExitCode::SUCCESS
    } else {
        println!("Restarting Omarchy's shell so it takes notifications back…");
        run(&["omarchy-restart-shell"]);
        if wait_for("", 15) {
            println!("Done: Omarchy shows the pop-ups again; Nyx keeps the history.");
            ExitCode::SUCCESS
        } else {
            eprintln!("No notification server answered; run: omarchy-restart-shell");
            ExitCode::FAILURE
        }
    }
}
