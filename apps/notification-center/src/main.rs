//! Nyx Notification Center: an inbox, pins and searchable history for notifications.
//!
//! `nyx-notification-center` starts it (hidden; normally as a user service).
//! `nyx-notification-center --toggle | --show | --hide` opens or closes the running one;
//! `--toggle pinned | history` opens it on that view. `--only <output>` keeps it on one
//! output (for trying it on a test output). `--use-nyx-popups` / `--use-omarchy-popups`
//! choose who receives notifications (see `switch`). `--ui qml|gtk` picks the
//! Quickshell or the GTK interface; with the Quickshell one on, `--toggle`, `--show` and
//! `--hide` go to its backend (nyx-notification-center-backend).

mod card;
mod choice;
mod client;
mod panel;
mod popups;
mod switch;
mod theme;
mod ui;

use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

pub const APP_ID: &str = "io.github.design_nexus.nyx.NotificationCenter";

const COMPONENT: nyx_ui_link::switch::Component = nyx_ui_link::switch::Component {
    app: "nyx-notification-center",
    gtk_unit: "nyx-notification-center.service",
    backend_units: &["nyx-notification-center-backend.service"],
};

/// `--ui qml|gtk`, and the command line's requests when the Quickshell interface runs.
fn quickshell(args: &[String]) -> Option<std::process::ExitCode> {
    use nyx_ui_link::switch::{self, Ui};
    if let Some(i) = args.iter().position(|a| a == "--ui") {
        let Some(ui) = args.get(i + 1).and_then(|a| Ui::parse(a)) else {
            println!("{}", if switch::current(&COMPONENT) == Ui::Qml { "qml" } else { "gtk" });
            return Some(std::process::ExitCode::SUCCESS);
        };
        return Some(match switch::set(&COMPONENT, ui) {
            Ok(msg) => {
                println!("{msg}");
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("nyx-notification-center: {e}");
                std::process::ExitCode::FAILURE
            }
        });
    }
    if args.iter().any(|a| a == "--only") {
        return None;
    }
    let (op, body) = if let Some(i) = args.iter().position(|a| a == "--toggle") {
        ("Toggle", serde_json::json!({"view": args.get(i + 1).filter(|a| !a.starts_with("--"))}))
    } else if args.iter().any(|a| a == "--show") {
        ("Show", serde_json::json!({}))
    } else if args.iter().any(|a| a == "--hide") {
        ("Hide", serde_json::json!({}))
    } else if switch::current(&COMPONENT) == Ui::Qml && !args.iter().any(|a| a.starts_with('-')) {
        // Started plainly (the GTK unit, by hand) while the Quickshell panel is on.
        eprintln!("nyx-notification-center: the Quickshell panel is on (nyx-notification-center-backend); switch back with --ui gtk");
        return Some(std::process::ExitCode::SUCCESS);
    } else {
        return None;
    };
    match switch::forward("nyx-notification-center", op, body)? {
        Ok(_) => Some(std::process::ExitCode::SUCCESS),
        Err(e) => {
            eprintln!("nyx-notification-center: {e}");
            Some(std::process::ExitCode::FAILURE)
        }
    }
}

/// `--copy-code`: the code on standard input goes to Nyx Clipboard (marked sensitive,
/// cleared after a while), else to the clipboard with wl-copy. For the Quickshell
/// panel, whose backend Nyx Clipboard doesn't take codes from.
fn copy_code() -> std::process::ExitCode {
    use std::io::{Read, Write};
    let mut code = String::new();
    if std::io::stdin().take(256).read_to_string(&mut code).is_err() || code.trim().is_empty() {
        return std::process::ExitCode::FAILURE;
    }
    let code = code.trim();
    let sent = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).is_ok_and(|bus| {
        bus.call_sync(
            Some("io.github.design_nexus.nyx.Clipboard1"),
            "/io/github/design_nexus/nyx/Clipboard1",
            "io.github.design_nexus.nyx.Clipboard.Otp1",
            "CopyCode",
            Some(&(code,).to_variant()),
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            3000,
            gio::Cancellable::NONE,
        )
        .is_ok()
    });
    if sent {
        return std::process::ExitCode::SUCCESS;
    }
    let copied = std::process::Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            c.stdin.take().map(|mut i| i.write_all(code.as_bytes())).transpose()?;
            c.wait()
        })
        .is_ok_and(|s| s.success());
    if copied { std::process::ExitCode::SUCCESS } else { std::process::ExitCode::FAILURE }
}

fn main() -> std::process::ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("--use-nyx-popups") => return switch::use_nyx(),
        Some("--use-omarchy-popups") => return switch::use_omarchy(),
        Some("--popups") => return switch::status(),
        Some("--copy-code") => return copy_code(),
        _ => {}
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.iter().any(|a| a == "-h" || a == "--help")
        && let Some(code) = quickshell(&args)
    {
        return code;
    }
    // A test copy (`--only <output>`) is separate: its own id, never the running panel.
    let test = std::env::args().any(|a| a == "--only");
    let (id, mut flags) = if test {
        (format!("{APP_ID}.Test"), gio::ApplicationFlags::NON_UNIQUE)
    } else {
        (APP_ID.to_string(), gio::ApplicationFlags::empty())
    };
    flags |= gio::ApplicationFlags::HANDLES_COMMAND_LINE;
    let app = gtk::Application::builder().application_id(id).flags(flags).build();
    let panel: Rc<RefCell<Option<Rc<panel::Panel>>>> = Rc::default();
    app.connect_startup(|app| {
        theme::install();
        // Resident: closing the panel doesn't quit.
        std::mem::forget(app.hold());
    });
    let p = panel.clone();
    app.connect_command_line(move |app, cmd| {
        let args: Vec<String> = cmd.arguments().iter().skip(1).map(|a| a.to_string_lossy().to_string()).collect();
        if args.iter().any(|a| a == "-h" || a == "--help") {
            println!(
                "Usage: nyx-notification-center [--toggle [inbox|pinned|history] | --show | --hide] [--only <output>]\n\
                        nyx-notification-center --use-nyx-popups | --use-omarchy-popups | --popups\n\
                        nyx-notification-center --ui [qml|gtk]\n\n\
                 With no option, start (hidden). The options open or close the running one;\n\
                 --toggle with a view's name opens it on that view. --use-nyx-popups has Nyx\n\
                 receive notifications and show the pop-ups; --use-omarchy-popups gives that back\n\
                 to Omarchy; --popups says which is in use. --ui uses the Quickshell (qml) or\n\
                 the GTK panel; alone, it says which is on."
            );
            return glib::ExitCode::SUCCESS;
        }
        let only = args.iter().position(|a| a == "--only").and_then(|i| args.get(i + 1)).cloned();
        if p.borrow().is_none() {
            let panel = panel::build(app, only.clone());
            let w = Rc::downgrade(&panel);
            let pops = popups::start(app, only, Rc::new(move || w.upgrade().is_some_and(|p| p.is_open())));
            panel.set_popups(pops);
            *p.borrow_mut() = Some(panel);
        }
        let panel = p.borrow().clone().expect("built");
        if let Some(i) = args.iter().position(|a| a == "--toggle") {
            match args.get(i + 1).and_then(|a| panel::View::parse(a)) {
                Some(v) => panel.toggle_view(v),
                None => panel.toggle(),
            }
        } else if args.iter().any(|a| a == "--show") {
            panel.show();
        } else if args.iter().any(|a| a == "--hide") {
            panel.hide();
        }
        glib::ExitCode::SUCCESS
    });
    let code = app.run();
    std::process::ExitCode::from(code.get())
}
