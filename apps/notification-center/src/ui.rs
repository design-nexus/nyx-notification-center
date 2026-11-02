//! Small building blocks: icons, labels, buttons, times, starting programs.

use gtk::glib;
use gtk::prelude::*;
use std::future::Future;

/// Run an action in the background; report a failure on stderr.
pub fn act(fut: impl Future<Output = Result<(), String>> + 'static) {
    glib::MainContext::default().spawn_local(async move {
        if let Err(e) = fut.await {
            eprintln!("nyx-notification-center: {e}");
        }
    });
}

pub fn icon(name: &str, size: i32) -> gtk::Image {
    let i = gtk::Image::from_icon_name(name);
    i.set_pixel_size(size);
    i
}

pub fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class(class);
    l.set_xalign(0.0);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    // Ask for no width of its own: the panel's width decides, and long text ellipsizes.
    l.set_max_width_chars(1);
    l.set_hexpand(true);
    l
}

/// An icon button with a tooltip (STYLE.md: actions are icons, named in the tooltip).
pub fn icon_button(name: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(name);
    b.set_tooltip_text(Some(tip));
    b.add_css_class("nc-icon-button");
    b
}

pub fn set_class(w: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if on {
        w.add_css_class(class);
    } else {
        w.remove_css_class(class);
    }
}

fn detached(args: &[&str]) -> std::process::Command {
    let mut cmd = std::process::Command::new("setsid");
    cmd.arg("-f");
    let uwsm = std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("uwsm-app").is_file()));
    if uwsm {
        cmd.args(["uwsm-app", "--"]);
    }
    cmd.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    cmd
}

/// Start a program the way Omarchy's launcher does, detached from the panel.
pub fn launch(args: &[&str]) {
    match detached(args).spawn() {
        Ok(mut c) => {
            std::thread::spawn(move || c.wait());
        }
        Err(e) => eprintln!("nyx-notification-center: couldn't start {}: {e}", args.join(" ")),
    }
}

/// Run a short command for its effect, without waiting.
pub fn spawn(args: &[&str]) {
    let mut cmd = std::process::Command::new(args[0]);
    cmd.args(&args[1..]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null());
    match cmd.spawn() {
        Ok(mut c) => {
            std::thread::spawn(move || c.wait());
        }
        Err(e) => eprintln!("nyx-notification-center: couldn't run {}: {e}", args[0]),
    }
}

fn local(ts: f64) -> Option<glib::DateTime> {
    glib::DateTime::from_unix_local(ts as i64).ok()
}

/// "now", "5 min", "14:15" today, "Yesterday", "Tue" this week, "Oct 28" before that.
pub fn when(ts: f64, now: f64) -> String {
    let ago = now - ts;
    if ago < 60.0 {
        return "now".into();
    }
    if ago < 3600.0 {
        return format!("{} min", (ago / 60.0) as i64);
    }
    let (Some(t), Some(n)) = (local(ts), local(now)) else { return String::new() };
    let days = n.day_of_year() - t.day_of_year() + if n.year() != t.year() { 365 } else { 0 };
    match days {
        0 => t.format("%H:%M").map(|s| s.to_string()).unwrap_or_default(),
        1 => "Yesterday".into(),
        2..=6 => t.format("%a").map(|s| s.to_string()).unwrap_or_default(),
        _ => t.format("%b %-d").map(|s| s.to_string()).unwrap_or_default(),
    }
}

/// A day heading for history: "Today", "Yesterday", "Oct 28".
pub fn day(ts: f64, now: f64) -> String {
    let (Some(t), Some(n)) = (local(ts), local(now)) else { return String::new() };
    if t.year() == n.year() && t.day_of_year() == n.day_of_year() {
        return "Today".into();
    }
    let y = n.add_days(-1).ok();
    if y.is_some_and(|y| y.year() == t.year() && y.day_of_year() == t.day_of_year()) {
        return "Yesterday".into();
    }
    let fmt = if t.year() == n.year() { "%b %-d" } else { "%b %-d, %Y" };
    t.format(fmt).map(|s| s.to_string()).unwrap_or_default()
}

/// The same calendar day.
pub fn same_day(a: f64, b: f64) -> bool {
    match (local(a), local(b)) {
        (Some(x), Some(y)) => x.year() == y.year() && x.day_of_year() == y.day_of_year(),
        _ => false,
    }
}
