//! A compact picker: a button showing the choice (shortened to fit) with the list in a
//! popover. GTK's drop-down asks for the width of its longest entry, which a 400 px
//! panel can't give when the entries are app names.

use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

type OnChange = Rc<RefCell<Option<Box<dyn Fn(usize)>>>>;

#[derive(Clone)]
pub struct Choice {
    pub button: gtk::MenuButton,
    label: gtk::Label,
    list: gtk::ListBox,
    items: Rc<RefCell<Vec<String>>>,
    selected: Rc<Cell<usize>>,
    on_change: OnChange,
}

impl Choice {
    pub fn new(items: &[String], selected: usize, max_chars: i32) -> Choice {
        let button = gtk::MenuButton::new();
        button.add_css_class("nc-choice");
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let label = gtk::Label::new(None);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(max_chars);
        label.set_xalign(0.0);
        row.append(&label);
        row.append(&gtk::Image::from_icon_name("pan-down-symbolic"));
        button.set_child(Some(&row));
        let pop = gtk::Popover::new();
        pop.add_css_class("nc-popover");
        let list = gtk::ListBox::new();
        list.add_css_class("nc-choice-list");
        list.set_selection_mode(gtk::SelectionMode::None);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_propagate_natural_height(true);
        scroll.set_max_content_height(320);
        scroll.set_child(Some(&list));
        pop.set_child(Some(&scroll));
        button.set_popover(Some(&pop));
        let c = Choice {
            button,
            label,
            list,
            items: Rc::default(),
            selected: Rc::new(Cell::new(0)),
            on_change: Rc::default(),
        };
        c.set_items(items, selected);
        let me = c.clone();
        c.list.connect_row_activated(move |_, row| {
            let i = row.index().max(0) as usize;
            me.button.popdown();
            if i != me.selected.get() {
                me.set_selected(i);
                if let Some(f) = me.on_change.borrow().as_ref() {
                    f(i);
                }
            }
        });
        c
    }

    pub fn set_items(&self, items: &[String], selected: usize) {
        while let Some(r) = self.list.first_child() {
            self.list.remove(&r);
        }
        for it in items {
            let l = gtk::Label::new(Some(it));
            l.set_xalign(0.0);
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            l.set_max_width_chars(32);
            l.add_css_class("nc-choice-item");
            self.list.append(&l);
        }
        *self.items.borrow_mut() = items.to_vec();
        self.set_selected(selected.min(items.len().saturating_sub(1)));
    }

    pub fn set_selected(&self, i: usize) {
        self.selected.set(i);
        let text = self.items.borrow().get(i).cloned().unwrap_or_default();
        self.label.set_text(&text);
        self.button.set_tooltip_text(Some(&text));
    }

    pub fn selected(&self) -> usize {
        self.selected.get()
    }

    /// Called with the new index when the user picks something else.
    pub fn connect_changed(&self, f: impl Fn(usize) + 'static) {
        *self.on_change.borrow_mut() = Some(Box::new(f));
    }
}
