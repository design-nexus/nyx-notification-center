//! Nyx Notification Center: an inbox, pins and searchable history for notifications.
//!
//! `nyx-notification-center` starts it (hidden; normally as a user service).
//! `nyx-notification-center --toggle | --show | --hide` opens or closes the running one;
//! `--toggle pinned | history` opens it on that view. `--only <output>` keeps it on one
//! output (for trying it on a test output). `--use-nyx-popups` / `--use-omarchy-popups`
//! choose who receives notifications (see `switch`).

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

fn main() -> std::process::ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("--use-nyx-popups") => return switch::use_nyx(),
        Some("--use-omarchy-popups") => return switch::use_omarchy(),
        Some("--popups") => return switch::status(),
        _ => {}
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
                        nyx-notification-center --use-nyx-popups | --use-omarchy-popups | --popups\n\n\
                 With no option, start (hidden). The options open or close the running one;\n\
                 --toggle with a view's name opens it on that view. --use-nyx-popups has Nyx\n\
                 receive notifications and show the pop-ups; --use-omarchy-popups gives that back\n\
                 to Omarchy; --popups says which is in use."
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
