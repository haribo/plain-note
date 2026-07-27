//! Plain Note — GTK4 + libadwaita desktop client.
//!
//! A single sidebar tree of folders and notes (notes without a folder sit at the
//! root) plus a Markdown editor with tags. Local editing over the same on-disk
//! store as `pn`, with background live auto-sync. Reuses `plain-note-client`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use note_core::{FolderId, NoteId, NoteStore, ROOT_FOLDER};
use plain_note_client::store::{self, LocalStore};
use plain_note_client::{config, remote};

const APP_ID: &str = "dev.plainnote.PlainNote";

#[derive(Clone)]
enum RowKind {
    Folder(String),
    Note(NoteId),
    Trash,
}

/// An open editor tab, bound to one note. Each tab owns its own editor widgets
/// and auto-save wiring, so several notes can be edited at once.
#[derive(Clone)]
struct Tab {
    id: NoteId,
    page: adw::TabPage,
    title: gtk::Entry,
    text_view: gtk::TextView,
    buffer: gtk::TextBuffer,
    tags_box: gtk::Box,
    atts_box: gtk::Box,
}

struct State {
    store: LocalStore,
    doc: NoteStore,
    rows: Vec<RowKind>,         // parallel to sidebar rows
    expanded: HashSet<String>,  // expanded folder ids
    sel_folder: Option<String>, // context folder for new note/folder
    tabs: Vec<Tab>,             // open editor tabs
    current: Option<NoteId>,    // note of the active tab
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
    tab_view: adw::TabView,
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
        expanded: load_expanded(&gui_state_path()),
        sel_folder: None,
        tabs: Vec::new(),
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

    // --- Editor (tabbed) ---
    let subtitle = adw::WindowTitle::new("Plain Note", "");
    let content_header = adw::HeaderBar::new();
    content_header.set_title_widget(Some(&subtitle));

    let tab_view = adw::TabView::new();
    let tab_bar = adw::TabBar::builder()
        .view(&tab_view)
        .autohide(false)
        .build();

    let content = adw::ToolbarView::new();
    content.add_top_bar(&content_header);
    content.add_top_bar(&tab_bar);
    content.set_content(Some(&tab_view));

    let split = adw::OverlaySplitView::new();
    split.set_sidebar(Some(&sidebar));
    split.set_content(Some(&content));
    split.set_min_sidebar_width(280.0);
    split.set_max_sidebar_width(360.0);

    let ui = Ui {
        tree: tree.clone(),
        tab_view: tab_view.clone(),
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
                        save_expanded(&gui_state_path(), &st.expanded);
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
                    open_note(&ui, &state, &id);
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

    // Root drop zone: dropping a note on the tree background (anywhere not a
    // folder row) moves it to the top level.
    {
        let drop = gtk::DropTarget::new(glib::types::Type::STRING, gtk::gdk::DragAction::MOVE);
        let ui = ui.clone();
        let state = state.clone();
        drop.connect_drop(move |_, value, _, _| {
            let Ok(note_id) = value.get::<String>() else {
                return false;
            };
            mutate(&state, |doc| {
                doc.move_note(&NoteId::from(note_id), ROOT_FOLDER, store::now_millis())
            });
            rebuild_tree(&ui, &state);
            true
        });
        tree.add_controller(drop);
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
        new_btn.connect_clicked(move |_| create_note(&ui, &state));
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

    // Tab switch -> update active note, subtitle and sidebar selection.
    {
        let ui = ui.clone();
        let state = state.clone();
        tab_view.connect_selected_page_notify(move |tv| {
            let id = tv.selected_page().and_then(|page| {
                state
                    .borrow()
                    .tabs
                    .iter()
                    .find(|t| t.page == page)
                    .map(|t| t.id.clone())
            });
            state.borrow_mut().current = id.clone();
            if let Some(id) = id {
                sync_header(&ui, &state, &id);
                reselect_current(&ui, &state);
            } else {
                ui.subtitle.set_title("Plain Note");
                ui.subtitle.set_subtitle("");
            }
        });
    }

    // Tab closed -> drop it from state (the note itself is untouched).
    {
        let state = state.clone();
        tab_view.connect_close_page(move |_, page| {
            state.borrow_mut().tabs.retain(|t| &t.page != page);
            glib::Propagation::Proceed
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
    install_shortcuts(&window, &ui, &state, &search);
    window.present();
}

/// Create a note in the selected folder, open it in a tab, focus its title.
fn create_note(ui: &Ui, state: &Rc<RefCell<State>>) {
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
        rebuild_tree(ui, state);
        open_note(ui, state, &id);
        if let Some(tab) = state.borrow().tabs.iter().find(|t| t.id == id) {
            tab.title.grab_focus();
        }
    }
}

/// Window-level keyboard shortcuts.
fn install_shortcuts(
    window: &adw::ApplicationWindow,
    ui: &Ui,
    state: &Rc<RefCell<State>>,
    search: &gtk::SearchEntry,
) {
    let controller = gtk::ShortcutController::new();
    controller.set_scope(gtk::ShortcutScope::Global);

    let add = |accel: &str, callback: Box<dyn Fn() -> bool>| {
        let trigger = gtk::ShortcutTrigger::parse_string(accel);
        let action = gtk::CallbackAction::new(move |_, _| callback().into());
        controller.add_shortcut(gtk::Shortcut::new(trigger, Some(action)));
    };

    {
        let ui = ui.clone();
        let state = state.clone();
        add(
            "<Control>n",
            Box::new(move || {
                create_note(&ui, &state);
                true
            }),
        );
    }
    {
        let tab_view = ui.tab_view.clone();
        add(
            "<Control>w",
            Box::new(move || {
                if let Some(page) = tab_view.selected_page() {
                    tab_view.close_page(&page);
                }
                true
            }),
        );
    }
    {
        let search = search.clone();
        add(
            "<Control>f",
            Box::new(move || {
                search.grab_focus();
                true
            }),
        );
    }
    {
        let tab_view = ui.tab_view.clone();
        add(
            "<Control>Page_Down",
            Box::new(move || tab_view.select_next_page()),
        );
    }
    {
        let tab_view = ui.tab_view.clone();
        add(
            "<Control>Page_Up",
            Box::new(move || tab_view.select_previous_page()),
        );
    }

    // Formatting shortcuts act on the active tab's text view.
    let fmt = |accel: &str, op: fn(&gtk::TextBuffer)| {
        let state = state.clone();
        add(
            accel,
            Box::new(move || {
                if let Some(tv) = active_text_view(&state) {
                    op(&tv.buffer());
                    tv.grab_focus();
                }
                true
            }),
        );
    };
    fmt("<Control>b", |b| apply_wrap(b, "**"));
    fmt("<Control>i", |b| apply_wrap(b, "*"));
    fmt("<Control>e", |b| apply_wrap(b, "`"));
    fmt("<Control>k", apply_link);

    window.add_controller(controller);
}

/// The text view of the currently active tab, if any.
fn active_text_view(state: &Rc<RefCell<State>>) -> Option<gtk::TextView> {
    let cur = state.borrow().current.clone()?;
    state
        .borrow()
        .tabs
        .iter()
        .find(|t| t.id == cur)
        .map(|t| t.text_view.clone())
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
    let folder_parents: Vec<(&str, &str)> = folders
        .iter()
        .map(|f| (f.id.as_str(), f.parent.as_str()))
        .collect();
    let note_folders: Vec<&str> = notes.iter().map(|n| n.folder.as_str()).collect();
    for f in subfolders {
        let is_expanded = expanded.contains(f.id.as_str());
        let count = subtree_note_count(f.id.as_str(), &folder_parents, &note_folders);
        ui.tree.append(&folder_row(
            ui,
            state,
            f.id.as_str(),
            depth,
            &f.name,
            is_expanded,
            count,
        ));
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

/// Escape text for inclusion in Pango markup.
fn esc(s: &str) -> String {
    glib::markup_escape_text(s).to_string()
}

/// Emphasis with `_`; escapes the leaf text.
fn ital_underscore(s: &str) -> String {
    let mut out = String::new();
    for (i, p) in s.split('_').enumerate() {
        if i % 2 == 1 {
            out.push_str("<i>");
            out.push_str(&esc(p));
            out.push_str("</i>");
        } else {
            out.push_str(&esc(p));
        }
    }
    out
}

/// Emphasis with `*`, then `_`.
fn ital_star(s: &str) -> String {
    let mut out = String::new();
    for (i, p) in s.split('*').enumerate() {
        if i % 2 == 1 {
            out.push_str("<i>");
            out.push_str(&ital_underscore(p));
            out.push_str("</i>");
        } else {
            out.push_str(&ital_underscore(p));
        }
    }
    out
}

/// Bold `**`, then italics. Toggle-splitting keeps every tag balanced even for
/// unmatched delimiters, so the result is always valid Pango markup.
fn emphasis(s: &str) -> String {
    let mut out = String::new();
    for (i, p) in s.split("**").enumerate() {
        if i % 2 == 1 {
            out.push_str("<b>");
            out.push_str(&ital_star(p));
            out.push_str("</b>");
        } else {
            out.push_str(&ital_star(p));
        }
    }
    out
}

/// Render one line's inline Markdown (code spans, bold, italics) to Pango.
fn inline_md(s: &str) -> String {
    let mut out = String::new();
    for (i, seg) in s.split('`').enumerate() {
        if i % 2 == 1 {
            out.push_str("<tt>");
            out.push_str(&esc(seg));
            out.push_str("</tt>");
        } else {
            out.push_str(&emphasis(seg));
        }
    }
    out
}

/// A leading-`#` heading: returns the level (1..=6) and the remaining text.
fn heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line.len() - line.trim_start_matches('#').len();
    if (1..=6).contains(&hashes)
        && let Some(rest) = line[hashes..].strip_prefix(' ')
    {
        return Some((hashes, rest));
    }
    None
}

/// Render Markdown to Pango markup for the preview pane. Deliberately small:
/// headings, bold/italic, inline code, fenced code blocks and bullet lists.
fn md_to_pango(src: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    let mut first = true;
    for line in src.split('\n') {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if !first {
            out.push('\n');
        }
        first = false;

        if in_code {
            out.push_str("<tt>");
            out.push_str(&esc(line));
            out.push_str("</tt>");
            continue;
        }

        let trimmed = line.trim_start();
        if let Some((level, rest)) = heading(trimmed) {
            let size = match level {
                1 => "xx-large",
                2 => "x-large",
                3 => "large",
                _ => "medium",
            };
            out.push_str(&format!(
                "<span size=\"{size}\" weight=\"bold\">{}</span>",
                inline_md(rest)
            ));
        } else if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            out.push_str("• ");
            out.push_str(&inline_md(rest));
        } else if let Some(rest) = trimmed.strip_prefix("> ") {
            out.push_str("<i>");
            out.push_str(&inline_md(rest));
            out.push_str("</i>");
        } else {
            out.push_str(&inline_md(line));
        }
    }
    out
}

// --- expanded-folders persistence (device-local UI state) ---

/// Path of the device-local file storing the expanded folder ids: `PN_GUI_STATE`
/// if set, else `$XDG_STATE_HOME/plain-note/expanded`, else `~/.local/state/...`.
fn gui_state_path() -> PathBuf {
    if let Ok(p) = std::env::var("PN_GUI_STATE") {
        return PathBuf::from(p);
    }
    let base = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".local/state")
        });
    base.join("plain-note").join("expanded")
}

/// Serialize expanded folder ids as one sorted id per line (dependency-free).
fn serialize_expanded(set: &HashSet<String>) -> String {
    let mut ids: Vec<&str> = set.iter().map(|s| s.as_str()).collect();
    ids.sort_unstable();
    ids.join("\n")
}

/// Parse the expanded-folders file: one id per line, blanks ignored.
fn parse_expanded(text: &str) -> HashSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

fn load_expanded(path: &std::path::Path) -> HashSet<String> {
    std::fs::read_to_string(path)
        .map(|s| parse_expanded(&s))
        .unwrap_or_default()
}

fn save_expanded(path: &std::path::Path, set: &HashSet<String>) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(path, serialize_expanded(set)) {
        eprintln!("plain-note-gui: could not save expanded state: {e}");
    }
}

/// Count notes in `folder_id` and all its descendant folders.
///
/// `folder_parents` is `(folder_id, parent_id)` for every folder; `note_folders`
/// is the folder id each note lives in. Used for the sidebar folder counts so a
/// folder holding only subfolders still reflects the notes nested under it.
fn subtree_note_count(
    folder_id: &str,
    folder_parents: &[(&str, &str)],
    note_folders: &[&str],
) -> usize {
    let mut subtree = vec![folder_id];
    let mut i = 0;
    while i < subtree.len() {
        let cur = subtree[i];
        for (id, parent) in folder_parents {
            if *parent == cur && !subtree.contains(id) {
                subtree.push(id);
            }
        }
        i += 1;
    }
    note_folders.iter().filter(|f| subtree.contains(f)).count()
}

// --- Markdown formatting transforms (pure, unit-tested) ---

/// Wrap `text` with `marker`, or unwrap it if already wrapped (toggle).
fn wrap_or_unwrap(text: &str, marker: &str) -> String {
    if text.len() >= 2 * marker.len() && text.starts_with(marker) && text.ends_with(marker) {
        text[marker.len()..text.len() - marker.len()].to_string()
    } else {
        format!("{marker}{text}{marker}")
    }
}

/// The heading level of a line (1..=6), or 0 if it is not a heading.
fn heading_level_of(line: &str) -> usize {
    let h = line.len() - line.trim_start_matches('#').len();
    if (1..=6).contains(&h) && line[h..].starts_with(' ') {
        h
    } else {
        0
    }
}

/// Toggle a heading of `level` on a line: apply it, change level, or remove it.
fn set_heading_line(line: &str, level: usize) -> String {
    let cur = heading_level_of(line);
    let body = if cur > 0 { &line[cur + 1..] } else { line };
    if cur == level {
        body.to_string()
    } else {
        format!("{} {body}", "#".repeat(level))
    }
}

/// Strip a leading list/quote marker (`- `, `* `, `> `, or `N. `) if present.
fn strip_list_marker(line: &str) -> &str {
    for p in ["- ", "* ", "> "] {
        if let Some(rest) = line.strip_prefix(p) {
            return rest;
        }
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && line[digits..].starts_with(". ") {
        return &line[digits + 2..];
    }
    line
}

/// Toggle a line prefix (`- `, `1. `, `> `): remove it if present, else replace
/// any existing list/quote marker with it.
fn toggle_line_prefix(line: &str, prefix: &str) -> String {
    if let Some(rest) = line.strip_prefix(prefix) {
        rest.to_string()
    } else {
        format!("{prefix}{}", strip_list_marker(line))
    }
}

/// A French word/character summary, e.g. "42 mots · 210 caractères".
fn count_text(text: &str) -> String {
    let words = text.split_whitespace().count();
    let chars = text.chars().count();
    let w = if words == 1 { "mot" } else { "mots" };
    let c = if chars == 1 {
        "caractère"
    } else {
        "caractères"
    };
    format!("{words} {w} · {chars} {c}")
}

fn folder_row(
    ui: &Ui,
    state: &Rc<RefCell<State>>,
    id: &str,
    depth: usize,
    name: &str,
    expanded: bool,
    count: usize,
) -> gtk::ListBoxRow {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    b.set_margin_start(8 + depth as i32 * 16);
    b.set_margin_end(4);
    b.set_margin_top(3);
    b.set_margin_bottom(3);
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
    let count_label = gtk::Label::new(Some(&count.to_string()));
    count_label.add_css_class("dim-label");
    count_label.add_css_class("caption");
    b.append(&chevron);
    b.append(&icon);
    b.append(&label);
    b.append(&count_label);
    b.append(&folder_menu(ui, state, id, name));
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&b));

    // Drop target: dropping a note here moves it into this folder.
    let drop = gtk::DropTarget::new(glib::types::Type::STRING, gtk::gdk::DragAction::MOVE);
    {
        let (ui, state, fid) = (ui.clone(), state.clone(), id.to_string());
        let row_ref = row.clone();
        drop.connect_drop(move |_, value, _, _| {
            row_ref.remove_css_class("pn-drop");
            let Ok(note_id) = value.get::<String>() else {
                return false;
            };
            mutate(&state, |doc| {
                doc.move_note(&NoteId::from(note_id), &fid, store::now_millis())
            });
            rebuild_tree(&ui, &state);
            true
        });
    }
    {
        let row_ref = row.clone();
        drop.connect_enter(move |_, _, _| {
            row_ref.add_css_class("pn-drop");
            gtk::gdk::DragAction::MOVE
        });
    }
    {
        let row_ref = row.clone();
        drop.connect_leave(move |_| row_ref.remove_css_class("pn-drop"));
    }
    row.add_controller(drop);
    row
}

/// The `⋯` menu on a folder row: rename, move, delete.
fn folder_menu(ui: &Ui, state: &Rc<RefCell<State>>, id: &str, name: &str) -> gtk::MenuButton {
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

    let rename = item("Renommer");
    {
        let (ui, state, id, name) = (ui.clone(), state.clone(), id.to_string(), name.to_string());
        let pop = popover.clone();
        rename.connect_clicked(move |btn| {
            pop.popdown();
            open_rename_folder_dialog(&ui, &state, &id, &name, btn);
        });
    }
    let move_btn = item("Déplacer vers…");
    {
        let (ui, state, id) = (ui.clone(), state.clone(), id.to_string());
        let pop = popover.clone();
        move_btn.connect_clicked(move |btn| {
            pop.popdown();
            open_move_folder_dialog(&ui, &state, &id, btn);
        });
    }
    let delete = item("Supprimer");
    delete.add_css_class("destructive-action");
    {
        let (ui, state, id) = (ui.clone(), state.clone(), id.to_string());
        let pop = popover.clone();
        delete.connect_clicked(move |_| {
            pop.popdown();
            mutate(&state, |doc| {
                doc.delete_folder(&FolderId::from(id.clone()), store::now_millis())
            });
            rebuild_tree(&ui, &state);
        });
    }
    menu.append(&rename);
    menu.append(&move_btn);
    menu.append(&delete);
    mb
}

fn open_rename_folder_dialog(
    ui: &Ui,
    state: &Rc<RefCell<State>>,
    id: &str,
    name: &str,
    anchor: &impl IsA<gtk::Widget>,
) {
    let entry = gtk::Entry::builder()
        .text(name)
        .activates_default(true)
        .build();
    let dialog = adw::AlertDialog::new(Some("Renommer le dossier"), None);
    dialog.set_extra_child(Some(&entry));
    dialog.add_response("cancel", "Annuler");
    dialog.add_response("rename", "Renommer");
    dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("rename"));
    dialog.set_close_response("cancel");

    let (ui, state, id) = (ui.clone(), state.clone(), id.to_string());
    dialog.connect_response(None, move |_, resp| {
        if resp != "rename" {
            return;
        }
        let new_name = entry.text().to_string();
        if new_name.trim().is_empty() {
            return;
        }
        mutate(&state, |doc| {
            doc.rename_folder(&FolderId::from(id.clone()), new_name.trim())
        });
        rebuild_tree(&ui, &state);
    });
    dialog.present(Some(anchor));
}

fn open_move_folder_dialog(
    ui: &Ui,
    state: &Rc<RefCell<State>>,
    id: &str,
    anchor: &impl IsA<gtk::Widget>,
) {
    // Destinations: root + every folder except the one being moved.
    let mut ids: Vec<String> = vec![ROOT_FOLDER.to_string()];
    let mut labels: Vec<String> = vec!["Racine".to_string()];
    {
        let st = state.borrow();
        let mut folders: Vec<(String, String)> = st
            .doc
            .list_folders()
            .unwrap_or_default()
            .into_iter()
            .filter(|f| f.id.as_str() != id)
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

    let dialog = adw::AlertDialog::new(Some("Déplacer le dossier vers"), None);
    dialog.set_extra_child(Some(&dropdown));
    dialog.add_response("cancel", "Annuler");
    dialog.add_response("move", "Déplacer");
    dialog.set_response_appearance("move", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("move"));
    dialog.set_close_response("cancel");

    let (ui, state, id) = (ui.clone(), state.clone(), id.to_string());
    dialog.connect_response(None, move |_, resp| {
        if resp != "move" {
            return;
        }
        let idx = dropdown.selected() as usize;
        if let Some(parent) = ids.get(idx) {
            let parent = parent.clone();
            mutate(&state, |doc| {
                doc.move_folder(&FolderId::from(id.clone()), &parent)
            });
            rebuild_tree(&ui, &state);
        }
    });
    dialog.present(Some(anchor));
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

    // Drag source: carry the note id so it can be dropped onto a folder.
    if !trash {
        let source = gtk::DragSource::new();
        source.set_actions(gtk::gdk::DragAction::MOVE);
        let id_str = id.as_str().to_string();
        source.connect_prepare(move |_, _, _| {
            Some(gtk::gdk::ContentProvider::for_value(&id_str.to_value()))
        });
        row.add_controller(source);
    }
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

// --- buffer-level formatting actions ---

/// The current selection, or a zero-width range at the cursor if none.
fn sel_bounds(b: &gtk::TextBuffer) -> (gtk::TextIter, gtk::TextIter) {
    if let Some(bounds) = b.selection_bounds() {
        bounds
    } else {
        let c = b.iter_at_mark(&b.get_insert());
        (c, c)
    }
}

/// Wrap/unwrap the selection with an inline `marker` (e.g. `**`, `*`, `` ` ``).
fn apply_wrap(b: &gtk::TextBuffer, marker: &str) {
    let (mut s, mut e) = sel_bounds(b);
    let off = s.offset();
    let text = b.text(&s, &e, false).to_string();
    let empty = text.is_empty();
    let new = wrap_or_unwrap(&text, marker);
    b.delete(&mut s, &mut e);
    let mut ins = b.iter_at_offset(off);
    b.insert(&mut ins, &new);
    if empty {
        let cur = b.iter_at_offset(off + marker.chars().count() as i32);
        b.place_cursor(&cur);
    } else {
        let a = b.iter_at_offset(off);
        let z = b.iter_at_offset(off + new.chars().count() as i32);
        b.select_range(&a, &z);
    }
}

/// Apply a per-line transform to every line the selection spans.
fn transform_lines(b: &gtk::TextBuffer, f: impl Fn(&str) -> String) {
    let (s, e) = sel_bounds(b);
    let (first, last) = (s.line(), e.line());
    let Some(mut ls) = b.iter_at_line(first) else {
        return;
    };
    let Some(mut le) = b.iter_at_line(last) else {
        return;
    };
    le.forward_to_line_end();
    let off = ls.offset();
    let block = b.text(&ls, &le, false).to_string();
    let new = block.split('\n').map(f).collect::<Vec<_>>().join("\n");
    b.delete(&mut ls, &mut le);
    let mut ins = b.iter_at_offset(off);
    b.insert(&mut ins, &new);
    let a = b.iter_at_offset(off);
    let z = b.iter_at_offset(off + new.chars().count() as i32);
    b.select_range(&a, &z);
}

/// Insert a fenced code block around the selection.
fn apply_code_block(b: &gtk::TextBuffer) {
    let (mut s, mut e) = sel_bounds(b);
    let off = s.offset();
    let text = b.text(&s, &e, false).to_string();
    let new = format!("```\n{text}\n```");
    b.delete(&mut s, &mut e);
    let mut ins = b.iter_at_offset(off);
    b.insert(&mut ins, &new);
    // Place the cursor on the (possibly empty) content line.
    let cur = b.iter_at_offset(off + 4 + text.chars().count() as i32);
    b.place_cursor(&cur);
}

/// Insert a Markdown link, selecting the `url` placeholder for quick typing.
fn apply_link(b: &gtk::TextBuffer) {
    let (mut s, mut e) = sel_bounds(b);
    let off = s.offset();
    let text = b.text(&s, &e, false).to_string();
    let label = if text.is_empty() { "texte" } else { &text };
    let new = format!("[{label}](url)");
    b.delete(&mut s, &mut e);
    let mut ins = b.iter_at_offset(off);
    b.insert(&mut ins, &new);
    // "url" sits after "[label](".
    let url_start = off + 1 + label.chars().count() as i32 + 2;
    let a = b.iter_at_offset(url_start);
    let z = b.iter_at_offset(url_start + 3);
    b.select_range(&a, &z);
}

/// A formatting toolbar bound to one editor's text view. Buttons return focus
/// to the text so typing continues right after a formatting action.
fn format_toolbar(text_view: &gtk::TextView) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    bar.add_css_class("toolbar");
    bar.set_margin_start(18);
    bar.set_margin_end(18);

    let buffer = text_view.buffer();
    let button = |markup: &str, tip: &str, op: Box<dyn Fn(&gtk::TextBuffer)>| -> gtk::Button {
        let label = gtk::Label::new(None);
        label.set_markup(markup);
        let btn = gtk::Button::builder().child(&label).build();
        btn.add_css_class("flat");
        btn.set_tooltip_text(Some(tip));
        let buffer = buffer.clone();
        let tv = text_view.clone();
        btn.connect_clicked(move |_| {
            op(&buffer);
            tv.grab_focus();
        });
        btn
    };
    let sep = || {
        let s = gtk::Separator::new(gtk::Orientation::Vertical);
        s.set_margin_top(4);
        s.set_margin_bottom(4);
        s
    };

    bar.append(&button(
        "<b>B</b>",
        "Gras (Ctrl+B)",
        Box::new(|b| apply_wrap(b, "**")),
    ));
    bar.append(&button(
        "<i>I</i>",
        "Italique (Ctrl+I)",
        Box::new(|b| apply_wrap(b, "*")),
    ));
    bar.append(&button(
        "<s>S</s>",
        "Barré",
        Box::new(|b| apply_wrap(b, "~~")),
    ));
    bar.append(&button(
        "<tt>&lt;/&gt;</tt>",
        "Code (Ctrl+E)",
        Box::new(|b| apply_wrap(b, "`")),
    ));
    bar.append(&sep());
    for level in 1..=3usize {
        bar.append(&button(
            &format!("H{level}"),
            &format!("Titre {level}"),
            Box::new(move |b| transform_lines(b, |l| set_heading_line(l, level))),
        ));
    }
    bar.append(&sep());
    bar.append(&button(
        "•",
        "Liste à puces",
        Box::new(|b| transform_lines(b, |l| toggle_line_prefix(l, "- "))),
    ));
    bar.append(&button(
        "1.",
        "Liste numérotée",
        Box::new(|b| transform_lines(b, |l| toggle_line_prefix(l, "1. "))),
    ));
    bar.append(&button(
        "❝",
        "Citation",
        Box::new(|b| transform_lines(b, |l| toggle_line_prefix(l, "> "))),
    ));
    bar.append(&button(
        "<tt>{ }</tt>",
        "Bloc de code",
        Box::new(apply_code_block),
    ));
    bar.append(&button("🔗", "Lien (Ctrl+K)", Box::new(apply_link)));
    bar
}

fn open_note(ui: &Ui, state: &Rc<RefCell<State>>, id: &NoteId) {
    let existing = state
        .borrow()
        .tabs
        .iter()
        .find(|t| &t.id == id)
        .map(|t| t.page.clone());
    let page = match existing {
        Some(page) => page,
        None => {
            let note = state.borrow().doc.get_note(id).ok().flatten();
            let Some(note) = note else { return };
            let tab = build_editor_pane(ui, state, &note);
            let page = tab.page.clone();
            state.borrow_mut().tabs.push(tab);
            page
        }
    };
    ui.tab_view.set_selected_page(&page);
    state.borrow_mut().current = Some(id.clone());
    sync_header(ui, state, id);
    reselect_current(ui, state);
}

/// Build an editor pane for `note`, append it as a tab, and wire its auto-save.
fn build_editor_pane(ui: &Ui, state: &Rc<RefCell<State>>, note: &note_core::Note) -> Tab {
    let id = note.id.clone();

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

    // Edit / preview stack: the raw editor, or a rendered Markdown view.
    let preview_label = gtk::Label::new(None);
    preview_label.set_wrap(true);
    preview_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    preview_label.set_xalign(0.0);
    preview_label.set_yalign(0.0);
    preview_label.set_selectable(true);
    preview_label.set_margin_start(18);
    preview_label.set_margin_end(18);
    preview_label.set_margin_top(10);
    preview_label.set_margin_bottom(18);
    let preview_scroll = gtk::ScrolledWindow::builder()
        .child(&preview_label)
        .vexpand(true)
        .build();
    let stack = gtk::Stack::new();
    stack.set_vexpand(true);
    stack.add_named(&text_scroll, Some("edit"));
    stack.add_named(&preview_scroll, Some("preview"));

    // Footer: preview toggle on the left, live word/character count on the right.
    let preview_toggle = gtk::ToggleButton::with_label("Aperçu");
    preview_toggle.add_css_class("flat");
    let count_label = gtk::Label::new(None);
    count_label.add_css_class("dim-label");
    count_label.add_css_class("caption");
    count_label.set_hexpand(true);
    count_label.set_halign(gtk::Align::End);
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    footer.set_margin_start(18);
    footer.set_margin_end(18);
    footer.set_margin_top(4);
    footer.set_margin_bottom(6);
    footer.append(&preview_toggle);
    footer.append(&count_label);

    // Attachments row: one chip per attachment + an "add" button. The button is
    // only sensitive when the device is enrolled (attach needs the relay).
    let atts_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let attach_btn = gtk::Button::from_icon_name("mail-attachment-symbolic");
    attach_btn.add_css_class("flat");
    attach_btn.set_tooltip_text(Some("Joindre un fichier"));
    let enrolled = config::Settings::load_from(&config::config_path()).is_ok();
    attach_btn.set_sensitive(enrolled);
    if !enrolled {
        attach_btn.set_tooltip_text(Some("Nécessite la synchronisation"));
    }
    let atts_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    atts_row.set_margin_start(18);
    atts_row.set_margin_end(18);
    atts_row.set_margin_top(4);
    atts_row.append(&attach_btn);
    atts_row.append(&atts_box);
    let atts_scroll = gtk::ScrolledWindow::builder()
        .child(&atts_row)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .build();

    let toolbar = format_toolbar(&text_view);

    let editor = gtk::Box::new(gtk::Orientation::Vertical, 0);
    editor.append(&title);
    editor.append(&tags_scroll);
    editor.append(&atts_scroll);
    editor.append(&toolbar);
    editor.append(&stack);
    editor.append(&footer);

    // Toggle between editing and a rendered Markdown preview.
    {
        let buffer = buffer.clone();
        let stack = stack.clone();
        let preview_label = preview_label.clone();
        preview_toggle.connect_toggled(move |btn| {
            if btn.is_active() {
                let text = buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), false)
                    .to_string();
                preview_label.set_markup(&md_to_pango(&text));
                stack.set_visible_child_name("preview");
            } else {
                stack.set_visible_child_name("edit");
            }
        });
    }

    // Seed content BEFORE connecting handlers so the initial load never writes
    // back (which would bump the note's `updated` timestamp).
    title.set_text(&note.title);
    buffer.set_text(&note.text);
    count_label.set_text(&count_text(&note.text));

    let page = ui.tab_view.append(&editor);
    page.set_title(note_title(&note.title));

    // Keep the count in sync on every edit (including programmatic reloads).
    {
        let count_label = count_label.clone();
        buffer.connect_changed(move |buf| {
            let text = buf
                .text(&buf.start_iter(), &buf.end_iter(), false)
                .to_string();
            count_label.set_text(&count_text(&text));
        });
    }

    // Title edits -> save + update tab, sidebar row and (if active) the header.
    {
        let ui = ui.clone();
        let state = state.clone();
        let id = id.clone();
        let page = page.clone();
        title.connect_changed(move |entry| {
            if state.borrow().loading {
                return;
            }
            let text = entry.text().to_string();
            {
                let mut st = state.borrow_mut();
                let _ = st.doc.set_title(&id, &text, store::now_millis());
                st.persist();
            }
            page.set_title(note_title(&text));
            set_row_title(&ui, &state, &id, &text);
            if state.borrow().current.as_ref() == Some(&id) {
                sync_header(&ui, &state, &id);
            }
        });
    }

    // Body edits -> save.
    {
        let state = state.clone();
        let id = id.clone();
        buffer.connect_changed(move |buf| {
            if state.borrow().loading {
                return;
            }
            let text = buf
                .text(&buf.start_iter(), &buf.end_iter(), false)
                .to_string();
            let mut st = state.borrow_mut();
            let _ = st.doc.replace_text(&id, &text, store::now_millis());
            st.persist();
        });
    }

    // Add tag on Enter.
    {
        let state = state.clone();
        let id = id.clone();
        let tags_box = tags_box.clone();
        tag_entry.connect_activate(move |entry| {
            let tag = entry.text().to_string();
            let tag = tag.trim().trim_start_matches('#');
            if tag.is_empty() {
                return;
            }
            {
                let mut st = state.borrow_mut();
                let _ = st.doc.add_tag(&id, tag, store::now_millis());
                st.persist();
            }
            entry.set_text("");
            fill_tags(&state, &tags_box, &id);
        });
    }

    // Attach a file: pick it, then upload on a background runtime.
    {
        let ui = ui.clone();
        let state = state.clone();
        let id = id.clone();
        attach_btn.connect_clicked(move |btn| {
            let dialog = gtk::FileDialog::new();
            dialog.set_title("Joindre un fichier");
            let window = btn.root().and_downcast::<gtk::Window>();
            let ui = ui.clone();
            let state = state.clone();
            let id = id.clone();
            dialog.open(window.as_ref(), gtk::gio::Cancellable::NONE, move |res| {
                let Ok(file) = res else { return };
                let Some(path) = file.path() else { return };
                upload_attachment(&ui, &state, &id, path);
            });
        });
    }

    let tab = Tab {
        id: id.clone(),
        page,
        title,
        text_view,
        buffer,
        tags_box: tags_box.clone(),
        atts_box: atts_box.clone(),
    };
    fill_tags(state, &tags_box, &id);
    fill_attachments(ui, state, &atts_box, &id);
    tab
}

/// Rebuild the attachment chips of one editor pane. Each chip downloads on
/// click; the trailing ✕ drops the reference (the blob stays on the relay).
fn fill_attachments(ui: &Ui, state: &Rc<RefCell<State>>, atts_box: &gtk::Box, id: &NoteId) {
    while let Some(child) = atts_box.first_child() {
        atts_box.remove(&child);
    }
    let atts = {
        let st = state.borrow();
        st.doc
            .get_note(id)
            .ok()
            .flatten()
            .map(|n| n.attachments)
            .unwrap_or_default()
    };
    let enrolled = config::Settings::load_from(&config::config_path()).is_ok();
    for (aid, name) in atts {
        let chip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        chip.add_css_class("pn-chip");
        let open = gtk::Button::with_label(&format!("📎 {name}"));
        open.add_css_class("flat");
        open.set_sensitive(enrolled);
        open.set_tooltip_text(Some(if enrolled {
            "Télécharger"
        } else {
            "Nécessite la synchronisation"
        }));
        {
            let (id, aid, name) = (id.clone(), aid.clone(), name.clone());
            open.connect_clicked(move |btn| {
                download_attachment(&id, &aid, &name, btn);
            });
        }
        let remove = gtk::Button::from_icon_name("window-close-symbolic");
        remove.add_css_class("flat");
        remove.set_tooltip_text(Some("Retirer"));
        {
            let (ui, state, id, aid, atts_box) = (
                ui.clone(),
                state.clone(),
                id.clone(),
                aid.clone(),
                atts_box.clone(),
            );
            remove.connect_clicked(move |_| {
                mutate(&state, |doc| {
                    doc.remove_attachment(&id, &aid, store::now_millis())
                });
                fill_attachments(&ui, &state, &atts_box, &id);
            });
        }
        chip.append(&open);
        chip.append(&remove);
        atts_box.append(&chip);
    }
}

/// Reload the in-memory doc from disk (after a background op wrote to it).
fn reload_from_disk(state: &Rc<RefCell<State>>) {
    let loaded = state.borrow().store.load();
    if let Ok(doc) = loaded {
        let sig = content_sig(&doc);
        let mut st = state.borrow_mut();
        st.doc = doc;
        st.last_sig = sig;
    }
}

/// Encrypt and upload `path` as an attachment of `id`, off the main loop.
fn upload_attachment(ui: &Ui, state: &Rc<RefCell<State>>, id: &NoteId, path: PathBuf) {
    let (tx, rx) = async_channel::bounded::<Result<(), String>>(1);
    let cfg = config::config_path();
    let note = id.as_str().to_string();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                let _ = tx.send_blocking(Err(e.to_string()));
                return;
            }
        };
        let store = LocalStore::at_default();
        let now = store::now_millis();
        let r = rt
            .block_on(remote::attach(&cfg, &store, now, &note, &path))
            .map(|_| ())
            .map_err(|e| e.to_string());
        let _ = tx.send_blocking(r);
    });

    let ui = ui.clone();
    let state = state.clone();
    let id = id.clone();
    glib::spawn_future_local(async move {
        match rx.recv().await {
            Ok(Ok(())) => {
                reload_from_disk(&state);
                let atts_box = state
                    .borrow()
                    .tabs
                    .iter()
                    .find(|t| t.id == id)
                    .map(|t| t.atts_box.clone());
                if let Some(b) = atts_box {
                    fill_attachments(&ui, &state, &b, &id);
                }
            }
            Ok(Err(e)) => eprintln!("plain-note-gui: attach failed: {e}"),
            Err(_) => {}
        }
    });
}

/// Download and decrypt an attachment to a chosen location, off the main loop.
fn download_attachment(id: &NoteId, aid: &str, name: &str, anchor: &impl IsA<gtk::Widget>) {
    let dialog = gtk::FileDialog::new();
    dialog.set_title("Enregistrer la pièce jointe");
    dialog.set_initial_name(Some(name));
    let window = anchor.root().and_downcast::<gtk::Window>();
    let cfg = config::config_path();
    let note = id.as_str().to_string();
    let aid = aid.to_string();
    dialog.save(window.as_ref(), gtk::gio::Cancellable::NONE, move |res| {
        let Ok(file) = res else { return };
        let Some(path) = file.path() else { return };
        let (tx, rx) = async_channel::bounded::<Result<PathBuf, String>>(1);
        let cfg = cfg.clone();
        let note = note.clone();
        let aid = aid.clone();
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send_blocking(Err(e.to_string()));
                    return;
                }
            };
            let store = LocalStore::at_default();
            let r = rt
                .block_on(remote::fetch(&cfg, &store, &note, &aid, Some(path)))
                .map_err(|e| e.to_string());
            let _ = tx.send_blocking(r);
        });
        glib::spawn_future_local(async move {
            match rx.recv().await {
                Ok(Ok(p)) => eprintln!("plain-note-gui: saved {}", p.display()),
                Ok(Err(e)) => eprintln!("plain-note-gui: download failed: {e}"),
                Err(_) => {}
            }
        });
    });
}

/// Rebuild the tag chips of one editor pane from the note's current tags.
fn fill_tags(state: &Rc<RefCell<State>>, tags_box: &gtk::Box, id: &NoteId) {
    while let Some(child) = tags_box.first_child() {
        tags_box.remove(&child);
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
        let state = state.clone();
        let id = id.clone();
        let box_for_cb = tags_box.clone();
        let tag_name = tag.clone();
        chip.connect_clicked(move |_| {
            {
                let mut st = state.borrow_mut();
                let _ = st.doc.remove_tag(&id, &tag_name, store::now_millis());
                st.persist();
            }
            fill_tags(&state, &box_for_cb, &id);
        });
        tags_box.append(&chip);
    }
}

/// Update the header title/subtitle for the note shown in the active tab.
fn sync_header(ui: &Ui, state: &Rc<RefCell<State>>, id: &NoteId) {
    let note = state.borrow().doc.get_note(id).ok().flatten();
    let Some(note) = note else { return };
    let path = state
        .borrow()
        .doc
        .folder_path(&note.folder)
        .unwrap_or_default();
    ui.subtitle.set_title(note_title(&note.title));
    ui.subtitle.set_subtitle(&path);
}

/// Update the sidebar row label for `id` without rebuilding the tree.
fn set_row_title(ui: &Ui, state: &Rc<RefCell<State>>, id: &NoteId, text: &str) {
    let Some(idx) = row_index_of(state, id) else {
        return;
    };
    let Some(row) = ui.tree.row_at_index(idx as i32) else {
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

fn reselect_current(ui: &Ui, state: &Rc<RefCell<State>>) {
    let cur = state.borrow().current.clone();
    if let Some(id) = cur
        && let Some(idx) = row_index_of(state, &id)
        && let Some(row) = ui.tree.row_at_index(idx as i32)
    {
        ui.tree.select_row(Some(&row));
    }
}

fn row_index_of(state: &Rc<RefCell<State>>, id: &NoteId) -> Option<usize> {
    state
        .borrow()
        .rows
        .iter()
        .position(|r| matches!(r, RowKind::Note(n) if n == id))
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
    {
        let mut st = state.borrow_mut();
        st.doc = doc;
        st.last_sig = sig;
    }

    // Refresh every open tab from the reloaded doc. Skip a tab being edited so
    // remote changes never yank text from under the cursor; close a tab whose
    // note has disappeared.
    let tabs = state.borrow().tabs.clone();
    let mut editing = false;
    for tab in &tabs {
        let note = state.borrow().doc.get_note(&tab.id).ok().flatten();
        let Some(note) = note else {
            ui.tab_view.close_page(&tab.page);
            continue;
        };
        if tab.title.has_focus() || tab.text_view.has_focus() {
            editing = true;
            continue;
        }
        let cur_title = tab.title.text().to_string();
        let cur_body = tab
            .buffer
            .text(&tab.buffer.start_iter(), &tab.buffer.end_iter(), false)
            .to_string();
        if cur_title != note.title || cur_body != note.text {
            state.borrow_mut().loading = true;
            if cur_title != note.title {
                tab.title.set_text(&note.title);
            }
            if cur_body != note.text {
                tab.buffer.set_text(&note.text);
            }
            state.borrow_mut().loading = false;
        }
        tab.page.set_title(note_title(&note.title));
        fill_tags(state, &tab.tags_box, &tab.id);
        fill_attachments(ui, state, &tab.atts_box, &tab.id);
    }

    rebuild_tree(ui, state);
    if !editing {
        reselect_current(ui, state);
    }
    if let Some(id) = state.borrow().current.clone() {
        sync_header(ui, state, &id);
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
         .pn-chip { padding: 2px 8px; min-height: 0; } \
         .pn-drop { background-color: alpha(@accent_bg_color, 0.25); border-radius: 6px; }",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        count_text, md_to_pango, parse_expanded, serialize_expanded, set_heading_line,
        subtree_note_count, toggle_line_prefix, wrap_or_unwrap,
    };
    use std::collections::HashSet;

    #[test]
    fn expanded_round_trips() {
        let set: HashSet<String> = ["b".to_string(), "a".to_string()].into_iter().collect();
        // Serialized form is sorted and stable.
        assert_eq!(serialize_expanded(&set), "a\nb");
        assert_eq!(parse_expanded(&serialize_expanded(&set)), set);
    }

    #[test]
    fn parse_expanded_ignores_blank_lines() {
        assert_eq!(parse_expanded(""), HashSet::new());
        let got = parse_expanded("a\n\n  \nb\n");
        let want: HashSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
        assert_eq!(got, want);
    }

    #[test]
    fn subtree_count_includes_descendants() {
        // A contains B, B contains 2 notes; A directly contains none.
        let folders = [("a", ""), ("b", "a")];
        let note_folders = ["b", "b"];
        // The bug: A used to show 0; it must now show 2 (its whole subtree).
        assert_eq!(subtree_note_count("a", &folders, &note_folders), 2);
        assert_eq!(subtree_note_count("b", &folders, &note_folders), 2);
    }

    #[test]
    fn subtree_count_direct_and_nested() {
        // a has 1 direct note + child b with 1 note + grandchild c with 1.
        let folders = [("a", ""), ("b", "a"), ("c", "b")];
        let note_folders = ["a", "b", "c"];
        assert_eq!(subtree_note_count("a", &folders, &note_folders), 3);
        assert_eq!(subtree_note_count("b", &folders, &note_folders), 2);
        assert_eq!(subtree_note_count("c", &folders, &note_folders), 1);
    }

    #[test]
    fn wrap_and_unwrap_toggles() {
        assert_eq!(wrap_or_unwrap("gras", "**"), "**gras**");
        assert_eq!(wrap_or_unwrap("**gras**", "**"), "gras");
        assert_eq!(wrap_or_unwrap("", "*"), "**");
        assert_eq!(wrap_or_unwrap("x", "`"), "`x`");
    }

    #[test]
    fn heading_toggles_and_switches_level() {
        assert_eq!(set_heading_line("Titre", 1), "# Titre");
        assert_eq!(set_heading_line("# Titre", 1), "Titre"); // same level -> off
        assert_eq!(set_heading_line("# Titre", 2), "## Titre"); // switch level
        assert_eq!(set_heading_line("### Titre", 2), "## Titre");
    }

    #[test]
    fn line_prefix_toggles_and_replaces() {
        assert_eq!(toggle_line_prefix("item", "- "), "- item");
        assert_eq!(toggle_line_prefix("- item", "- "), "item"); // off
        assert_eq!(toggle_line_prefix("- item", "1. "), "1. item"); // replace marker
        assert_eq!(toggle_line_prefix("1. item", "> "), "> item");
    }

    #[test]
    fn count_text_pluralizes() {
        assert_eq!(count_text(""), "0 mots · 0 caractères");
        assert_eq!(count_text("a"), "1 mot · 1 caractère");
        assert_eq!(count_text("un deux"), "2 mots · 7 caractères");
    }

    #[test]
    fn count_text_ignores_extra_whitespace() {
        assert_eq!(count_text("  un   deux  "), "2 mots · 13 caractères");
    }

    #[test]
    fn md_escapes_markup_special_chars() {
        assert_eq!(md_to_pango("a < b & c"), "a &lt; b &amp; c");
    }

    #[test]
    fn md_renders_emphasis_and_code() {
        assert_eq!(md_to_pango("**gras**"), "<b>gras</b>");
        assert_eq!(md_to_pango("*ital*"), "<i>ital</i>");
        assert_eq!(md_to_pango("_ital_"), "<i>ital</i>");
        assert_eq!(md_to_pango("`code`"), "<tt>code</tt>");
    }

    #[test]
    fn md_renders_headings_and_lists() {
        assert_eq!(
            md_to_pango("# Titre"),
            "<span size=\"xx-large\" weight=\"bold\">Titre</span>"
        );
        assert_eq!(md_to_pango("- item"), "• item");
        assert_eq!(md_to_pango("* item"), "• item");
    }

    #[test]
    fn md_renders_fenced_code_block_verbatim() {
        assert_eq!(md_to_pango("```\na*b*\n```"), "<tt>a*b*</tt>");
    }

    #[test]
    fn md_unmatched_delimiters_stay_balanced() {
        // Toggle-splitting must never emit an unclosed tag.
        let out = md_to_pango("**oups");
        assert_eq!(out, "<b>oups</b>");
    }
}
