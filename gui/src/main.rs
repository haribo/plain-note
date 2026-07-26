//! Plain Note — GTK4 + libadwaita desktop client.
//!
//! Local editing over the same on-disk store as `pn`: a folder tree + note list
//! sidebar with search, and a Markdown editor with tags. Live auto-sync is a
//! follow-up. Reuses `plain-note-client` for persistence.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use note_core::{FolderId, NoteId, NoteStore, ROOT_FOLDER};
use plain_note_client::store::{self, LocalStore};
use plain_note_client::{config, remote};

const APP_ID: &str = "dev.plainnote.PlainNote";

struct State {
    store: LocalStore,
    doc: NoteStore,
    note_ids: Vec<NoteId>,
    folder_ids: Vec<Option<String>>, // parallel to folder rows; None = "all notes"
    current: Option<NoteId>,
    filter: Option<String>, // folder id filter; None = all
    query: String,
    loading: bool,
    last_sig: String, // signature of the last-rendered content, to skip no-op reloads
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
    folder_list: gtk::ListBox,
    note_list: gtk::ListBox,
    title: gtk::Entry,
    text_view: gtk::TextView,
    buffer: gtk::TextBuffer,
    tags_box: gtk::Box,
    subtitle: adw::WindowTitle,
}

enum SyncMsg {
    Done,
    Error,
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
        note_ids: Vec::new(),
        folder_ids: Vec::new(),
        current: None,
        filter: None,
        query: String::new(),
        loading: false,
        last_sig: String::new(),
    }));

    // --- Sidebar ---
    let new_btn = gtk::Button::from_icon_name("list-add-symbolic");
    new_btn.add_css_class("flat");
    new_btn.set_tooltip_text(Some("Nouvelle note"));

    let sidebar_header = adw::HeaderBar::new();
    sidebar_header.set_title_widget(Some(&adw::WindowTitle::new("Plain Note", "")));
    sidebar_header.pack_start(&new_btn);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Rechercher"));
    search.set_margin_top(8);
    search.set_margin_start(8);
    search.set_margin_end(8);

    let folder_list = gtk::ListBox::new();
    folder_list.set_selection_mode(gtk::SelectionMode::Single);
    folder_list.add_css_class("navigation-sidebar");

    let new_folder = gtk::Entry::builder()
        .placeholder_text("Nouveau dossier…")
        .build();
    new_folder.set_margin_start(8);
    new_folder.set_margin_end(8);
    new_folder.set_margin_bottom(4);

    let note_list = gtk::ListBox::new();
    note_list.set_selection_mode(gtk::SelectionMode::Single);
    note_list.add_css_class("navigation-sidebar");
    let note_scroll = gtk::ScrolledWindow::builder()
        .child(&note_list)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();

    let sidebar_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    sidebar_box.append(&search);
    sidebar_box.append(&section_label("Dossiers"));
    sidebar_box.append(&folder_list);
    sidebar_box.append(&new_folder);
    sidebar_box.append(&section_label("Notes"));
    sidebar_box.append(&note_scroll);

    let sidebar = adw::ToolbarView::new();
    sidebar.add_top_bar(&sidebar_header);
    sidebar.set_content(Some(&sidebar_box));

    // --- Editor ---
    let subtitle = adw::WindowTitle::new("Plain Note", "");
    let content_header = adw::HeaderBar::new();
    content_header.set_title_widget(Some(&subtitle));

    let title = gtk::Entry::builder().placeholder_text("Titre").build();
    title.add_css_class("pn-title");
    title.set_margin_top(14);
    title.set_margin_start(18);
    title.set_margin_end(18);

    let tags_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let tag_entry = gtk::Entry::builder()
        .placeholder_text("+ tag")
        .max_width_chars(10)
        .build();
    tag_entry.add_css_class("flat");
    let tags_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    tags_row.set_margin_start(18);
    tags_row.set_margin_end(18);
    tags_row.set_margin_top(8);
    tags_row.append(&tags_box);
    tags_row.append(&tag_entry);
    let tags_scroll = gtk::ScrolledWindow::builder()
        .child(&tags_row)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .build();

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
    editor.append(&tags_scroll);
    editor.append(&text_scroll);

    let content = adw::ToolbarView::new();
    content.add_top_bar(&content_header);
    content.set_content(Some(&editor));

    let split = adw::OverlaySplitView::new();
    split.set_sidebar(Some(&sidebar));
    split.set_content(Some(&content));
    split.set_min_sidebar_width(280.0);
    split.set_max_sidebar_width(360.0);

    let ui = Ui {
        folder_list: folder_list.clone(),
        note_list: note_list.clone(),
        title: title.clone(),
        text_view: text_view.clone(),
        buffer: buffer.clone(),
        tags_box,
        subtitle,
    };

    rebuild_folders(&ui, &state);
    rebuild_notes(&ui, &state);

    // Live auto-sync (only if the device is enrolled).
    let sync_label = gtk::Label::new(None);
    sync_label.add_css_class("dim-label");
    sync_label.add_css_class("caption");
    content_header.pack_end(&sync_label);
    start_auto_sync(&ui, &state, &sync_label);

    // Folder selection -> filter.
    {
        let ui = ui.clone();
        let state = state.clone();
        folder_list.connect_row_selected(move |_, row| {
            if let Some(row) = row {
                let idx = row.index() as usize;
                let filter = state.borrow().folder_ids.get(idx).cloned().flatten();
                state.borrow_mut().filter = filter;
                rebuild_notes(&ui, &state);
            }
        });
    }

    // Note selection -> load into editor.
    {
        let ui = ui.clone();
        let state = state.clone();
        note_list.connect_row_selected(move |_, row| {
            if let Some(row) = row {
                show_note(&ui, &state, row.index());
            }
        });
    }

    // Search -> refilter notes.
    {
        let ui = ui.clone();
        let state = state.clone();
        search.connect_search_changed(move |e| {
            state.borrow_mut().query = e.text().to_string();
            rebuild_notes(&ui, &state);
        });
    }

    // New note (in the selected folder, if any).
    {
        let ui = ui.clone();
        let state = state.clone();
        new_btn.connect_clicked(move |_| {
            let now = store::now_millis();
            let created = {
                let mut st = state.borrow_mut();
                match st.doc.create_note(now) {
                    Ok(id) => {
                        if let Some(f) = st.filter.clone() {
                            let _ = st.doc.move_note(&id, &f, now);
                        }
                        st.persist();
                        Some(id)
                    }
                    Err(_) => None,
                }
            };
            if created.is_some() {
                rebuild_notes(&ui, &state);
                if let Some(row) = ui.note_list.row_at_index(0) {
                    ui.note_list.select_row(Some(&row));
                }
                ui.title.grab_focus();
            }
        });
    }

    // New folder.
    {
        let ui = ui.clone();
        let state = state.clone();
        new_folder.connect_activate(move |entry| {
            let name = entry.text().to_string();
            if name.trim().is_empty() {
                return;
            }
            {
                let mut st = state.borrow_mut();
                if st
                    .doc
                    .create_folder(&name, ROOT_FOLDER, store::now_millis())
                    .is_ok()
                {
                    st.persist();
                }
            }
            entry.set_text("");
            rebuild_folders(&ui, &state);
        });
    }

    // Add a tag.
    {
        let ui = ui.clone();
        let state = state.clone();
        tag_entry.connect_activate(move |entry| {
            let tag = entry.text().to_string();
            let tag = tag.trim().trim_start_matches('#');
            let current = state.borrow().current.clone();
            if tag.is_empty() {
                return;
            }
            if let Some(id) = current {
                {
                    let mut st = state.borrow_mut();
                    let _ = st.doc.add_tag(&id, tag, store::now_millis());
                    st.persist();
                }
                entry.set_text("");
                rebuild_tags(&ui, &state, &id);
                rebuild_notes(&ui, &state);
            }
        });
    }

    // Title edits -> save.
    {
        let ui = ui.clone();
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
                {
                    let mut st = state.borrow_mut();
                    let _ = st.doc.set_title(&id, &entry.text(), store::now_millis());
                    st.persist();
                }
                refresh_note_row(&ui, &state, &id);
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

fn section_label(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("dim-label");
    l.add_css_class("caption-heading");
    l.set_halign(gtk::Align::Start);
    l.set_margin_start(12);
    l.set_margin_top(6);
    l
}

fn rebuild_folders(ui: &Ui, state: &Rc<RefCell<State>>) {
    while let Some(child) = ui.folder_list.first_child() {
        ui.folder_list.remove(&child);
    }
    let mut folder_ids: Vec<Option<String>> = vec![None];

    let all = adw::ActionRow::builder().title("Toutes les notes").build();
    ui.folder_list.append(&all);

    let folders = {
        let st = state.borrow();
        let mut fs: Vec<(String, String)> = st
            .doc
            .list_folders()
            .unwrap_or_default()
            .into_iter()
            .map(|f| {
                let path = st.doc.folder_path(f.id.as_str()).unwrap_or_default();
                (f.id.as_str().to_string(), path)
            })
            .collect();
        fs.sort_by(|a, b| a.1.cmp(&b.1));
        fs
    };

    for (id, path) in folders {
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&path).as_str())
            .build();
        let del = gtk::Button::from_icon_name("user-trash-symbolic");
        del.add_css_class("flat");
        del.set_valign(gtk::Align::Center);
        del.set_tooltip_text(Some("Supprimer le dossier"));
        {
            let ui = ui.clone();
            let state = state.clone();
            let fid = id.clone();
            del.connect_clicked(move |_| {
                {
                    let mut st = state.borrow_mut();
                    if st
                        .doc
                        .delete_folder(&FolderId::from(fid.clone()), store::now_millis())
                        .is_ok()
                    {
                        if st.filter.as_deref() == Some(fid.as_str()) {
                            st.filter = None;
                        }
                        st.persist();
                    }
                }
                rebuild_folders(&ui, &state);
                rebuild_notes(&ui, &state);
            });
        }
        row.add_suffix(&del);
        ui.folder_list.append(&row);
        folder_ids.push(Some(id));
    }

    state.borrow_mut().folder_ids = folder_ids;
}

fn rebuild_notes(ui: &Ui, state: &Rc<RefCell<State>>) {
    while let Some(child) = ui.note_list.first_child() {
        ui.note_list.remove(&child);
    }
    let (query, filter) = {
        let st = state.borrow();
        (st.query.clone(), st.filter.clone())
    };
    let mut notes = {
        let st = state.borrow();
        if query.trim().is_empty() {
            st.doc.list()
        } else {
            st.doc.search(&query)
        }
    }
    .unwrap_or_default();
    if let Some(f) = &filter {
        notes.retain(|n| &n.folder == f);
    }
    notes.sort_by_key(|n| std::cmp::Reverse(n.updated));
    state.borrow_mut().note_ids = notes.iter().map(|n| n.id.clone()).collect();

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
        ui.note_list.append(&row);
    }
}

fn refresh_note_row(ui: &Ui, state: &Rc<RefCell<State>>, id: &NoteId) {
    // Cheapest correct approach: rebuild the list, keeping the selection.
    let idx = state.borrow().note_ids.iter().position(|n| n == id);
    rebuild_notes(ui, state);
    if let Some(idx) = idx
        && let Some(row) = ui.note_list.row_at_index(idx as i32)
    {
        state.borrow_mut().loading = true;
        ui.note_list.select_row(Some(&row));
        state.borrow_mut().loading = false;
    }
}

fn show_note(ui: &Ui, state: &Rc<RefCell<State>>, idx: i32) {
    let note = {
        let st = state.borrow();
        st.note_ids
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
    let path = {
        let st = state.borrow();
        st.doc.folder_path(&note.folder).unwrap_or_default()
    };
    ui.subtitle.set_title(if note.title.is_empty() {
        "(sans titre)"
    } else {
        &note.title
    });
    ui.subtitle.set_subtitle(&path);
    state.borrow_mut().loading = false;
    rebuild_tags(ui, state, &note.id);
}

fn rebuild_tags(ui: &Ui, state: &Rc<RefCell<State>>, id: &NoteId) {
    while let Some(child) = ui.tags_box.first_child() {
        ui.tags_box.remove(&child);
    }
    let tags = {
        let st = state.borrow();
        st.doc
            .get_note(id)
            .ok()
            .flatten()
            .map(|n| n.tags)
            .unwrap_or_default()
    };
    for tag in tags {
        let chip = gtk::Button::with_label(&format!("#{tag}  ✕"));
        chip.add_css_class("flat");
        chip.add_css_class("pn-chip");
        let ui2 = ui.clone();
        let state = state.clone();
        let id = id.clone();
        let tag_name = tag.clone();
        chip.connect_clicked(move |_| {
            {
                let mut st = state.borrow_mut();
                let _ = st.doc.remove_tag(&id, &tag_name, store::now_millis());
                st.persist();
            }
            rebuild_tags(&ui2, &state, &id);
            rebuild_notes(&ui2, &state);
        });
        ui.tags_box.append(&chip);
    }
}

/// Start a background sync loop (own thread + Tokio runtime) if the device is
/// enrolled. Each sync result is delivered to the GTK main loop, which reloads
/// the store when its content actually changed.
fn start_auto_sync(ui: &Ui, state: &Rc<RefCell<State>>, label: &gtk::Label) {
    let cfg = config::config_path();
    if config::Settings::load_from(&cfg).is_err() {
        return; // not enrolled — the GUI stays local-only
    }

    let (tx, rx) = async_channel::unbounded::<SyncMsg>();
    std::thread::spawn(move || {
        let Ok(rt) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        rt.block_on(async move {
            let store = LocalStore::at_default();
            loop {
                let msg = match remote::sync(&cfg, &store).await {
                    Ok(_) => SyncMsg::Done,
                    Err(_) => SyncMsg::Error,
                };
                if tx.send(msg).await.is_err() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        });
    });

    let ui = ui.clone();
    let state = state.clone();
    let label = label.clone();
    glib::spawn_future_local(async move {
        while let Ok(msg) = rx.recv().await {
            match msg {
                SyncMsg::Done => {
                    label.set_text("synchronisé");
                    maybe_reload(&ui, &state);
                }
                SyncMsg::Error => label.set_text("hors ligne"),
            }
        }
    });
}

/// Reload the store from disk if its visible content changed, preserving the
/// user's selection and not interrupting active typing.
fn maybe_reload(ui: &Ui, state: &Rc<RefCell<State>>) {
    let Ok(doc) = state.borrow().store.load() else {
        return;
    };
    let sig = content_sig(&doc);
    if sig == state.borrow().last_sig {
        return;
    }
    let editing = ui.title.has_focus() || ui.text_view.has_focus();
    let current = state.borrow().current.clone();
    {
        let mut st = state.borrow_mut();
        st.doc = doc;
        st.last_sig = sig;
    }
    rebuild_folders(ui, state);
    rebuild_notes(ui, state);

    if !editing && let Some(id) = current {
        let idx = state.borrow().note_ids.iter().position(|n| n == &id);
        if let Some(idx) = idx
            && let Some(row) = ui.note_list.row_at_index(idx as i32)
        {
            ui.note_list.select_row(Some(&row));
        }
    }
}

/// A cheap signature of the store's visible content (folders + note metadata),
/// used to skip rebuilds when a sync changed nothing.
fn content_sig(doc: &NoteStore) -> String {
    let mut s = String::new();
    if let Ok(folders) = doc.list_folders() {
        for f in folders {
            s.push_str(f.id.as_str());
            s.push(':');
            s.push_str(&f.name);
            s.push('/');
            s.push_str(&f.parent);
            s.push(';');
        }
    }
    if let Ok(notes) = doc.list() {
        for n in notes {
            s.push_str(n.id.as_str());
            s.push(':');
            s.push_str(&n.title);
            s.push('@');
            s.push_str(&n.updated.to_string());
            s.push('#');
            s.push_str(&n.tags.join(","));
            s.push(';');
        }
    }
    s
}

fn install_css() {
    let css = gtk::CssProvider::new();
    css.load_from_data(
        ".pn-title { font-size: 1.4rem; font-weight: 800; } \
         .pn-title text { font-weight: 800; } \
         .pn-chip { padding: 2px 8px; min-height: 0; }",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
