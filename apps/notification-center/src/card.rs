//! One notification: the app's icon, who and when, the title and text, its buttons.
//! Hovering shows pin and dismiss (or delete); clicking runs what the sender asked for,
//! or opens the app.

use crate::client;
use crate::ui::{self, act, icon, icon_button, label, set_class};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use nyx_notify_store::Note;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Inbox,
    Pinned,
    History,
    /// A pop-up (stage B).
    Popup,
}

/// What a card can ask of the panel.
#[derive(Clone)]
pub struct Ctx {
    pub hide: Rc<dyn Fn()>,
    /// Words to mark in the text (history search).
    pub highlight: Rc<Vec<String>>,
    /// Show only the app and title (the privacy setting, for pop-ups).
    pub hide_text: bool,
}

/// The app's icon: its theme icon or file, else Omarchy's glyph, else a generic one.
pub fn app_icon(n_icon: &str, glyph: &str, size: i32) -> gtk::Widget {
    if n_icon.starts_with('/') || n_icon.starts_with("file://") {
        let path = n_icon.trim_start_matches("file://");
        if std::path::Path::new(path).is_file() {
            let i = gtk::Image::from_file(path);
            i.set_pixel_size(size);
            return i.upcast();
        }
    }
    let theme = gdk::Display::default().map(|d| gtk::IconTheme::for_display(&d));
    if !n_icon.is_empty() && theme.as_ref().is_some_and(|t| t.has_icon(n_icon)) {
        return icon(n_icon, size).upcast();
    }
    let chip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    chip.add_css_class("nc-glyph");
    chip.set_size_request(size, size);
    chip.set_valign(gtk::Align::Start);
    if !glyph.is_empty() {
        let l = gtk::Label::new(Some(glyph));
        l.set_hexpand(true);
        chip.append(&l);
    } else {
        let i = icon("application-x-executable-symbolic", size / 2 + 1);
        i.set_hexpand(true);
        chip.append(&i);
    }
    // The icon centres inside; the box itself takes no spare width.
    chip.set_hexpand(false);
    chip.set_halign(gtk::Align::Start);
    chip.upcast()
}

/// A display name: Omarchy's own messages say "Omarchy", not "omarchy-action".
pub fn app_name(app: &str) -> String {
    match app {
        "omarchy-action" => "Omarchy".into(),
        "notify-send" | "" => "Notification".into(),
        a => a.to_string(),
    }
}

/// Escape for Pango, marking each search word.
fn marked(text: &str, words: &[String]) -> String {
    if words.is_empty() {
        return glib::markup_escape_text(text).to_string();
    }
    let lower = text.to_lowercase();
    // Byte ranges to mark (case-insensitive, ASCII-safe lengths: same text, lowercased).
    let mut spans: Vec<(usize, usize)> = Vec::new();
    if lower.len() == text.len() {
        for w in words {
            let w = w.to_lowercase();
            if w.is_empty() {
                continue;
            }
            let mut from = 0;
            while let Some(i) = lower[from..].find(&w) {
                spans.push((from + i, from + i + w.len()));
                from += i + w.len();
            }
        }
    }
    spans.sort();
    let mut out = String::new();
    let mut at = 0;
    for (s, e) in spans {
        if s < at {
            continue;
        }
        out.push_str(&glib::markup_escape_text(&text[at..s]));
        out.push_str("<span background=\"#7aa2f755\">");
        out.push_str(&glib::markup_escape_text(&text[s..e]));
        out.push_str("</span>");
        at = e;
    }
    out.push_str(&glib::markup_escape_text(&text[at..]));
    out
}

/// The app's icon name is its desktop id (as Nyx apps and most others send it).
fn has_desktop(id: &str) -> bool {
    !id.is_empty() && !id.contains('/') && gio_unix::DesktopAppInfo::new(&format!("{id}.desktop")).is_some()
}

/// Something happens when it's clicked.
fn clickable(n: &Note) -> bool {
    n.exec_argv.is_some()
        || (n.open() && n.app_actions.iter().any(|a| a.key == "default"))
        || has_desktop(&n.desktop_entry)
        || has_desktop(&n.app_icon)
}

/// Put text on the clipboard (a verification code).
fn copy(text: &str) {
    if let Some(d) = gdk::Display::default() {
        d.clipboard().set_text(text);
    }
}

/// A code goes through Nyx Clipboard when it's there: marked sensitive (kept out of
/// clipboard histories) and cleared again after a little while. Otherwise a plain copy.
fn copy_code(code: &str) {
    let code = code.to_string();
    glib::spawn_future_local(async move {
        let sent = match gio::bus_get_future(gio::BusType::Session).await {
            Ok(bus) => bus
                .call_future(
                    Some("io.github.design_nexus.nyx.Clipboard1"),
                    "/io/github/design_nexus/nyx/Clipboard1",
                    "io.github.design_nexus.nyx.Clipboard.Otp1",
                    "CopyCode",
                    Some(&(code.as_str(),).to_variant()),
                    None,
                    gio::DBusCallFlags::NO_AUTO_START,
                    3000,
                )
                .await
                .is_ok(),
            Err(_) => false,
        };
        if !sent {
            copy(&code);
        }
    });
}

pub fn card(n: &Note, place: Place, ctx: &Ctx, now: f64) -> gtk::Widget {
    let overlay = gtk::Overlay::new();
    overlay.add_css_class("nc-card");
    set_class(&overlay, "unread", !n.read && place == Place::Inbox);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    // A picture sent with it (an avatar, a cover) stands in for the app's icon.
    let ai: gtk::Widget = if !n.image.is_empty() && std::path::Path::new(&n.image).is_file() {
        let pic = gtk::Image::from_file(&n.image);
        pic.set_pixel_size(30);
        let frame = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        frame.set_overflow(gtk::Overflow::Hidden);
        frame.add_css_class("nc-picture-frame");
        frame.set_halign(gtk::Align::Start);
        frame.set_valign(gtk::Align::Start);
        frame.append(&pic);
        frame.upcast()
    } else {
        app_icon(&n.app_icon, &n.glyph, 30)
    };
    ai.set_valign(gtk::Align::Start);
    ai.add_css_class("nc-app-icon");
    row.append(&ai);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 1);
    body.set_hexpand(true);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 5);
    top.add_css_class("nc-top");
    let app = label(&app_name(&n.app), "nc-app");
    app.set_hexpand(false);
    app.set_max_width_chars(28);
    top.append(&app);
    if n.pinned && place != Place::Pinned {
        let p = icon("view-pin-symbolic", 12);
        p.add_css_class("nc-pinmark");
        top.append(&p);
    }
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    top.append(&spacer);
    let when = gtk::Label::new(Some(&if place == Place::Popup { "now".to_string() } else { ui::when(n.ts, now) }));
    when.add_css_class("nc-when");
    top.append(&when);
    body.append(&top);

    let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    if !n.read && place == Place::Inbox {
        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dot.add_css_class("nc-dot");
        dot.set_valign(gtk::Align::Center);
        title_row.append(&dot);
    }
    let title = label("", "nc-title");
    title.set_markup(&marked(if n.summary.is_empty() { &n.app } else { &n.summary }, &ctx.highlight));
    title_row.append(&title);
    body.append(&title_row);
    if !n.body.is_empty() && !ctx.hide_text {
        let text = label("", "nc-text");
        // One paragraph, so the three-line limit holds (Pango counts it per paragraph).
        let flat = n.body.split_whitespace().collect::<Vec<_>>().join(" ");
        text.set_markup(&marked(&flat, &ctx.highlight));
        text.set_wrap(true);
        text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        text.set_lines(3);
        text.set_ellipsize(gtk::pango::EllipsizeMode::End);
        body.append(&text);
    }
    // Buttons: a code to copy, the app's own (while it's open), Omarchy's commands.
    let acts = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    acts.add_css_class("nc-actions");
    if !ctx.hide_text {
        for code in n.codes.iter().take(2) {
            let b = gtk::Button::new();
            b.add_css_class("nc-action");
            b.add_css_class("nc-code");
            let inner = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            inner.append(&icon("edit-copy-symbolic", 13));
            inner.append(&gtk::Label::new(Some(code)));
            b.set_child(Some(&inner));
            b.set_tooltip_text(Some("Copy the code"));
            let code = code.clone();
            b.connect_clicked(move |b| {
                copy_code(&code);
                b.set_tooltip_text(Some("Copied"));
            });
            acts.append(&b);
        }
    }
    if n.open() {
        for a in n.app_actions.iter().filter(|a| a.key != "default" && !a.label.trim().is_empty()).take(3) {
            let b = gtk::Button::with_label(&a.label);
            b.add_css_class("nc-action");
            let (id, key, hide) = (n.id, a.key.clone(), ctx.hide.clone());
            b.connect_clicked(move |_| {
                let key = key.clone();
                act(async move { client::invoke_action(id, &key).await.map(|_| ()) });
                hide();
            });
            acts.append(&b);
        }
    }
    for (i, a) in n.actions.iter().enumerate().take(3) {
        let b = gtk::Button::with_label(&a.label);
        b.add_css_class("nc-action");
        let (id, hide) = (n.id, ctx.hide.clone());
        b.connect_clicked(move |_| {
            act(async move { client::run_action(id, i as u32).await.map(|_| ()) });
            hide();
        });
        acts.append(&b);
    }
    if acts.first_child().is_some() {
        body.append(&acts);
    }
    row.append(&body);
    overlay.set_child(Some(&row));

    // Hover: pin, and dismiss (Inbox, pop-ups) or delete (Pinned, History).
    let hover = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    hover.add_css_class("nc-hover");
    hover.set_halign(gtk::Align::End);
    hover.set_valign(gtk::Align::Start);
    let pin = icon_button("view-pin-symbolic", if n.pinned { "Unpin" } else { "Pin" });
    set_class(&pin, "on", n.pinned);
    let (id, pinned) = (n.id, n.pinned);
    pin.connect_clicked(move |_| act(client::set_pinned(id, !pinned)));
    hover.append(&pin);
    let remove = match place {
        Place::Popup => {
            let b = icon_button("window-close-symbolic", "Dismiss");
            b.connect_clicked(move |_| act(client::popup_ended(id, "dismissed")));
            b
        }
        Place::Inbox => {
            let b = icon_button("window-close-symbolic", "Dismiss");
            b.connect_clicked(move |_| act(client::dismiss(vec![id])));
            b
        }
        _ => {
            let b = icon_button("user-trash-symbolic", if pinned { "Delete (also unpins)" } else { "Delete" });
            b.connect_clicked(move |_| act(client::delete(vec![id])));
            b
        }
    };
    hover.append(&remove);
    hover.set_visible(false);
    overlay.add_overlay(&hover);
    let motion = gtk::EventControllerMotion::new();
    let (h, w, o) = (hover.clone(), when.clone(), overlay.clone());
    motion.connect_enter(move |_, _, _| {
        h.set_visible(true);
        w.set_opacity(0.0);
        o.add_css_class("hover");
    });
    let (h, w, o) = (hover.clone(), when.clone(), overlay.clone());
    motion.connect_leave(move |_| {
        h.set_visible(false);
        w.set_opacity(1.0);
        o.remove_css_class("hover");
    });
    overlay.add_controller(motion);

    // Click: the app's own action, Omarchy's command, or open the app; then it's dealt
    // with (nyx-notifyd does all of that).
    let click = gtk::GestureClick::new();
    let (id, hide) = (n.id, ctx.hide.clone());
    click.connect_released(move |g, _, _, _| {
        if g.current_button() != 1 {
            return;
        }
        let hide = hide.clone();
        glib::MainContext::default().spawn_local(async move {
            match client::activate(id).await {
                Ok(true) => hide(),
                Ok(false) => {}
                Err(e) => eprintln!("nyx-notification-center: {e}"),
            }
        });
    });
    row.add_controller(click);
    set_class(&overlay, "clickable", clickable(n));
    overlay.upcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_words_and_escapes() {
        assert_eq!(marked("a < b", &[]), "a &lt; b");
        let m = marked("Hinge & hinge", &["HINGE".into()]);
        assert_eq!(m.matches("<span").count(), 2);
        assert!(m.contains("&amp;"));
        assert_eq!(app_name("omarchy-action"), "Omarchy");
    }
}
