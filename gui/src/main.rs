//! Plain Note — GTK4 + libadwaita desktop client.
//!
//! A single sidebar tree of folders and notes (notes without a folder sit at the
//! root) plus a Markdown editor with tags. Local editing over the same on-disk
//! store as `pn`, with background live auto-sync. Reuses `plain-note-client`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use note_core::{NoteId, NoteStore, ROOT_FOLDER};
use plain_note_client::store::{self, LocalStore};
use plain_note_client::{config, remote};

const APP_ID: &str = "dev.plainnote.PlainNote";

#[derive(Clone)]
enum RowKind {
    Folder(String),
    Note(NoteId),
    Trash,
}

struct State {
    store: LocalStore,
    doc: NoteStore,
    rows: Vec<RowKind>,         // parallel to sidebar rows
    expanded: HashSet<String>,  // expanded folder ids
    sel_folder: Option<String>, // context folder for new note/folder
    current: Option<NoteId>,
    query: String,
    trash_view: bool,
    loading: bool,
    last_sig: String,
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
    tree: gtk::ListBox,
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
        rows: Vec::new(),
        expanded: HashSet::new(),
        sel_folder: None,
        current: None,
        query: String::new(),
        trash_view: false,
        loading: false,
        last_sig: String::new(),
    }));

    // --- Sidebar ---
    let new_btn = gtk::Button::from_icon_name("list-add-symbolic");
    new_btn.add_css_class("flat");
    new_btn.set_tooltip_text(Some("Nouvelle note"));

    let new_folder_btn = gtk::Button::from_icon_name("folder-new-symbolic");
    new_folder_btn.add_css_class("flat");
    new_folder_btn.set_tooltip_text(Some("Nouveau dossier"));

    let sidebar_header = adw::HeaderBar::new();
    sidebar_header.set_title_widget(Some(&adw::WindowTitle::new("Plain Note", "")));
    sidebar_header.pack_start(&new_btn);
    sidebar_header.pack_end(&new_folder_btn);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Rechercher"));
    search.set_margin_top(8);
    search.set_margin_start(8);
    search.set_margin_end(8);

    let tree = gtk::ListBox::new();
    tree.set_selection_mode(gtk::SelectionMode::Single);
    tree.add_css_class("navigation-sidebar");
    let tree_scroll = gtk::ScrolledWindow::builder()
        .child(&tree)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();

    let sidebar_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    sidebar_box.append(&search);
    sidebar_box.append(&tree_scroll);

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
        tree: tree.clone(),
        title: title.clone(),
        text_view: text_view.clone(),
        buffer: buffer.clone(),
        tags_box,
        subtitle,
    };

    rebuild_tree(&ui, &state);

    let sync_label = gtk::Label::new(None);
    sync_label.add_css_class("dim-label");
    sync_label.add_css_class("caption");
    content_header.pack_end(&sync_label);
    start_auto_sync(&ui, &state, &sync_label);

    // Row selection: folders toggle, notes open.
    {
        let ui = ui.clone();
        let state = state.clone();
        tree.connect_row_selected(move |_, row| {
            let Some(row) = row else { return };
            let kind = state.borrow().rows.get(row.index() as usize).cloned();
            match kind {
                Some(RowKind::Folder(id)) => {
                    {
                        let mut st = state.borrow_mut();
                        if !st.expanded.remove(&id) {
                            st.expanded.insert(id.clone());
                        }
                        st.sel_folder = Some(id);
                    }
                    rebuild_tree(&ui, &state);
                    reselect_current(&ui, &state);
                }
                Some(RowKind::Note(id)) => {
                    let folder = state
                        .borrow()
                        .doc
                        .get_note(&id)
                        .ok()
                        .flatten()
                        .map(|n| n.folder);
                    state.borrow_mut().sel_folder = folder;
                    show_note_by_id(&ui, &state, &id);
                }
                Some(RowKind::Trash) => {
                    {
                        let mut st = state.borrow_mut();
                        st.trash_view = !st.trash_view;
                    }
                    rebuild_tree(&ui, &state);
                }
                None => {}
            }
        });
    }

    // Search.
    {
        let ui = ui.clone();
        let state = state.clone();
        search.connect_search_changed(move |e| {
            state.borrow_mut().query = e.text().to_string();
            rebuild_tree(&ui, &state);
        });
    }

    // New note (into the selected folder, if any).
    {
        let ui = ui.clone();
        let state = state.clone();
        new_btn.connect_clicked(move |_| {
            let now = store::now_millis();
            let created = {
                let mut st = state.borrow_mut();
                match st.doc.create_note(now) {
                    Ok(id) => {
                        if let Some(f) = st.sel_folder.clone() {
                            let _ = st.doc.move_note(&id, &f, now);
                        }
                        st.persist();
                        Some(id)
                    }
                    Err(_) => None,
                }
            };
            if let Some(id) = created {
                rebuild_tree(&ui, &state);
                select_note(&ui, &state, &id);
                ui.title.grab_focus();
            }
        });
    }

    // New folder (dialog).
    {
        let ui = ui.clone();
        let state = state.clone();
        new_folder_btn.connect_clicked(move |btn| {
            let dialog = adw::AlertDialog::new(Some("Nouveau dossier"), None);
            let entry = gtk::Entry::builder()
                .placeholder_text("Nom du dossier")
                .activates_default(true)
                .build();
            dialog.set_extra_child(Some(&entry));
            dialog.add_response("cancel", "Annuler");
            dialog.add_response("create", "Créer");
            dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("create"));
            dialog.set_close_response("cancel");

            let ui = ui.clone();
            let state = state.clone();
            dialog.connect_response(None, move |_, resp| {
                if resp != "create" {
                    return;
                }
                let name = entry.text().to_string();
                if name.trim().is_empty() {
                    return;
                }
                {
                    let mut st = state.borrow_mut();
                    let parent = st
                        .sel_folder
                        .clone()
                        .unwrap_or_else(|| ROOT_FOLDER.to_string());
                    if st
                        .doc
                        .create_folder(&name, &parent, store::now_millis())
                        .is_ok()
                    {
                        st.persist();
                    }
                }
                rebuild_tree(&ui, &state);
            });
            dialog.present(Some(btn));
        });
    }

    // Add tag.
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
            }
        });
    }

    // Title edits -> save + live-update the selected sidebar row.
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
                set_selected_row_title(&ui, &entry.text());
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

/// Rebuild the sidebar: a folders+notes tree, or a flat match list when
/// searching. Expansion state (kept in `state`) is honored so edits/syncs never
/// collapse the tree.
fn rebuild_tree(ui: &Ui, state: &Rc<RefCell<State>>) {
    while let Some(child) = ui.tree.first_child() {
        ui.tree.remove(&child);
    }
    let mut rows: Vec<RowKind> = Vec::new();
    let (query, trash_view) = {
        let st = state.borrow();
        (st.query.trim().to_string(), st.trash_view)
    };

    if trash_view {
        let mut notes = state.borrow().doc.list_trashed().unwrap_or_default();
        notes.sort_by_key(|n| std::cmp::Reverse(n.updated));
        for n in &notes {
            ui.tree.append(&note_row(
                ui,
                state,
                &n.id,
                note_title(&n.title),
                0,
                false,
                true,
            ));
            rows.push(RowKind::Note(n.id.clone()));
        }
    } else if query.is_empty() {
        let (folders, notes) = {
            let st = state.borrow();
            (
                st.doc.list_folders().unwrap_or_default(),
                st.doc.list().unwrap_or_default(),
            )
        };
        let expanded = state.borrow().expanded.clone();
        walk(
            ui,
            state,
            &folders,
            &notes,
            ROOT_FOLDER,
            0,
            &expanded,
            &mut rows,
        );
    } else {
        let mut notes = state.borrow().doc.search(&query).unwrap_or_default();
        notes.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.updated.cmp(&a.updated)));
        for n in &notes {
            ui.tree.append(&note_row(
                ui,
                state,
                &n.id,
                note_title(&n.title),
                0,
                n.pinned,
                false,
            ));
            rows.push(RowKind::Note(n.id.clone()));
        }
    }

    let trash_count = state
        .borrow()
        .doc
        .list_trashed()
        .map(|v| v.len())
        .unwrap_or(0);
    ui.tree
        .append(&trash_row(ui, state, trash_count, trash_view));
    rows.push(RowKind::Trash);

    state.borrow_mut().rows = rows;
}

#[allow(clippy::too_many_arguments)]
fn walk(
    ui: &Ui,
    state: &Rc<RefCell<State>>,
    folders: &[note_core::FolderMeta],
    notes: &[note_core::NoteMeta],
    parent: &str,
    depth: usize,
    expanded: &HashSet<String>,
    rows: &mut Vec<RowKind>,
) {
    let mut subfolders: Vec<&note_core::FolderMeta> =
        folders.iter().filter(|f| f.parent == parent).collect();
    subfolders.sort_by(|a, b| a.name.cmp(&b.name));
    for f in subfolders {
        let is_expanded = expanded.contains(f.id.as_str());
        let count = notes.iter().filter(|n| n.folder == f.id.as_str()).count();
        ui.tree
            .append(&folder_row(depth, &f.name, is_expanded, count));
        rows.push(RowKind::Folder(f.id.as_str().to_string()));
        if is_expanded {
            walk(
                ui,
                state,
                folders,
                notes,
                f.id.as_str(),
                depth + 1,
                expanded,
                rows,
            );
        }
    }
    let mut child_notes: Vec<&note_core::NoteMeta> =
        notes.iter().filter(|n| n.folder == parent).collect();
    child_notes.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.updated.cmp(&a.updated)));
    for n in child_notes {
        ui.tree.append(&note_row(
            ui,
            state,
            &n.id,
            note_title(&n.title),
            depth,
            n.pinned,
            false,
        ));
        rows.push(RowKind::Note(n.id.clone()));
    }
}

fn note_title(t: &str) -> &str {
    if t.is_empty() { "(sans titre)" } else { t }
}

fn folder_row(depth: usize, name: &str, expanded: bool, count: usize) -> gtk::ListBoxRow {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    b.set_margin_start(8 + depth as i32 * 16);
    b.set_margin_end(8);
    b.set_margin_top(5);
    b.set_margin_bottom(5);
    let chevron = gtk::Image::from_icon_name(if expanded {
        "pan-down-symbolic"
    } else {
        "pan-end-symbolic"
    });
    let icon = gtk::Image::from_icon_name("folder-symbolic");
    let label = gtk::Label::new(Some(name));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_halign(gtk::Align::Start);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.add_css_class("pn-folder");
    let count = gtk::Label::new(Some(&count.to_string()));
    count.add_css_class("dim-label");
    count.add_css_class("caption");
    b.append(&chevron);
    b.append(&icon);
    b.append(&label);
    b.append(&count);
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&b));
    row
}

#[allow(clippy::too_many_arguments)]
fn note_row(
    ui: &Ui,
    state: &Rc<RefCell<State>>,
    id: &NoteId,
    title: &str,
    depth: usize,
    pinned: bool,
    trash: bool,
) -> gtk::ListBoxRow {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    b.set_margin_start(8 + depth as i32 * 16);
    b.set_margin_end(4);
    b.set_margin_top(3);
    b.set_margin_bottom(3);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_size_request(16, -1);
    let icon = gtk::Image::from_icon_name("text-x-generic-symbolic");
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_halign(gtk::Align::Start);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    b.append(&spacer);
    b.append(&icon);
    b.append(&label);
    if pinned {
        b.append(&gtk::Image::from_icon_name("view-pin-symbolic"));
    }
    b.append(&note_menu(ui, state, id, pinned, trash));
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&b));
    row
}

fn note_menu(
    ui: &Ui,
    state: &Rc<RefCell<State>>,
    id: &NoteId,
    pinned: bool,
    trash: bool,
) -> gtk::MenuButton {
    let mb = gtk::MenuButton::new();
    mb.set_icon_name("view-more-symbolic");
    mb.add_css_class("flat");
    mb.set_valign(gtk::Align::Center);
    let menu = gtk::Box::new(gtk::Orientation::Vertical, 2);
    menu.set_margin_top(4);
    menu.set_margin_bottom(4);
    menu.set_margin_start(4);
    menu.set_margin_end(4);
    let popover = gtk::Popover::new();
    popover.set_child(Some(&menu));
    mb.set_popover(Some(&popover));

    let item = |label: &str| {
        let btn = gtk::Button::with_label(label);
        btn.add_css_class("flat");
        if let Some(lbl) = btn.child().and_downcast::<gtk::Label>() {
            lbl.set_xalign(0.0);
        }
        btn
    };

    if trash {
        let restore = item("Restaurer");
        {
            let (ui, state, id, pop) = (ui.clone(), state.clone(), id.clone(), popover.clone());
            restore.connect_clicked(move |_| {
                pop.popdown();
                mutate(&state, |doc| doc.restore_note(&id, store::now_millis()));
                rebuild_tree(&ui, &state);
            });
        }
        let purge = item("Supprimer définitivement");
        purge.add_css_class("destructive-action");
        {
            let (ui, state, id, pop) = (ui.clone(), state.clone(), id.clone(), popover.clone());
            purge.connect_clicked(move |_| {
                pop.popdown();
                mutate(&state, |doc| doc.delete_note(&id));
                rebuild_tree(&ui, &state);
            });
        }
        menu.append(&restore);
        menu.append(&purge);
    } else {
        let pin = item(if pinned { "Désépingler" } else { "Épingler" });
        {
            let (ui, state, id, pop) = (ui.clone(), state.clone(), id.clone(), popover.clone());
            pin.connect_clicked(move |_| {
                pop.popdown();
                mutate(&state, |doc| {
                    doc.set_pinned(&id, !pinned, store::now_millis())
                });
                rebuild_tree(&ui, &state);
            });
        }
        let move_btn = item("Déplacer vers…");
        {
            let (ui, state, id) = (ui.clone(), state.clone(), id.clone());
            let pop = popover.clone();
            move_btn.connect_clicked(move |btn| {
                pop.popdown();
                open_move_dialog(&ui, &state, &id, btn);
            });
        }
        let trash_btn = item("Mettre à la corbeille");
        {
            let (ui, state, id, pop) = (ui.clone(), state.clone(), id.clone(), popover.clone());
            trash_btn.connect_clicked(move |_| {
                pop.popdown();
                mutate(&state, |doc| doc.trash_note(&id, store::now_millis()));
                rebuild_tree(&ui, &state);
            });
        }
        menu.append(&pin);
        menu.append(&move_btn);
        menu.append(&trash_btn);
    }
    mb
}

fn open_move_dialog(
    ui: &Ui,
    state: &Rc<RefCell<State>>,
    id: &NoteId,
    anchor: &impl IsA<gtk::Widget>,
) {
    // Destination list: root + every folder by path.
    let mut ids: Vec<String> = vec![ROOT_FOLDER.to_string()];
    let mut labels: Vec<String> = vec!["Racine".to_string()];
    {
        let st = state.borrow();
        let mut folders: Vec<(String, String)> = st
            .doc
            .list_folders()
            .unwrap_or_default()
            .into_iter()
            .map(|f| {
                let path = st.doc.folder_path(f.id.as_str()).unwrap_or_default();
                (f.id.as_str().to_string(), path)
            })
            .collect();
        folders.sort_by(|a, b| a.1.cmp(&b.1));
        for (fid, path) in folders {
            ids.push(fid);
            labels.push(path);
        }
    }
    let label_refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
    let dropdown = gtk::DropDown::from_strings(&label_refs);

    let dialog = adw::AlertDialog::new(Some("Déplacer vers"), None);
    dialog.set_extra_child(Some(&dropdown));
    dialog.add_response("cancel", "Annuler");
    dialog.add_response("move", "Déplacer");
    dialog.set_response_appearance("move", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("move"));
    dialog.set_close_response("cancel");

    let ui = ui.clone();
    let state = state.clone();
    let id = id.clone();
    dialog.connect_response(None, move |_, resp| {
        if resp != "move" {
            return;
        }
        let idx = dropdown.selected() as usize;
        if let Some(folder) = ids.get(idx) {
            let folder = folder.clone();
            mutate(&state, |doc| {
                doc.move_note(&id, &folder, store::now_millis())
            });
            rebuild_tree(&ui, &state);
        }
    });
    dialog.present(Some(anchor));
}

fn trash_row(ui: &Ui, state: &Rc<RefCell<State>>, count: usize, active: bool) -> gtk::ListBoxRow {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    b.set_margin_start(8);
    b.set_margin_end(4);
    b.set_margin_top(3);
    b.set_margin_bottom(3);
    let icon = gtk::Image::from_icon_name("user-trash-symbolic");
    let label = gtk::Label::new(Some("Corbeille"));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_halign(gtk::Align::Start);
    if active {
        label.add_css_class("accent");
    }
    let count_label = gtk::Label::new(Some(&count.to_string()));
    count_label.add_css_class("dim-label");
    count_label.add_css_class("caption");
    b.append(&icon);
    b.append(&label);
    b.append(&count_label);

    if count > 0 {
        let mb = gtk::MenuButton::new();
        mb.set_icon_name("view-more-symbolic");
        mb.add_css_class("flat");
        mb.set_valign(gtk::Align::Center);
        let menu = gtk::Box::new(gtk::Orientation::Vertical, 2);
        menu.set_margin_top(4);
        menu.set_margin_bottom(4);
        menu.set_margin_start(4);
        menu.set_margin_end(4);
        let popover = gtk::Popover::new();
        popover.set_child(Some(&menu));
        mb.set_popover(Some(&popover));
        let empty = gtk::Button::with_label("Vider la corbeille");
        empty.add_css_class("flat");
        empty.add_css_class("destructive-action");
        if let Some(lbl) = empty.child().and_downcast::<gtk::Label>() {
            lbl.set_xalign(0.0);
        }
        {
            let (ui, state, pop) = (ui.clone(), state.clone(), popover.clone());
            empty.connect_clicked(move |_| {
                pop.popdown();
                {
                    let mut st = state.borrow_mut();
                    let ids: Vec<NoteId> = st
                        .doc
                        .list_trashed()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|n| n.id)
                        .collect();
                    for id in &ids {
                        let _ = st.doc.delete_note(id);
                    }
                    st.persist();
                }
                rebuild_tree(&ui, &state);
            });
        }
        menu.append(&empty);
        b.append(&mb);
    }

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&b));
    row
}

/// Apply a mutation to the in-memory doc and persist it.
fn mutate(
    state: &Rc<RefCell<State>>,
    f: impl FnOnce(&mut NoteStore) -> Result<(), note_core::ModelError>,
) {
    let mut st = state.borrow_mut();
    let _ = f(&mut st.doc);
    st.persist();
}

/// Update the title label of the currently selected row without rebuilding
/// (so editing the title never disturbs the tree or the cursor).
fn set_selected_row_title(ui: &Ui, text: &str) {
    let Some(row) = ui.tree.selected_row() else {
        return;
    };
    let Some(b) = row.child().and_downcast::<gtk::Box>() else {
        return;
    };
    let mut child = b.first_child();
    while let Some(w) = child {
        if let Some(label) = w.downcast_ref::<gtk::Label>()
            && label.hexpands()
        {
            label.set_text(note_title(text));
            return;
        }
        child = w.next_sibling();
    }
}

fn select_note(ui: &Ui, state: &Rc<RefCell<State>>, id: &NoteId) {
    let idx = row_index_of(state, id);
    if let Some(idx) = idx
        && let Some(row) = ui.tree.row_at_index(idx as i32)
    {
        ui.tree.select_row(Some(&row));
    }
}

fn reselect_current(ui: &Ui, state: &Rc<RefCell<State>>) {
    let cur = state.borrow().current.clone();
    if let Some(id) = cur {
        let idx = row_index_of(state, &id);
        if let Some(idx) = idx
            && let Some(row) = ui.tree.row_at_index(idx as i32)
        {
            state.borrow_mut().loading = true;
            ui.tree.select_row(Some(&row));
            state.borrow_mut().loading = false;
        }
    }
}

fn row_index_of(state: &Rc<RefCell<State>>, id: &NoteId) -> Option<usize> {
    state
        .borrow()
        .rows
        .iter()
        .position(|r| matches!(r, RowKind::Note(n) if n == id))
}

fn show_note_by_id(ui: &Ui, state: &Rc<RefCell<State>>, id: &NoteId) {
    let note = state.borrow().doc.get_note(id).ok().flatten();
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
    let path = state
        .borrow()
        .doc
        .folder_path(&note.folder)
        .unwrap_or_default();
    ui.subtitle.set_title(note_title(&note.title));
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
        });
        ui.tags_box.append(&chip);
    }
}

fn start_auto_sync(ui: &Ui, state: &Rc<RefCell<State>>, label: &gtk::Label) {
    let cfg = config::config_path();
    if config::Settings::load_from(&cfg).is_err() {
        return; // not enrolled — local-only
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

fn maybe_reload(ui: &Ui, state: &Rc<RefCell<State>>) {
    let Ok(doc) = state.borrow().store.load() else {
        return;
    };
    let sig = content_sig(&doc);
    if sig == state.borrow().last_sig {
        return;
    }
    let editing = ui.title.has_focus() || ui.text_view.has_focus();
    {
        let mut st = state.borrow_mut();
        st.doc = doc;
        st.last_sig = sig;
    }
    rebuild_tree(ui, state);
    if !editing {
        reselect_current(ui, state);
    }
}

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
            s.push_str(&n.folder);
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
         .pn-folder { font-weight: 600; } \
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
