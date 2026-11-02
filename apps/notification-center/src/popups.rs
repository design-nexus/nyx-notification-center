//! Pop-ups (stage B): nyx-notifyd says when one should show (`Popup`) and when its
//! notification closed (`PopupClosed`); this draws them in the top-right corner of the
//! focused screen, newest on top. Each stays for its time (0: until dismissed), longer
//! while the pointer is on it; when it goes the notification stays in the Inbox.
//!
//! In Omarchy mode nothing is ever announced, so this shows nothing.

use crate::card::{self, Ctx, Place};
use crate::client;
use crate::ui::act;
use gtk::glib;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use nyx_notify_store::Note;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

const MAX: usize = 4;
const WIDTH: i32 = 380;

struct Toast {
    widget: gtk::Widget,
    timer: Option<glib::SourceId>,
    timeout_ms: u32,
}

pub struct Popups {
    window: gtk::Window,
    list: gtk::Box,
    toasts: RefCell<HashMap<i64, Toast>>,
    /// Newest last.
    order: RefCell<Vec<i64>>,
    only: Option<String>,
    _subs: RefCell<Vec<gtk::gio::SignalSubscription>>,
}

#[derive(serde::Deserialize)]
struct Payload {
    note: Note,
    #[serde(default)]
    timeout_ms: u32,
    #[serde(default)]
    hide_text: bool,
}

impl Popups {
    /// Take one down without telling anyone (its notification is already closed).
    fn remove(&self, id: i64) {
        if let Some(mut t) = self.toasts.borrow_mut().remove(&id) {
            if let Some(timer) = t.timer.take() {
                timer.remove();
            }
            self.list.remove(&t.widget);
        }
        self.order.borrow_mut().retain(|x| *x != id);
        if self.order.borrow().is_empty() {
            self.window.set_visible(false);
        }
    }

    /// Its time is up: it goes, the notification stays in the Inbox.
    fn expire(&self, id: i64) {
        self.remove(id);
        act(client::popup_ended(id, "timeout"));
    }

    /// Every pop-up goes (the Notification Center opened; they're all in it).
    pub fn clear(&self) {
        let ids: Vec<i64> = self.order.borrow().clone();
        for id in ids {
            self.expire(id);
        }
    }

    fn start_timer(self: &Rc<Self>, id: i64) {
        let ms = match self.toasts.borrow().get(&id) {
            Some(t) if t.timeout_ms > 0 => t.timeout_ms,
            _ => return,
        };
        let weak = Rc::downgrade(self);
        let timer = glib::timeout_add_local_once(Duration::from_millis(u64::from(ms)), move || {
            if let Some(p) = weak.upgrade() {
                if let Some(t) = p.toasts.borrow_mut().get_mut(&id) {
                    t.timer = None;
                }
                p.expire(id);
            }
        });
        if let Some(t) = self.toasts.borrow_mut().get_mut(&id) {
            if let Some(old) = t.timer.replace(timer) {
                old.remove();
            }
        }
    }

    fn show(self: &Rc<Self>, payload: &str, panel_open: bool) {
        let Ok(p) = serde_json::from_str::<Payload>(payload) else { return };
        let id = p.note.id;
        // The Notification Center is open: it shows the notification already.
        if panel_open {
            act(client::popup_ended(id, "timeout"));
            return;
        }
        let weak = Rc::downgrade(self);
        let ctx = Ctx { hide: Rc::new(|| {}), highlight: Rc::new(Vec::new()), hide_text: p.hide_text };
        let widget = card::card(&p.note, Place::Popup, &ctx, nyx_notify_store::now());
        widget.add_css_class("nc-popup");
        // The pointer on it holds it; leaving starts its time again.
        let motion = gtk::EventControllerMotion::new();
        let w = weak.clone();
        motion.connect_enter(move |_, _, _| {
            if let Some(p) = w.upgrade()
                && let Some(t) = p.toasts.borrow_mut().get_mut(&id)
                && let Some(timer) = t.timer.take()
            {
                timer.remove();
            }
        });
        let w = weak.clone();
        motion.connect_leave(move |_| {
            if let Some(p) = w.upgrade() {
                p.start_timer(id);
            }
        });
        widget.add_controller(motion);

        // Replaced: same place, new content. Otherwise on top.
        let replaced = self.toasts.borrow().contains_key(&id);
        if replaced {
            let old = self.toasts.borrow().get(&id).map(|t| t.widget.clone());
            if let Some(old) = old {
                self.list.insert_child_after(&widget, Some(&old));
                self.list.remove(&old);
            }
            if let Some(t) = self.toasts.borrow_mut().get_mut(&id) {
                t.widget = widget;
                t.timeout_ms = p.timeout_ms;
            }
        } else {
            self.list.prepend(&widget);
            self.toasts.borrow_mut().insert(id, Toast { widget, timer: None, timeout_ms: p.timeout_ms });
            self.order.borrow_mut().push(id);
        }
        self.start_timer(id);
        // Too many: the oldest goes first.
        while self.order.borrow().len() > MAX {
            let oldest = self.order.borrow()[0];
            self.expire(oldest);
        }
        if !self.window.is_visible() {
            if let Some(m) = crate::panel::focused_monitor(self.only.as_deref()) {
                self.window.set_monitor(Some(&m));
            }
            self.window.present();
        }
    }
}

/// Build the pop-up surface and start listening. `panel_open` says whether the
/// Notification Center is showing.
pub fn start(app: &gtk::Application, only: Option<String>, panel_open: Rc<dyn Fn() -> bool>) -> Rc<Popups> {
    let window = gtk::Window::new();
    window.set_application(Some(app));
    window.add_css_class("nyx-notification-center");
    window.add_css_class("nc-popups");
    window.init_layer_shell();
    window.set_namespace(Some("nyx-notification-popups"));
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Right, true);
    window.set_margin(Edge::Top, 8);
    window.set_margin(Edge::Right, 8);
    window.set_exclusive_zone(0);
    window.set_default_size(WIDTH, -1);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 8);
    list.set_size_request(WIDTH, -1);
    list.add_css_class("nc-popup-list");
    window.set_child(Some(&list));
    let p =
        Rc::new(Popups { window, list, toasts: RefCell::default(), order: RefCell::default(), only, _subs: RefCell::default() });
    let (w1, w2) = (Rc::downgrade(&p), Rc::downgrade(&p));
    let subs = client::on_popups(
        move |payload| {
            if let Some(p) = w1.upgrade() {
                p.show(&payload, panel_open());
            }
        },
        move |id| {
            if let Some(p) = w2.upgrade() {
                p.remove(id);
            }
        },
    );
    *p._subs.borrow_mut() = subs;
    p
}
