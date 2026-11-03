//! The panel: a full-height layer-shell surface on the right edge of the focused screen,
//! with a catcher behind it that closes it on a click anywhere else. Three views (Inbox,
//! Pinned, History) and Settings. It stays in memory while hidden but listens to
//! nothing until it's opened.

use crate::card::{self, Ctx, Place};
use crate::choice::Choice;
use crate::client;
use crate::ui::{self, act, icon, icon_button, label, set_class};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use nyx_notify_store::{Note, Query, now};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

pub const WIDTH: i32 = 400;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Inbox,
    Pinned,
    History,
}

impl View {
    pub fn parse(s: &str) -> Option<View> {
        match s {
            "inbox" => Some(View::Inbox),
            "pinned" => Some(View::Pinned),
            "history" => Some(View::History),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            View::Inbox => "inbox",
            View::Pinned => "pinned",
            View::History => "history",
        }
    }
}

pub struct Panel {
    window: gtk::Window,
    catcher: gtk::Window,
    /// "main" (the views) or "settings".
    content: gtk::Stack,
    views: gtk::Stack,
    tabs: Vec<(View, gtk::ToggleButton)>,
    view: Cell<View>,
    only: Option<String>,
    inbox: gtk::Box,
    pinned: gtk::Box,
    history: gtk::Box,
    search: gtk::SearchEntry,
    app_filter: Choice,
    days_filter: Choice,
    /// The app names behind `app_filter`'s entries (after "All apps").
    filter_apps: RefCell<Vec<String>>,
    settings_box: gtk::Box,
    dnd: gtk::Button,
    clear: gtk::Button,
    expanded: RefCell<HashSet<String>>,
    /// While open: the service's Changed signal and Omarchy's quiet.json.
    listening: RefCell<Option<(Option<gio::SignalSubscription>, Option<gio::FileMonitor>)>>,
    /// A redraw is queued.
    pending: Cell<bool>,
    me: RefCell<Weak<Panel>>,
    panel1: RefCell<Option<nyx_desktop::panel::Panel>>,
    popups: RefCell<Option<Rc<crate::popups::Popups>>>,
}

fn scrolled(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let s = gtk::ScrolledWindow::new();
    s.set_hscrollbar_policy(gtk::PolicyType::Never);
    s.set_vexpand(true);
    s.set_child(Some(child));
    s
}

fn clear(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}

fn empty(icon_name: &str, text: &str) -> gtk::Widget {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 8);
    b.add_css_class("nc-empty");
    b.set_vexpand(true);
    b.set_valign(gtk::Align::Center);
    let i = icon(icon_name, 30);
    b.append(&i);
    let l = gtk::Label::new(Some(text));
    l.set_wrap(true);
    l.set_justify(gtk::Justification::Center);
    b.append(&l);
    b.upcast()
}

fn section(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(&text.to_uppercase()));
    l.add_css_class("nc-section");
    l.set_xalign(0.0);
    l
}

impl Panel {
    pub fn is_open(&self) -> bool {
        self.window.is_visible()
    }

    pub fn set_popups(&self, p: Rc<crate::popups::Popups>) {
        *self.popups.borrow_mut() = Some(p);
    }

    fn weak(&self) -> Weak<Panel> {
        self.me.borrow().clone()
    }

    fn ctx(&self) -> Ctx {
        let w = self.weak();
        let words: Vec<String> = if self.view.get() == View::History {
            self.search.text().split_whitespace().map(String::from).collect()
        } else {
            Vec::new()
        };
        Ctx {
            hide: Rc::new(move || {
                if let Some(p) = w.upgrade() {
                    p.hide();
                }
            }),
            highlight: Rc::new(words),
            hide_text: false,
        }
    }

    pub fn show(&self) {
        if self.is_open() {
            return;
        }
        if let Some(m) = self.target_monitor() {
            self.window.set_monitor(Some(&m));
            self.catcher.set_monitor(Some(&m));
        }
        self.content.set_visible_child_name("main");
        // Pop-ups make way: everything they showed is in here.
        if let Some(p) = self.popups.borrow().as_ref() {
            p.clear();
        }
        self.catcher.present();
        self.window.present();
        if let Some(p) = self.panel1.borrow().as_ref() {
            p.announce();
        }
        self.listen();
        // For screenshots: open a view, or settings, straight away.
        match std::env::var("NYX_NC_TEST_VIEW").as_deref() {
            Ok("settings") => self.open_settings(),
            Ok(v) => {
                if let Some(v) = View::parse(v) {
                    self.set_view(v);
                }
            }
            Err(_) => {}
        }
        self.refresh();
    }

    pub fn hide(&self) {
        if !self.is_open() {
            return;
        }
        self.window.set_visible(false);
        self.catcher.set_visible(false);
        self.listening.borrow_mut().take();
        // What was on screen has been seen.
        act(client::mark_all_read());
        self.expanded.borrow_mut().clear();
        self.content.set_visible_child_name("main");
    }

    pub fn toggle(&self) {
        if self.is_open() { self.hide() } else { self.show() }
    }

    /// Open on a view; close if it's already showing.
    pub fn toggle_view(&self, v: View) {
        if self.is_open() && self.view.get() == v && self.content.visible_child_name().as_deref() == Some("main") {
            self.hide();
            return;
        }
        self.set_view(v);
        self.show();
        self.content.set_visible_child_name("main");
    }

    fn set_view(&self, v: View) {
        self.view.set(v);
        self.views.set_visible_child_name(v.name());
        for (tv, b) in &self.tabs {
            if b.is_active() != (*tv == v) {
                b.set_active(*tv == v);
            }
        }
        self.clear.set_visible(v == View::Inbox);
        if self.is_open() {
            self.refresh();
        }
    }

    fn listen(&self) {
        let w = self.weak();
        let sub = client::on_changed(move || {
            if let Some(p) = w.upgrade() {
                p.refresh();
            }
        });
        let w = self.weak();
        let monitor = gio::File::for_path(client::omarchy_dir().join("quiet.json"))
            .monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
            .ok();
        if let Some(m) = &monitor {
            m.connect_changed(move |_, _, _, _| {
                if let Some(p) = w.upgrade() {
                    p.draw_dnd();
                }
            });
        }
        *self.listening.borrow_mut() = Some((sub, monitor));
        self.draw_dnd();
    }

    fn draw_dnd(&self) {
        let on = client::dnd();
        set_class(&self.dnd, "on", on);
        self.dnd.set_icon_name(if on { "notifications-disabled-symbolic" } else { "weather-clear-night-symbolic" });
        self.dnd.set_tooltip_text(Some(if on { "Do not disturb is on" } else { "Do not disturb" }));
    }

    /// Redraw the current view (once, however many changes arrive together).
    pub fn refresh(&self) {
        if self.pending.replace(true) {
            return;
        }
        let w = self.weak();
        glib::MainContext::default().spawn_local(async move {
            let Some(p) = w.upgrade() else { return };
            p.pending.set(false);
            match p.view.get() {
                View::Inbox => p.draw_inbox().await,
                View::Pinned => p.draw_pinned().await,
                View::History => p.draw_history().await,
            }
        });
    }

    fn problem(&self, into: &gtk::Box, e: String) {
        clear(into);
        into.append(&empty("dialog-warning-symbolic", &e));
    }

    async fn draw_inbox(&self) {
        let (inbox, pinned) = (client::inbox().await, client::pinned().await);
        let inbox = match inbox {
            Ok(i) => i,
            Err(e) => return self.problem(&self.inbox, e),
        };
        let pinned = pinned.unwrap_or_default();
        let b = &self.inbox;
        clear(b);
        let ctx = self.ctx();
        let t = now();
        if !pinned.is_empty() {
            let head = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            head.add_css_class("nc-section-row");
            let s = section("Pinned");
            s.set_hexpand(true);
            head.append(&s);
            let more = icon_button("go-next-symbolic", "All pinned");
            let w = self.weak();
            more.connect_clicked(move |_| {
                if let Some(p) = w.upgrade() {
                    p.set_view(View::Pinned);
                }
            });
            head.append(&more);
            b.append(&head);
            for n in pinned.iter().take(2) {
                b.append(&card::card(n, Place::Pinned, &ctx, t));
            }
        }
        if inbox.is_empty() {
            b.append(&empty("preferences-system-notifications-symbolic", "No new notifications"));
            return;
        }
        if !pinned.is_empty() {
            b.append(&section("Notifications"));
        }
        // Groups by app, the most recent group first.
        let mut order: Vec<String> = Vec::new();
        for n in &inbox {
            if !order.contains(&n.app) {
                order.push(n.app.clone());
            }
        }
        for app in order {
            let group: Vec<&Note> = inbox.iter().filter(|n| n.app == app).collect();
            if group.len() == 1 {
                b.append(&card::card(group[0], Place::Inbox, &ctx, t));
            } else if self.expanded.borrow().contains(&app) {
                b.append(&self.group_head(&app, group[0], &group));
                let stack = gtk::Box::new(gtk::Orientation::Vertical, 6);
                for n in &group {
                    stack.append(&card::card(n, Place::Inbox, &ctx, t));
                }
                b.append(&stack);
            } else {
                b.append(&self.collapsed(&app, &group, &ctx, t));
            }
        }
    }

    fn group_head(&self, app: &str, first: &Note, group: &[&Note]) -> gtk::Widget {
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        head.add_css_class("nc-group-head");
        head.append(&card::app_icon(&first.app_icon, &first.glyph, 18));
        head.append(&label(&card::app_name(app), "nc-group-name"));
        let clear_group = icon_button("edit-clear-all-symbolic", "Clear these");
        let ids: Vec<i64> = group.iter().map(|n| n.id).collect();
        clear_group.connect_clicked(move |_| act(client::dismiss(ids.clone())));
        head.append(&clear_group);
        let fold = icon_button("pan-up-symbolic", "Show less");
        let (w, a) = (self.weak(), app.to_string());
        fold.connect_clicked(move |_| {
            if let Some(p) = w.upgrade() {
                p.expanded.borrow_mut().remove(&a);
                p.refresh();
            }
        });
        head.append(&fold);
        head.upcast()
    }

    /// A group folded up: its newest card with the edges of the others under it.
    fn collapsed(&self, app: &str, group: &[&Note], ctx: &Ctx, t: f64) -> gtk::Widget {
        let wrap = gtk::Box::new(gtk::Orientation::Vertical, 0);
        wrap.add_css_class("nc-stack");
        let top = gtk::Overlay::new();
        top.set_child(Some(&card::card(group[0], Place::Inbox, ctx, t)));
        let count = gtk::Label::new(Some(&group.len().to_string()));
        count.add_css_class("nc-count");
        count.set_halign(gtk::Align::End);
        count.set_valign(gtk::Align::End);
        top.add_overlay(&count);
        wrap.append(&top);
        for class in ["nc-edge1", "nc-edge2"] {
            let e = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            e.add_css_class(class);
            e.set_halign(gtk::Align::Fill);
            wrap.append(&e);
        }
        wrap.set_tooltip_text(Some(&format!("{} notifications from {}", group.len(), card::app_name(app))));
        // Any click opens the group (before the card would act on it).
        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let (w, a) = (self.weak(), app.to_string());
        click.connect_pressed(move |g, _, _, _| {
            g.set_state(gtk::EventSequenceState::Claimed);
            if let Some(p) = w.upgrade() {
                p.expanded.borrow_mut().insert(a.clone());
                p.refresh();
            }
        });
        wrap.add_controller(click);
        wrap.upcast()
    }

    async fn draw_pinned(&self) {
        let pinned = match client::pinned().await {
            Ok(p) => p,
            Err(e) => return self.problem(&self.pinned, e),
        };
        clear(&self.pinned);
        if pinned.is_empty() {
            self.pinned.append(&empty("view-pin-symbolic", "Pin a notification to keep it here"));
            return;
        }
        let (ctx, t) = (self.ctx(), now());
        for n in &pinned {
            self.pinned.append(&card::card(n, Place::Pinned, &ctx, t));
        }
    }

    async fn draw_history(&self) {
        // The app filter's entries, kept as they are when nothing new has appeared.
        if let Ok(apps) = client::apps().await {
            let names: Vec<String> = apps.iter().filter(|a| a.count > 0).map(|a| a.app.clone()).collect();
            if *self.filter_apps.borrow() != names {
                let chosen = self.chosen_app();
                let mut items = vec!["All apps".to_string()];
                items.extend(names.iter().map(|n| card::app_name(n)));
                *self.filter_apps.borrow_mut() = names.clone();
                let at = chosen.and_then(|c| names.iter().position(|n| *n == c)).map_or(0, |i| i + 1);
                self.app_filter.set_items(&items, at);
            }
        }
        let q = Query {
            text: self.search.text().to_string(),
            app: self.chosen_app().unwrap_or_default(),
            days: [0, 1, 7, 30][self.days_filter.selected().min(3)],
            limit: 300,
        };
        let notes = match client::history(&q).await {
            Ok(n) => n,
            Err(e) => return self.problem(&self.history, e),
        };
        clear(&self.history);
        if notes.is_empty() {
            let text = if q.text.is_empty() && q.app.is_empty() && q.days == 0 { "No history yet" } else { "Nothing matches" };
            self.history.append(&empty("edit-find-symbolic", text));
            return;
        }
        let (ctx, t) = (self.ctx(), now());
        let mut last: Option<f64> = None;
        for n in &notes {
            if last.is_none_or(|l| !ui::same_day(l, n.ts)) {
                self.history.append(&section(&ui::day(n.ts, t)));
                last = Some(n.ts);
            }
            self.history.append(&card::card(n, Place::History, &ctx, t));
        }
    }

    fn chosen_app(&self) -> Option<String> {
        let i = self.app_filter.selected();
        if i == 0 {
            return None;
        }
        self.filter_apps.borrow().get(i - 1).cloned()
    }

    fn open_settings(&self) {
        self.content.set_visible_child_name("settings");
        let w = self.weak();
        glib::MainContext::default().spawn_local(async move {
            if let Some(p) = w.upgrade() {
                p.draw_settings().await;
            }
        });
    }

    async fn draw_settings(&self) {
        let b = &self.settings_box;
        let (settings, apps) = (client::settings().await, client::apps().await);
        let settings = match settings {
            Ok(s) => s,
            Err(e) => return self.problem(b, e),
        };
        clear(b);
        let state = Rc::new(RefCell::new(settings));
        let save = {
            let state = state.clone();
            Rc::new(move || {
                let s = state.borrow().clone();
                act(async move { client::set_settings(&s).await.map(|_| ()) });
            })
        };

        // Do not disturb.
        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("nc-settings-card");
        let dnd = gtk::Switch::new();
        dnd.set_active(client::dnd());
        dnd.connect_state_set(|_, on| {
            if client::dnd() != on {
                client::toggle_dnd();
            }
            glib::Propagation::Proceed
        });
        card.append(&option("Do not disturb", "Pop-ups stay quiet; everything still arrives here", &dnd));
        let sched = gtk::Switch::new();
        sched.set_active(state.borrow().schedule.enabled);
        let times = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        times.add_css_class("nc-times");
        let from = time_choice(state.borrow().schedule.from);
        let to = time_choice(state.borrow().schedule.to);
        times.append(&from.button);
        times.append(&gtk::Label::new(Some("to")));
        times.append(&to.button);
        times.set_sensitive(state.borrow().schedule.enabled);
        let sched_row = option("On a schedule", "", &sched);
        if let Some(text) = sched_row.first_child().and_then(|t| t.downcast::<gtk::Box>().ok()) {
            if let Some(sub) = text.last_child() {
                text.remove(&sub);
            }
            text.append(&times);
        }
        card.append(&sched_row);
        {
            let (state, save, times) = (state.clone(), save.clone(), times.clone());
            sched.connect_state_set(move |_, on| {
                state.borrow_mut().schedule.enabled = on;
                times.set_sensitive(on);
                save();
                glib::Propagation::Proceed
            });
        }
        for (dd, is_from) in [(from, true), (to, false)] {
            let (state, save) = (state.clone(), save.clone());
            dd.connect_changed(move |i| {
                let minute = (i * 30) as u16;
                let mut s = state.borrow_mut();
                if is_from {
                    s.schedule.from = minute
                } else {
                    s.schedule.to = minute
                }
                drop(s);
                save();
            });
        }
        b.append(&card);

        // History.
        b.append(&section("History"));
        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("nc-settings-card");
        let seg = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        seg.add_css_class("nc-segmented");
        seg.set_valign(gtk::Align::Center);
        let mut first: Option<gtk::ToggleButton> = None;
        for days in [7u32, 30, 90] {
            let t = gtk::ToggleButton::with_label(&format!("{days} d"));
            t.set_tooltip_text(Some(&format!("Keep {days} days")));
            t.set_active(state.borrow().retention_days == days);
            if let Some(f) = &first {
                t.set_group(Some(f));
            } else {
                first = Some(t.clone());
            }
            let (state, save) = (state.clone(), save.clone());
            t.connect_toggled(move |t| {
                if t.is_active() && state.borrow().retention_days != days {
                    state.borrow_mut().retention_days = days;
                    save();
                }
            });
            seg.append(&t);
        }
        card.append(&option("Keep for", "Pinned notifications are always kept", &seg));
        let del = icon_button("user-trash-symbolic", "Delete history");
        let confirm = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        let yes = icon_button("object-select-symbolic", "Delete everything except pinned");
        yes.add_css_class("danger");
        let no = icon_button("window-close-symbolic", "Keep it");
        confirm.append(&yes);
        confirm.append(&no);
        confirm.set_visible(false);
        let ends = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        ends.append(&del);
        ends.append(&confirm);
        {
            let (d, c) = (del.clone(), confirm.clone());
            del.connect_clicked(move |_| {
                d.set_visible(false);
                c.set_visible(true);
            });
            let (d, c) = (del.clone(), confirm.clone());
            no.connect_clicked(move |_| {
                c.set_visible(false);
                d.set_visible(true);
            });
            let (d, c) = (del.clone(), confirm.clone());
            yes.connect_clicked(move |_| {
                c.set_visible(false);
                d.set_visible(true);
                act(client::delete_history());
            });
        }
        card.append(&option("Delete history", "Everything except pinned", &ends));
        let keep = gtk::Switch::new();
        keep.set_active(state.borrow().keep_codes);
        {
            let (state, save) = (state.clone(), save.clone());
            keep.connect_state_set(move |_, on| {
                state.borrow_mut().keep_codes = on;
                save();
                glib::Propagation::Proceed
            });
        }
        card.append(&option("Keep verification codes", "Off: codes in history are hidden once ten minutes old", &keep));
        b.append(&card);

        // Pop-ups: only when Nyx shows them (Omarchy has its own otherwise).
        let nyx = client::mode().await.as_deref() == Ok("nyx");
        if nyx {
            b.append(&section("Pop-ups"));
            let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
            card.add_css_class("nc-settings-card");
            let text = gtk::Switch::new();
            text.set_active(!state.borrow().hide_popup_text);
            {
                let (state, save) = (state.clone(), save.clone());
                text.connect_state_set(move |_, on| {
                    state.borrow_mut().hide_popup_text = !on;
                    save();
                    glib::Propagation::Proceed
                });
            }
            card.append(&option("Show text in pop-ups", "Off: only the app and the title", &text));
            b.append(&card);
        }

        // Apps: pop-ups (when Nyx shows them) and history, each its own switch.
        let apps = apps.unwrap_or_default();
        if !apps.is_empty() {
            let head = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            let title = section("Apps");
            title.set_hexpand(true);
            head.append(&title);
            for (name, shown) in [("Pop-ups", nyx), ("History", true)] {
                let l = section(name);
                l.add_css_class("nc-column");
                l.set_visible(shown);
                head.append(&l);
            }
            head.add_css_class("nc-apps-head");
            b.append(&head);
            let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
            card.add_css_class("nc-settings-card");
            for a in apps {
                let name = card::app_name(&a.app);
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
                row.add_css_class("nc-app-row");
                row.append(&card::app_icon(&a.app_icon, "", 20));
                row.append(&label(&name, "nc-row-title"));
                // Pop-ups: off means it arrives quietly, straight into the Inbox.
                let pop = gtk::Switch::new();
                pop.set_valign(gtk::Align::Center);
                pop.set_active(!state.borrow().no_popups.contains(&a.app));
                pop.set_tooltip_text(Some(&format!("Show pop-ups for {name}")));
                pop.set_visible(nyx);
                {
                    let (state, save, app) = (state.clone(), save.clone(), a.app.clone());
                    pop.connect_state_set(move |_, on| {
                        let mut st = state.borrow_mut();
                        st.no_popups.retain(|x| *x != app);
                        if !on {
                            st.no_popups.push(app.clone());
                        }
                        drop(st);
                        save();
                        glib::Propagation::Proceed
                    });
                }
                row.append(&pop);
                let s = gtk::Switch::new();
                s.set_valign(gtk::Align::Center);
                s.set_active(!a.excluded);
                s.set_tooltip_text(Some(&format!("Keep {name}'s notifications in history")));
                let (state, save, app) = (state.clone(), save.clone(), a.app.clone());
                s.connect_state_set(move |_, on| {
                    let mut st = state.borrow_mut();
                    st.excluded.retain(|x| *x != app);
                    if !on {
                        st.excluded.push(app.clone());
                    }
                    drop(st);
                    save();
                    glib::Propagation::Proceed
                });
                row.append(&s);
                card.append(&row);
            }
            b.append(&card);
        }
    }

    /// The focused screen (or `--only`'s).
    fn target_monitor(&self) -> Option<gdk::Monitor> {
        focused_monitor(self.only.as_deref())
    }
}

/// The focused screen, or the one named.
pub fn focused_monitor(only: Option<&str>) -> Option<gdk::Monitor> {
    {
        let name = match only {
            Some(o) => o.to_string(),
            None => nyx_desktop::hypr::Instance::find()
                .ok()
                .and_then(|i| i.json::<Vec<serde_json::Value>>("monitors").ok())
                .and_then(|ms| ms.into_iter().find(|m| m["focused"].as_bool() == Some(true)))
                .and_then(|m| m["name"].as_str().map(str::to_string))?,
        };
        let display = gdk::Display::default()?;
        display.monitors().iter::<gdk::Monitor>().flatten().find(|m| m.connector().as_deref() == Some(name.as_str()))
    }
}

/// A settings row: title and explanation on the left, a control on the right.
fn option(title: &str, sub: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("nc-option");
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.append(&label(title, "nc-row-title"));
    let s = label(sub, "nc-row-sub");
    s.set_ellipsize(gtk::pango::EllipsizeMode::None);
    s.set_wrap(true);
    s.set_visible(!sub.is_empty());
    text.append(&s);
    row.append(&text);
    control.set_valign(gtk::Align::Center);
    row.append(control);
    row
}

/// A time of day, in half hours.
fn time_choice(minute: u16) -> Choice {
    let times: Vec<String> = (0..48).map(|i| format!("{:02}:{:02}", i / 2, (i % 2) * 30)).collect();
    let c = Choice::new(&times, usize::from(minute / 30).min(47), 5);
    c.button.add_css_class("nc-time");
    c
}

pub fn build(app: &gtk::Application, only: Option<String>) -> Rc<Panel> {
    let window = gtk::Window::new();
    window.set_application(Some(app));
    window.add_css_class("nyx-notification-center");
    window.init_layer_shell();
    window.set_namespace(Some("nyx-notification-center"));
    window.set_layer(Layer::Overlay);
    // Keys (Esc, typing a search) when clicked. NYX_NC_NO_KEYBOARD keeps it out of
    // keyboard focus altogether, for trying it on a test output while someone types.
    window.set_keyboard_mode(if std::env::var_os("NYX_NC_NO_KEYBOARD").is_some() {
        KeyboardMode::None
    } else {
        KeyboardMode::OnDemand
    });
    for e in [Edge::Top, Edge::Bottom, Edge::Right] {
        window.set_anchor(e, true);
    }
    window.set_exclusive_zone(0);
    window.set_default_size(WIDTH, -1);

    // Behind the panel on its own layer and over the bar and dock, so a click anywhere
    // else closes it. Its whole surface takes input (a clear one can otherwise get none).
    let catcher = gtk::Window::new();
    catcher.set_application(Some(app));
    catcher.add_css_class("nc-catcher");
    catcher.init_layer_shell();
    catcher.set_namespace(Some("nyx-notification-center-catcher"));
    catcher.set_layer(Layer::Overlay);
    catcher.set_keyboard_mode(KeyboardMode::None);
    for e in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        catcher.set_anchor(e, true);
    }
    catcher.set_exclusive_zone(-1);
    catcher.set_child(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
    catcher.connect_realize(|w| {
        if let Some(surface) = w.surface() {
            let all = gtk::cairo::Region::create_rectangle(&gtk::cairo::RectangleInt::new(0, 0, 100_000, 100_000));
            surface.set_input_region(Some(&all));
        }
    });

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("nc-panel");
    root.set_size_request(WIDTH, -1);
    window.set_child(Some(&root));

    let content = gtk::Stack::new();
    content.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    content.set_transition_duration(200);
    content.set_vexpand(true);
    root.append(&content);

    // Main: header, then the views.
    let main = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    head.add_css_class("nc-header");
    let seg = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    seg.add_css_class("nc-segmented");
    seg.add_css_class("nc-tabs");
    let mut tabs = Vec::new();
    let mut group: Option<gtk::ToggleButton> = None;
    for (v, name, tip) in [
        (View::Inbox, "preferences-system-notifications-symbolic", "Inbox"),
        (View::Pinned, "view-pin-symbolic", "Pinned"),
        (View::History, "document-open-recent-symbolic", "History"),
    ] {
        let t = gtk::ToggleButton::new();
        t.set_icon_name(name);
        t.set_tooltip_text(Some(tip));
        if let Some(g) = &group {
            t.set_group(Some(g));
        } else {
            group = Some(t.clone());
        }
        seg.append(&t);
        tabs.push((v, t));
    }
    head.append(&seg);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    head.append(&spacer);
    let dnd = icon_button("weather-clear-night-symbolic", "Do not disturb");
    dnd.connect_clicked(|_| client::toggle_dnd());
    head.append(&dnd);
    let settings_button = icon_button("emblem-system-symbolic", "Notification settings");
    head.append(&settings_button);
    let clear_button = icon_button("edit-clear-all-symbolic", "Clear all (pinned ones stay)");
    clear_button.connect_clicked(|_| act(client::dismiss_all()));
    head.append(&clear_button);
    main.append(&head);

    let views = gtk::Stack::new();
    views.set_transition_type(gtk::StackTransitionType::Crossfade);
    views.set_transition_duration(120);
    views.set_vexpand(true);
    let list = || {
        let b = gtk::Box::new(gtk::Orientation::Vertical, 8);
        b.add_css_class("nc-list");
        b
    };
    let (inbox, pinned, history) = (list(), list(), list());
    views.add_named(&scrolled(&inbox), Some("inbox"));
    views.add_named(&scrolled(&pinned), Some("pinned"));
    let hist_page = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search notifications"));
    search.add_css_class("nc-search");
    hist_page.append(&search);
    let filters = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    filters.add_css_class("nc-filters");
    let app_filter = Choice::new(&["All apps".to_string()], 0, 16);
    let days: Vec<String> = ["Any time", "Today", "Last 7 days", "Last 30 days"].map(String::from).to_vec();
    let days_filter = Choice::new(&days, 0, 12);
    filters.append(&app_filter.button);
    filters.append(&days_filter.button);
    hist_page.append(&filters);
    hist_page.append(&scrolled(&history));
    views.add_named(&hist_page, Some("history"));
    main.append(&views);
    content.add_named(&main, Some("main"));

    // Settings: a detail view.
    let settings_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let shead = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    shead.add_css_class("nc-detail-head");
    let back = icon_button("go-previous-symbolic", "Back");
    shead.append(&back);
    shead.append(&label("Notifications", "nc-detail-title"));
    settings_page.append(&shead);
    let settings_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    settings_box.add_css_class("nc-settings");
    settings_page.append(&scrolled(&settings_box));
    content.add_named(&settings_page, Some("settings"));

    let panel = Rc::new(Panel {
        window: window.clone(),
        catcher: catcher.clone(),
        content: content.clone(),
        views,
        tabs,
        view: Cell::new(View::Inbox),
        only,
        inbox,
        pinned,
        history,
        search: search.clone(),
        app_filter: app_filter.clone(),
        days_filter: days_filter.clone(),
        filter_apps: RefCell::default(),
        settings_box,
        dnd,
        clear: clear_button,
        expanded: RefCell::default(),
        listening: RefCell::default(),
        pending: Cell::new(false),
        me: RefCell::default(),
        panel1: RefCell::default(),
        popups: RefCell::default(),
    });
    *panel.me.borrow_mut() = Rc::downgrade(&panel);
    let weak = Rc::downgrade(&panel);

    for (v, t) in &panel.tabs {
        let (w, v) = (weak.clone(), *v);
        t.connect_toggled(move |t| {
            if t.is_active()
                && let Some(p) = w.upgrade()
                && p.view.get() != v
            {
                p.set_view(v);
            }
        });
    }
    panel.set_view(View::Inbox);
    let w = weak.clone();
    settings_button.connect_clicked(move |_| {
        if let Some(p) = w.upgrade() {
            p.open_settings();
        }
    });
    let w = weak.clone();
    back.connect_clicked(move |_| {
        if let Some(p) = w.upgrade() {
            p.content.set_visible_child_name("main");
            p.refresh();
        }
    });
    let w = weak.clone();
    search.connect_search_changed(move |_| {
        if let Some(p) = w.upgrade() {
            p.refresh();
        }
    });
    let w = weak.clone();
    app_filter.connect_changed(move |_| {
        if let Some(p) = w.upgrade() {
            p.refresh();
        }
    });
    let w = weak.clone();
    days_filter.connect_changed(move |_| {
        if let Some(p) = w.upgrade() {
            p.refresh();
        }
    });

    // Esc: back from settings, else close.
    let keys = gtk::EventControllerKey::new();
    let w = weak.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key != gdk::Key::Escape {
            return glib::Propagation::Proceed;
        }
        if let Some(p) = w.upgrade() {
            if p.content.visible_child_name().as_deref() == Some("settings") {
                p.content.set_visible_child_name("main");
                p.refresh();
            } else {
                p.hide();
            }
        }
        glib::Propagation::Stop
    });
    window.add_controller(keys);

    let click = gtk::GestureClick::new();
    let w = weak.clone();
    click.connect_pressed(move |_, _, _, _| {
        if let Some(p) = w.upgrade() {
            p.hide();
        }
    });
    catcher.add_controller(click);

    // Never with the Control Center: opening one closes the other.
    if let Some(conn) = app.dbus_connection() {
        let (w1, w2, w3) = (weak.clone(), weak.clone(), weak.clone());
        match nyx_desktop::panel::Panel::serve(
            &conn,
            "notification-center",
            nyx_desktop::panel::Handlers {
                show: Box::new(move || {
                    if let Some(p) = w1.upgrade() {
                        p.show();
                    }
                }),
                hide: Box::new(move || {
                    if let Some(p) = w2.upgrade() {
                        p.hide();
                    }
                }),
                is_open: Box::new(move || w3.upgrade().is_some_and(|p| p.is_open())),
            },
        ) {
            Ok(p) => *panel.panel1.borrow_mut() = Some(p),
            Err(e) => eprintln!("nyx-notification-center: panel coordination: {e}"),
        }
    }

    window.set_visible(false);
    catcher.set_visible(false);
    panel
}
