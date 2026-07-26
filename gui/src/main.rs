//! Plain Note — GTK4 + libadwaita desktop client.
//!
//! v1: local editing (note list + Markdown editor + auto-save) over the same
//! on-disk store as `pn`. Folder tree, tags UI, search, and live auto-sync are
//! follow-ups (the latter needs a shared client library so config/sync are not
//! duplicated).

mod store;

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use note_core::{NoteId, NoteStore};

use crate::store::LocalStore;

const APP_ID: &str = "dev.plainnote.PlainNote";

struct State {
    store: LocalStore,
    doc: NoteStore,
    ids: Vec<NoteId>,
    current: Option<NoteId>,
    loading: bool,
}

impl State {
    fn persist(&mut self) {
        if let Err(e) = self.store.save(&mut self.doc) {
            eprintln!("plain-note-gui: save failed: {e}");
        }
    }
}

#[derive(Clone)]
struct Ui {
    list: gtk::ListBox,
    title: gtk::Entry,
    buffer: gtk::TextBuffer,
    subtitle: adw::WindowTitle,
}

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &adw::Application) {
    install_css();

    let store = LocalStore::at_default();
    let doc = store.load().unwrap_or_else(|e| {
        eprintln!("plain-note-gui: could not load store ({e}); starting empty");
        NoteStore::new()
    });
    let state = Rc::new(RefCell::new(State {
        store,
        doc,
        ids: Vec::new(),
        current: None,
        loading: false,
    }));

    // --- Sidebar: header + note list ---
    let new_btn = gtk::Button::from_icon_name("list-add-symbolic");
    new_btn.add_css_class("flat");
    new_btn.set_tooltip_text(Some("Nouvelle note"));

    let sidebar_header = adw::HeaderBar::new();
    sidebar_header.set_title_widget(Some(&adw::WindowTitle::new("Notes", "")));
    sidebar_header.pack_start(&new_btn);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.add_css_class("navigation-sidebar");
    let list_scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();

    let sidebar = adw::ToolbarView::new();
    sidebar.add_top_bar(&sidebar_header);
    sidebar.set_content(Some(&list_scroll));

    // --- Content: header + editor ---
    let subtitle = adw::WindowTitle::new("Plain Note", "");
    let content_header = adw::HeaderBar::new();
    content_header.set_title_widget(Some(&subtitle));

    let title = gtk::Entry::builder().placeholder_text("Titre").build();
    title.add_css_class("pn-title");
    title.set_margin_top(14);
    title.set_margin_start(18);
    title.set_margin_end(18);

    let text_view = gtk::TextView::new();
    text_view.set_monospace(true);
    text_view.set_wrap_mode(gtk::WrapMode::WordChar);
    text_view.set_left_margin(18);
    text_view.set_right_margin(18);
    text_view.set_top_margin(10);
    text_view.set_bottom_margin(18);
    let buffer = text_view.buffer();
    let text_scroll = gtk::ScrolledWindow::builder()
        .child(&text_view)
        .vexpand(true)
        .build();

    let editor = gtk::Box::new(gtk::Orientation::Vertical, 0);
    editor.append(&title);
    editor.append(&text_scroll);

    let content = adw::ToolbarView::new();
    content.add_top_bar(&content_header);
    content.set_content(Some(&editor));

    // --- Split view ---
    let split = adw::OverlaySplitView::new();
    split.set_sidebar(Some(&sidebar));
    split.set_content(Some(&content));
    split.set_min_sidebar_width(280.0);
    split.set_max_sidebar_width(360.0);

    let ui = Ui {
        list: list.clone(),
        title: title.clone(),
        buffer: buffer.clone(),
        subtitle,
    };

    rebuild_list(&ui, &state);

    // Row selection -> load note into the editor.
    {
        let ui = ui.clone();
        let state = state.clone();
        list.connect_row_selected(move |_, row| {
            if let Some(row) = row {
                show_note(&ui, &state, row.index());
            }
        });
    }

    // New note.
    {
        let ui = ui.clone();
        let state = state.clone();
        new_btn.connect_clicked(move |_| {
            let now = store::now_millis();
            let created = {
                let mut st = state.borrow_mut();
                let id = st.doc.create_note(now);
                if id.is_ok() {
                    st.persist();
                }
                id.ok()
            };
            if created.is_some() {
                rebuild_list(&ui, &state);
                if let Some(row) = ui.list.row_at_index(0) {
                    ui.list.select_row(Some(&row));
                }
                ui.title.grab_focus();
            }
        });
    }

    // Title edits -> save.
    {
        let state = state.clone();
        title.connect_changed(move |entry| {
            let (loading, current) = {
                let st = state.borrow();
                (st.loading, st.current.clone())
            };
            if loading {
                return;
            }
            if let Some(id) = current {
                let text = entry.text().to_string();
                let mut st = state.borrow_mut();
                let _ = st.doc.set_title(&id, &text, store::now_millis());
                st.persist();
            }
        });
    }

    // Body edits -> save.
    {
        let state = state.clone();
        buffer.connect_changed(move |buf| {
            let (loading, current) = {
                let st = state.borrow();
                (st.loading, st.current.clone())
            };
            if loading {
                return;
            }
            if let Some(id) = current {
                let text = buf
                    .text(&buf.start_iter(), &buf.end_iter(), false)
                    .to_string();
                let mut st = state.borrow_mut();
                let _ = st.doc.replace_text(&id, &text, store::now_millis());
                st.persist();
            }
        });
    }

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .default_width(1000)
        .default_height(640)
        .width_request(360)
        .build();
    window.set_title(Some("Plain Note"));
    window.set_content(Some(&split));
    window.present();
}

/// Rebuild the sidebar list from the store, newest first.
fn rebuild_list(ui: &Ui, state: &Rc<RefCell<State>>) {
    while let Some(child) = ui.list.first_child() {
        ui.list.remove(&child);
    }
    let mut notes = {
        let st = state.borrow();
        st.doc.list().unwrap_or_default()
    };
    notes.sort_by_key(|n| std::cmp::Reverse(n.updated));
    state.borrow_mut().ids = notes.iter().map(|n| n.id.clone()).collect();

    for n in &notes {
        let title = if n.title.is_empty() {
            "(sans titre)".to_string()
        } else {
            n.title.clone()
        };
        let subtitle = if n.tags.is_empty() {
            String::new()
        } else {
            format!("#{}", n.tags.join(" #"))
        };
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&title).as_str())
            .subtitle(subtitle.as_str())
            .build();
        ui.list.append(&row);
    }
}

/// Load the note at `idx` into the editor.
fn show_note(ui: &Ui, state: &Rc<RefCell<State>>, idx: i32) {
    let note = {
        let st = state.borrow();
        st.ids
            .get(idx as usize)
            .cloned()
            .and_then(|id| st.doc.get_note(&id).ok().flatten())
    };
    let Some(note) = note else {
        return;
    };
    {
        let mut st = state.borrow_mut();
        st.loading = true;
        st.current = Some(note.id.clone());
    }
    ui.title.set_text(&note.title);
    ui.buffer.set_text(&note.text);
    let heading = if note.title.is_empty() {
        "(sans titre)"
    } else {
        &note.title
    };
    ui.subtitle.set_title(heading);
    state.borrow_mut().loading = false;
}

fn install_css() {
    let css = gtk::CssProvider::new();
    css.load_from_data(
        ".pn-title { font-size: 1.4rem; font-weight: 800; } \
         .pn-title text { font-weight: 800; }",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
