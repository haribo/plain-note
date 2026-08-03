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

    let sync_btn = gtk::Button::from_icon_name("emblem-synchronizing-symbolic");
    sync_btn.add_css_class("flat");
    sync_btn.set_tooltip_text(Some("Synchronisation"));
    content_header.pack_end(&sync_btn);
    {
        let ui = ui.clone();
        let state = state.clone();
        let sync_label = sync_label.clone();
        sync_btn.connect_clicked(move |btn| open_sync_dialog(btn, &ui, &state, &sync_label));
    }

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

// --- WYSIWYG inline styling (pure, unit-tested) ---

/// A styled or hidden span over the source, in CHARACTER offsets (GtkTextBuffer
/// iters use character offsets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpanKind {
    Bold,
    Italic,
    Code,
    Strike,
    Hidden,
    H1,
    H2,
    H3,
    Link,
    Quote,
    CodeBlock,
    ListItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    start: usize,
    end: usize,
    kind: SpanKind,
}

/// Inline Markdown marks → styling spans, with the markers themselves returned
/// as `Hidden`. Nested marks stack (`***x***` = bold+italic). An unterminated or
/// empty marker is left literal (no span). Offsets are character indices.
fn inline_spans(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    // Longest markers first so `***`/`**` win over `*`.
    let markers: [(&str, &[SpanKind]); 5] = [
        ("***", &[SpanKind::Bold, SpanKind::Italic]),
        ("**", &[SpanKind::Bold]),
        ("~~", &[SpanKind::Strike]),
        ("*", &[SpanKind::Italic]),
        ("`", &[SpanKind::Code]),
    ];
    let mut out = Vec::new();
    parse_marks(&chars, 0, chars.len(), &markers, &mut out);
    out
}

fn matches_at(chars: &[char], i: usize, m: &[char]) -> bool {
    i + m.len() <= chars.len() && chars[i..i + m.len()] == *m
}

/// First index >= `from` (with `close + m.len() <= hi`) where marker `m` recurs.
fn find_close(chars: &[char], from: usize, hi: usize, m: &[char]) -> Option<usize> {
    let mut j = from;
    while j + m.len() <= hi {
        if chars[j..j + m.len()] == *m {
            return Some(j);
        }
        j += 1;
    }
    None
}

fn parse_marks(
    chars: &[char],
    lo: usize,
    hi: usize,
    markers: &[(&str, &[SpanKind])],
    out: &mut Vec<Span>,
) {
    let mut i = lo;
    while i < hi {
        // Link `[label](url)`: keep the label (styled), hide `[`, `]`, `(url)`.
        if chars[i] == '[' {
            let rb = find_close(chars, i + 1, hi, &[']']);
            if let Some(rb) = rb
                && rb > i + 1
                && rb + 1 < hi
                && chars[rb + 1] == '('
                && let Some(rp) = find_close(chars, rb + 2, hi, &[')'])
            {
                out.push(Span {
                    start: i,
                    end: i + 1,
                    kind: SpanKind::Hidden,
                });
                out.push(Span {
                    start: i + 1,
                    end: rb,
                    kind: SpanKind::Link,
                });
                parse_marks(chars, i + 1, rb, markers, out); // marks inside the label
                out.push(Span {
                    start: rb,
                    end: rp + 1,
                    kind: SpanKind::Hidden,
                });
                i = rp + 1;
                continue;
            }
        }
        let mut matched = false;
        for (m, kinds) in markers {
            let mc: Vec<char> = m.chars().collect();
            let len = mc.len();
            if !matches_at(chars, i, &mc) {
                continue;
            }
            // A marker matches only with non-empty content and a close before `hi`.
            if let Some(c) = find_close(chars, i + len, hi, &mc)
                && c > i + len
            {
                out.push(Span {
                    start: i,
                    end: i + len,
                    kind: SpanKind::Hidden,
                });
                for k in *kinds {
                    out.push(Span {
                        start: i + len,
                        end: c,
                        kind: *k,
                    });
                }
                parse_marks(chars, i + len, c, markers, out);
                out.push(Span {
                    start: c,
                    end: c + len,
                    kind: SpanKind::Hidden,
                });
                i = c + len;
                matched = true;
                break;
            }
        }
        if !matched {
            i += 1;
        }
    }
}

/// A heading prefix (`#`..`######` + space): returns the level (capped at 3 for
/// the editor) and the prefix length in chars (hashes + the space).
fn heading_prefix(line: &str) -> Option<(u8, usize)> {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) && line.chars().nth(hashes) == Some(' ') {
        return Some((hashes.min(3) as u8, hashes + 1));
    }
    None
}

/// All WYSIWYG spans over the full text (char offsets): heading prefixes hidden
/// and their content sized (H1/H2/H3), plus inline marks (markers hidden).
fn spans(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    let mut base = 0usize; // char offset of the current line's start
    let mut in_code = false; // inside a ``` fenced block
    for line in text.split('\n') {
        let ll = line.chars().count();
        if line.starts_with("```") {
            // Fence line (with optional language): hidden; toggles code state.
            out.push(Span {
                start: base,
                end: base + ll,
                kind: SpanKind::Hidden,
            });
            in_code = !in_code;
        } else if in_code {
            // Verbatim: monospace, no inline/heading/quote parsing.
            if ll > 0 {
                out.push(Span {
                    start: base,
                    end: base + ll,
                    kind: SpanKind::CodeBlock,
                });
            }
        } else if let Some((level, prefix)) = heading_prefix(line) {
            out.push(Span {
                start: base,
                end: base + prefix,
                kind: SpanKind::Hidden,
            });
            let kind = match level {
                1 => SpanKind::H1,
                2 => SpanKind::H2,
                _ => SpanKind::H3,
            };
            out.push(Span {
                start: base + prefix,
                end: base + ll,
                kind,
            });
            let content: String = line.chars().skip(prefix).collect();
            for s in inline_spans(&content) {
                out.push(Span {
                    start: base + prefix + s.start,
                    end: base + prefix + s.end,
                    kind: s.kind,
                });
            }
        } else if let Some((mlen, _)) = line_list_marker(line) {
            // Hide the source marker; the gutter glyph is drawn by the view.
            out.push(Span {
                start: base,
                end: base + mlen,
                kind: SpanKind::Hidden,
            });
            out.push(Span {
                start: base,
                end: base + ll,
                kind: SpanKind::ListItem, // indent (left_margin) leaves a gutter
            });
            let content: String = line.chars().skip(mlen).collect();
            for s in inline_spans(&content) {
                out.push(Span {
                    start: base + mlen + s.start,
                    end: base + mlen + s.end,
                    kind: s.kind,
                });
            }
        } else if let Some(content) = line.strip_prefix("> ") {
            out.push(Span {
                start: base,
                end: base + 2,
                kind: SpanKind::Hidden,
            });
            // Quote covers the whole line (including the hidden `> `) so its
            // paragraph `left_margin` indents from the line start; the prefix
            // stays invisible via the Hidden span above.
            out.push(Span {
                start: base,
                end: base + ll,
                kind: SpanKind::Quote,
            });
            for s in inline_spans(content) {
                out.push(Span {
                    start: base + 2 + s.start,
                    end: base + 2 + s.end,
                    kind: s.kind,
                });
            }
        } else {
            for s in inline_spans(line) {
                out.push(Span {
                    start: base + s.start,
                    end: base + s.end,
                    kind: s.kind,
                });
            }
        }
        base += ll + 1; // account for the '\n'
    }
    out
}

/// URL of the link whose *label* contains char `offset`, if any. Mirrors what is
/// rendered as a link: code fences are skipped, and only the visible label (not
/// the hidden `](url)`) is clickable. Used to follow links on Ctrl+click.
fn link_at(text: &str, offset: usize) -> Option<String> {
    let mut base = 0usize;
    let mut in_code = false;
    for line in text.split('\n') {
        let ll = line.chars().count();
        if line.starts_with("```") {
            in_code = !in_code;
        } else if !in_code && offset >= base && offset <= base + ll {
            return link_in_line(line, offset - base);
        }
        base += ll + 1;
    }
    None
}

/// URL of the `[label](url)` whose label contains the line-local char `off`.
fn link_in_line(line: &str, off: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        if chars[i] == '['
            && let Some(rb) = find_close(&chars, i + 1, n, &[']'])
            && rb > i + 1
            && rb + 1 < n
            && chars[rb + 1] == '('
            && let Some(rp) = find_close(&chars, rb + 2, n, &[')'])
        {
            if off > i && off < rb {
                let url: String = chars[rb + 2..rp].iter().collect();
                if !url.is_empty() {
                    return Some(url);
                }
            }
            i = rp + 1;
            continue;
        }
        i += 1;
    }
    None
}

/// The leading list marker of `line`, if any: returns its length in chars and
/// the glyph to draw in the gutter. `- `/`* `/`+ ` render as a bullet; `N. `
/// keeps the number. The marker itself is hidden in the source; the view draws
/// the returned glyph in the left gutter.
fn line_list_marker(line: &str) -> Option<(usize, String)> {
    if line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ") {
        return Some((2, "•".to_string()));
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let rest: String = line.chars().skip(digits).collect();
        if rest.starts_with(". ") {
            let num: String = line.chars().take(digits).collect();
            return Some((digits + 2, format!("{num}.")));
        }
    }
    None
}

/// For each list line, the char offset of its start (the hidden marker sits at
/// the paragraph's left edge) and the gutter glyph to draw. Skips ``` fenced
/// blocks. The view draws the glyph; `spans` hides the source markers in step.
fn list_markers(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut base = 0usize;
    let mut in_code = false;
    for line in text.split('\n') {
        let ll = line.chars().count();
        if line.starts_with("```") {
            in_code = !in_code;
        } else if !in_code && let Some((_, draw)) = line_list_marker(line) {
            out.push((base, draw));
        }
        base += ll + 1;
    }
    out
}

/// Render `data` as a QR code into an RGBA8 buffer: `scale` pixels per module,
/// dark modules black on white, with a 4-module quiet zone. Returns the buffer
/// and the square side in pixels. Errors if `data` is too large for a QR code.
fn qr_rgba(data: &str, scale: usize) -> anyhow::Result<(Vec<u8>, usize)> {
    const QUIET: usize = 4;
    let code = qrcode::QrCode::new(data.as_bytes())?;
    let w = code.width();
    let side = (w + QUIET * 2) * scale;
    let colors = code.to_colors();
    let mut rgba = vec![255u8; side * side * 4]; // white background
    for my in 0..w {
        for mx in 0..w {
            if colors[my * w + mx] == qrcode::Color::Dark {
                for py in 0..scale {
                    for px in 0..scale {
                        let x = (QUIET + mx) * scale + px;
                        let y = (QUIET + my) * scale + py;
                        let idx = (y * side + x) * 4;
                        rgba[idx] = 0;
                        rgba[idx + 1] = 0;
                        rgba[idx + 2] = 0;
                    }
                }
            }
        }
    }
    Ok((rgba, side))
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
/// Toggle an inline `marker` (`**`, `*`, `` ` ``, `~~`) around the char range
/// `[start, end)` of `text`. Operates on the FULL text so it detects markers
/// sitting just *outside* the selection — which is what makes un-styling work
/// when the markers are hidden in the WYSIWYG view (the user selects only the
/// visible content). Returns the new text and the new selection (char offsets),
/// placed on the content.
fn toggle_wrap(text: &str, start: usize, end: usize, marker: &str) -> (String, usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let mc: Vec<char> = marker.chars().collect();
    let m = mc.len();
    let n = chars.len();
    let start = start.min(n);
    let end = end.min(n).max(start);

    // Markers immediately outside the selection (hidden-marker case).
    let outside = start >= m
        && end + m <= n
        && chars[start - m..start] == mc[..]
        && chars[end..end + m] == mc[..];
    // Markers inside the selection (it includes them).
    let inside =
        end >= start + 2 * m && chars[start..start + m] == mc[..] && chars[end - m..end] == mc[..];

    let mut out: Vec<char> = Vec::new();
    let (ns, ne);
    if outside {
        out.extend_from_slice(&chars[..start - m]);
        out.extend_from_slice(&chars[start..end]);
        out.extend_from_slice(&chars[end + m..]);
        (ns, ne) = (start - m, end - m);
    } else if inside {
        out.extend_from_slice(&chars[..start]);
        out.extend_from_slice(&chars[start + m..end - m]);
        out.extend_from_slice(&chars[end..]);
        (ns, ne) = (start, end - 2 * m);
    } else {
        out.extend_from_slice(&chars[..start]);
        out.extend_from_slice(&mc);
        out.extend_from_slice(&chars[start..end]);
        out.extend_from_slice(&mc);
        out.extend_from_slice(&chars[end..]);
        (ns, ne) = (start + m, end + m);
    }
    (out.into_iter().collect(), ns, ne)
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

/// Char offset of the start of the line containing `pos`.
fn line_start(chars: &[char], pos: usize) -> usize {
    let mut i = pos.min(chars.len());
    while i > 0 && chars[i - 1] != '\n' {
        i -= 1;
    }
    i
}

/// Char offset of the end of the line containing `pos` (before the next `\n`).
fn line_end(chars: &[char], pos: usize) -> usize {
    let mut i = pos.min(chars.len());
    while i < chars.len() && chars[i] != '\n' {
        i += 1;
    }
    i
}

/// Apply a per-line transform to every line the char range `[start, end)` spans
/// (expanded to whole lines). Returns the new text and the selection covering
/// the transformed block. Pure core of `transform_lines`.
fn transform_block(
    text: &str,
    start: usize,
    end: usize,
    f: impl Fn(&str) -> String,
) -> (String, usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let start = start.min(n);
    let end = end.min(n).max(start);
    let bs = line_start(&chars, start);
    let be = line_end(&chars, end);
    let block: String = chars[bs..be].iter().collect();
    let new: String = block.split('\n').map(f).collect::<Vec<_>>().join("\n");
    let mut out: String = chars[..bs].iter().collect();
    out.push_str(&new);
    out.extend(chars[be..].iter());
    (out, bs, bs + new.chars().count())
}

/// Wrap the char range `[start, end)` in a fenced code block. Returns the new
/// text and a caret position on the content line. Pure core of `apply_code_block`.
fn code_block(text: &str, start: usize, end: usize) -> (String, usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let start = start.min(n);
    let end = end.min(n).max(start);
    let sel: String = chars[start..end].iter().collect();
    let inserted = format!("```\n{sel}\n```");
    let mut out: String = chars[..start].iter().collect();
    out.push_str(&inserted);
    out.extend(chars[end..].iter());
    let caret = start + 4 + sel.chars().count(); // after "```\n" + selection
    (out, caret, caret)
}

/// Insert a Markdown link around the char range `[start, end)` (its text becomes
/// the label, or "texte" if empty). Returns the new text and the selection over
/// the `url` placeholder. Pure core of `apply_link`.
fn insert_link(text: &str, start: usize, end: usize) -> (String, usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let start = start.min(n);
    let end = end.min(n).max(start);
    let sel: String = chars[start..end].iter().collect();
    let label = if sel.is_empty() {
        "texte".to_string()
    } else {
        sel
    };
    let inserted = format!("[{label}](url)");
    let mut out: String = chars[..start].iter().collect();
    out.push_str(&inserted);
    out.extend(chars[end..].iter());
    let url_start = start + 1 + label.chars().count() + 2; // after "[label]("
    (out, url_start, url_start + 3)
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

/// Full Markdown source of `b`, including the WYSIWYG-hidden marker chars.
/// Reading with `include_hidden_chars = false` drops runs covered by the
/// `invisible` tags (markers like `**`, `#`, `- `), which would corrupt the note
/// on save or rewrite it without its formatting on a toolbar action (#154).
fn buffer_source(b: &gtk::TextBuffer) -> String {
    b.text(&b.start_iter(), &b.end_iter(), true).to_string()
}

/// Wrap/unwrap the selection with an inline `marker` (e.g. `**`, `*`, `` ` ``).
fn apply_wrap(b: &gtk::TextBuffer, marker: &str) {
    let (s, e) = sel_bounds(b);
    let full = buffer_source(b);
    let (new_text, ns, ne) = toggle_wrap(&full, s.offset() as usize, e.offset() as usize, marker);
    replace_and_select(b, &new_text, ns, ne);
}

/// Replace the whole buffer with `new_text` and select `[ns, ne)` (char offsets).
/// `changed` re-runs the WYSIWYG styling.
fn replace_and_select(b: &gtk::TextBuffer, new_text: &str, ns: usize, ne: usize) {
    b.set_text(new_text);
    let a = b.iter_at_offset(ns as i32);
    let z = b.iter_at_offset(ne as i32);
    b.select_range(&a, &z);
}

/// Apply a per-line transform to every line the selection spans.
fn transform_lines(b: &gtk::TextBuffer, f: impl Fn(&str) -> String) {
    let (s, e) = sel_bounds(b);
    let full = buffer_source(b);
    let (new_text, ns, ne) = transform_block(&full, s.offset() as usize, e.offset() as usize, f);
    replace_and_select(b, &new_text, ns, ne);
}

/// Insert a fenced code block around the selection.
fn apply_code_block(b: &gtk::TextBuffer) {
    let (s, e) = sel_bounds(b);
    let full = buffer_source(b);
    let (new_text, ns, ne) = code_block(&full, s.offset() as usize, e.offset() as usize);
    replace_and_select(b, &new_text, ns, ne);
}

/// Insert a Markdown link, selecting the `url` placeholder for quick typing.
fn apply_link(b: &gtk::TextBuffer) {
    let (s, e) = sel_bounds(b);
    let full = buffer_source(b);
    let (new_text, ns, ne) = insert_link(&full, s.offset() as usize, e.offset() as usize);
    replace_and_select(b, &new_text, ns, ne);
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
/// A `GtkTextView` that draws list-item markers (bullets, numbers) in the left
/// gutter. The buffer keeps the raw Markdown (`- `/`N. `); those markers are
/// hidden by tags and the glyph is painted here, so the source round-trips and
/// editing/offsets are unaffected. Decoration is off in raw-source mode.
mod bullet_view {
    use super::list_markers;
    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use std::cell::Cell;

    mod imp {
        use super::*;

        pub struct BulletTextView {
            pub decorate: Cell<bool>,
        }

        impl Default for BulletTextView {
            fn default() -> Self {
                Self {
                    decorate: Cell::new(true),
                }
            }
        }

        #[glib::object_subclass]
        impl ObjectSubclass for BulletTextView {
            const NAME: &'static str = "PnBulletTextView";
            type Type = super::BulletTextView;
            type ParentType = gtk::TextView;
        }

        impl ObjectImpl for BulletTextView {}
        impl TextViewImpl for BulletTextView {}

        impl WidgetImpl for BulletTextView {
            fn snapshot(&self, snapshot: &gtk::Snapshot) {
                self.parent_snapshot(snapshot);
                if !self.decorate.get() {
                    return;
                }
                let view = self.obj();
                let buffer = view.buffer();
                // Include hidden chars: the markers are invisible, but iters count
                // them, so offsets must be computed over the full source.
                let text = buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), true)
                    .to_string();
                let color = view.color();
                for (offset, marker) in list_markers(&text) {
                    let iter = buffer.iter_at_offset(offset as i32);
                    let rect = view.iter_location(&iter);
                    let (wx, wy) = view.buffer_to_window_coords(
                        gtk::TextWindowType::Widget,
                        rect.x(),
                        rect.y(),
                    );
                    let layout = view.create_pango_layout(Some(&marker));
                    let (lw, _) = layout.pixel_size();
                    // Right-align the glyph in the gutter, just left of the content.
                    let x = (wx - 8 - lw).max(0);
                    snapshot.save();
                    snapshot.translate(&gtk::graphene::Point::new(x as f32, wy as f32));
                    snapshot.append_layout(&layout, &color);
                    snapshot.restore();
                }
            }
        }
    }

    glib::wrapper! {
        pub struct BulletTextView(ObjectSubclass<imp::BulletTextView>)
            @extends gtk::TextView, gtk::Widget,
            @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
    }

    impl BulletTextView {
        pub fn new() -> Self {
            glib::Object::new()
        }

        /// Toggle gutter drawing (off when showing raw Markdown source).
        pub fn set_decorate(&self, on: bool) {
            self.imp().decorate.set(on);
            self.queue_draw();
        }
    }
}
use bullet_view::BulletTextView;

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

    let text_view = BulletTextView::new();
    // WYSIWYG: proportional text; code spans get a monospace tag instead.
    text_view.set_monospace(false);
    text_view.set_wrap_mode(gtk::WrapMode::WordChar);
    text_view.set_left_margin(18);
    text_view.set_right_margin(18);
    text_view.set_top_margin(10);
    text_view.set_bottom_margin(18);
    let buffer = text_view.buffer();
    // WYSIWYG tags: style inline content and hide the Markdown markers. A pure
    // `inline_spans` computes the ranges; the buffer keeps the Markdown source.
    let tag_bold = gtk::TextTag::builder().weight(700).build();
    let tag_italic = gtk::TextTag::builder()
        .style(gtk::pango::Style::Italic)
        .build();
    let tag_code = gtk::TextTag::builder().family("monospace").build();
    let tag_strike = gtk::TextTag::builder().strikethrough(true).build();
    let tag_hidden = gtk::TextTag::builder().invisible(true).build();
    let tag_h1 = gtk::TextTag::builder().weight(800).scale(1.6).build();
    let tag_h2 = gtk::TextTag::builder().weight(800).scale(1.3).build();
    let tag_h3 = gtk::TextTag::builder().weight(800).scale(1.15).build();
    // Links: underlined, in the Adwaita accent blue. Quotes: italic + indented.
    let tag_link = gtk::TextTag::builder()
        .underline(gtk::pango::Underline::Single)
        .foreground("#3584e4")
        .build();
    let tag_quote = gtk::TextTag::builder()
        .style(gtk::pango::Style::Italic)
        .left_margin(24)
        .build();
    // Fenced code: monospace and indented so the block reads apart from prose.
    let tag_code_block = gtk::TextTag::builder()
        .family("monospace")
        .left_margin(24)
        .build();
    // List items: indent the paragraph, leaving a gutter for the drawn marker.
    let tag_list = gtk::TextTag::builder().left_margin(44).build();
    let all_tags = [
        &tag_bold,
        &tag_italic,
        &tag_code,
        &tag_strike,
        &tag_hidden,
        &tag_h1,
        &tag_h2,
        &tag_h3,
        &tag_link,
        &tag_quote,
        &tag_code_block,
        &tag_list,
    ];
    for t in all_tags {
        buffer.tag_table().add(t);
    }
    // When on, show the raw Markdown (no tags); otherwise the styled WYSIWYG view.
    let source_mode = std::rc::Rc::new(std::cell::Cell::new(false));
    let restyle: std::rc::Rc<dyn Fn()> = std::rc::Rc::new({
        let buffer = buffer.clone();
        let source_mode = source_mode.clone();
        let tags: Vec<gtk::TextTag> = all_tags.iter().map(|t| (*t).clone()).collect();
        move || {
            let start = buffer.start_iter();
            let end = buffer.end_iter();
            for t in &tags {
                buffer.remove_tag(t, &start, &end);
            }
            if source_mode.get() {
                return; // source mode: leave the Markdown markers visible
            }
            let text = buffer.text(&start, &end, false).to_string();
            for s in spans(&text) {
                let a = buffer.iter_at_offset(s.start as i32);
                let b = buffer.iter_at_offset(s.end as i32);
                let idx = match s.kind {
                    SpanKind::Bold => 0,
                    SpanKind::Italic => 1,
                    SpanKind::Code => 2,
                    SpanKind::Strike => 3,
                    SpanKind::Hidden => 4,
                    SpanKind::H1 => 5,
                    SpanKind::H2 => 6,
                    SpanKind::H3 => 7,
                    SpanKind::Link => 8,
                    SpanKind::Quote => 9,
                    SpanKind::CodeBlock => 10,
                    SpanKind::ListItem => 11,
                };
                buffer.apply_tag(&tags[idx], &a, &b);
            }
        }
    });
    let text_scroll = gtk::ScrolledWindow::builder()
        .child(&text_view)
        .vexpand(true)
        .build();

    // Footer: source toggle on the left, live word/character count on the right.
    let source_toggle = gtk::ToggleButton::with_label("Source");
    source_toggle.set_tooltip_text(Some("Afficher le Markdown brut"));
    source_toggle.add_css_class("flat");
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
    footer.append(&source_toggle);
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

    let toolbar = format_toolbar(text_view.upcast_ref::<gtk::TextView>());

    let editor = gtk::Box::new(gtk::Orientation::Vertical, 0);
    editor.append(&title);
    editor.append(&tags_scroll);
    editor.append(&atts_scroll);
    editor.append(&toolbar);
    editor.append(&text_scroll);
    editor.append(&footer);

    // Toggle raw Markdown (source) vs the styled WYSIWYG view.
    {
        let source_mode = source_mode.clone();
        let restyle = restyle.clone();
        let text_view = text_view.clone();
        source_toggle.connect_toggled(move |btn| {
            let raw = btn.is_active();
            source_mode.set(raw);
            text_view.set_decorate(!raw); // no gutter glyphs over raw markers
            restyle();
        });
    }

    // Seed content BEFORE connecting handlers so the initial load never writes
    // back (which would bump the note's `updated` timestamp).
    title.set_text(&note.title);
    buffer.set_text(&note.text);
    count_label.set_text(&count_text(&note.text));
    restyle(); // initial WYSIWYG styling of the seeded text

    let page = ui.tab_view.append(&editor);
    page.set_title(note_title(&note.title));

    // Keep the count in sync on every edit (including programmatic reloads).
    {
        let count_label = count_label.clone();
        buffer.connect_changed(move |buf| {
            count_label.set_text(&count_text(&buffer_source(buf)));
        });
    }

    // Re-apply WYSIWYG styling on every edit.
    {
        let restyle = restyle.clone();
        buffer.connect_changed(move |_| restyle());
    }

    // Follow links on Ctrl+click (plain clicks stay cursor placement, since the
    // view is editable). The link URL lives in the Markdown source at the offset.
    {
        let tv = text_view.clone();
        let buffer = buffer.clone();
        let gesture = gtk::GestureClick::new();
        gesture.set_button(gtk::gdk::BUTTON_PRIMARY);
        gesture.connect_released(move |g, _n, x, y| {
            if !g
                .current_event_state()
                .contains(gtk::gdk::ModifierType::CONTROL_MASK)
            {
                return;
            }
            let (bx, by) =
                tv.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
            if let Some(iter) = tv.iter_at_location(bx, by) {
                let text = buffer_source(&buffer);
                if let Some(url) = link_at(&text, iter.offset() as usize) {
                    let _ = gtk::gio::AppInfo::launch_default_for_uri(
                        &url,
                        None::<&gtk::gio::AppLaunchContext>,
                    );
                }
            }
        });
        text_view.add_controller(gesture);
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
            let text = buffer_source(buf);
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
        text_view: text_view.upcast(),
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

/// Run an async `remote` call on a worker runtime, delivering the result to
/// `on_done` back on the GTK main loop.
fn spawn_remote<T: Send + 'static>(
    fut: impl std::future::Future<Output = Result<T, String>> + Send + 'static,
    on_done: impl Fn(Result<T, String>) + 'static,
) {
    let (tx, rx) = async_channel::bounded::<Result<T, String>>(1);
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
        let _ = tx.send_blocking(rt.block_on(fut));
    });
    glib::spawn_future_local(async move {
        if let Ok(r) = rx.recv().await {
            on_done(r);
        }
    });
}

fn section_title(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_halign(gtk::Align::Start);
    l.add_css_class("heading");
    l.set_margin_top(6);
    l
}

/// Sync onboarding + device management: create a group, join one, or list and
/// revoke devices. Writes the same `config.json` the CLI would — elsewhere the
/// GUI only reads it.
fn open_sync_dialog(
    anchor: &impl IsA<gtk::Widget>,
    ui: &Ui,
    state: &Rc<RefCell<State>>,
    sync_label: &gtk::Label,
) {
    let window = anchor.root().and_downcast::<gtk::Window>();
    let dialog = adw::Dialog::new();
    dialog.set_title("Synchronisation");
    dialog.set_content_width(440);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 10);
    body.set_margin_top(18);
    body.set_margin_bottom(18);
    body.set_margin_start(18);
    body.set_margin_end(18);

    let header = adw::HeaderBar::new();
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&header);
    let scroll = gtk::ScrolledWindow::builder()
        .child(&body)
        .propagate_natural_height(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    tv.set_content(Some(&scroll));
    dialog.set_child(Some(&tv));

    match config::Settings::load_from(&config::config_path()).ok() {
        Some(settings) => build_enrolled_body(&body, &settings),
        None => build_unenrolled_body(&body, ui, state, sync_label, &dialog),
    }

    dialog.present(window.as_ref());
}

/// Not-enrolled view: join an existing group (paste blob) or create a new one.
fn build_unenrolled_body(
    body: &gtk::Box,
    ui: &Ui,
    state: &Rc<RefCell<State>>,
    sync_label: &gtk::Label,
    dialog: &adw::Dialog,
) {
    let intro = gtk::Label::new(Some("Cet appareil n'est pas synchronisé."));
    intro.set_halign(gtk::Align::Start);
    intro.set_wrap(true);
    body.append(&intro);

    // --- Join an existing group ---
    body.append(&section_title("Rejoindre un groupe"));
    let blob_entry = gtk::Entry::new();
    blob_entry.set_placeholder_text(Some("Coller le code d'appairage"));
    blob_entry.set_hexpand(true);
    body.append(&blob_entry);
    let join_btn = gtk::Button::with_label("Rejoindre");
    join_btn.add_css_class("suggested-action");
    join_btn.set_halign(gtk::Align::End);
    body.append(&join_btn);
    let join_status = gtk::Label::new(None);
    join_status.set_halign(gtk::Align::Start);
    join_status.add_css_class("dim-label");
    body.append(&join_status);
    {
        let ui = ui.clone();
        let state = state.clone();
        let sync_label = sync_label.clone();
        let dialog = dialog.clone();
        let blob_entry = blob_entry.clone();
        let join_status = join_status.clone();
        join_btn.connect_clicked(move |btn| {
            let blob = blob_entry.text().trim().to_string();
            if blob.is_empty() {
                join_status.set_text("Code d'appairage requis.");
                return;
            }
            btn.set_sensitive(false);
            join_status.set_text("Appairage…");
            let cfg = config::config_path();
            let ui = ui.clone();
            let state = state.clone();
            let sync_label = sync_label.clone();
            let dialog = dialog.clone();
            let btn = btn.clone();
            let join_status = join_status.clone();
            spawn_remote(
                async move { remote::pair(&cfg, &blob).await.map_err(|e| e.to_string()) },
                move |res| match res {
                    Ok(()) => {
                        start_auto_sync(&ui, &state, &sync_label);
                        dialog.close();
                    }
                    Err(e) => {
                        btn.set_sensitive(true);
                        join_status.set_text(&format!("Échec : {e}"));
                    }
                },
            );
        });
    }

    // --- Create a new group (this device becomes admin) ---
    body.append(&section_title("Créer un groupe"));
    let relay_entry = gtk::Entry::new();
    relay_entry.set_placeholder_text(Some("URL du relais (ex. https://relay.example.org)"));
    body.append(&relay_entry);
    let admin_entry = gtk::Entry::new();
    admin_entry.set_visibility(false);
    admin_entry.set_placeholder_text(Some("Jeton admin"));
    body.append(&admin_entry);
    let create_btn = gtk::Button::with_label("Créer le groupe");
    create_btn.add_css_class("suggested-action");
    create_btn.set_halign(gtk::Align::End);
    body.append(&create_btn);
    let create_status = gtk::Label::new(None);
    create_status.set_halign(gtk::Align::Start);
    create_status.add_css_class("dim-label");
    body.append(&create_status);
    {
        let ui = ui.clone();
        let state = state.clone();
        let sync_label = sync_label.clone();
        let body = body.clone();
        let relay_entry = relay_entry.clone();
        let admin_entry = admin_entry.clone();
        let create_status = create_status.clone();
        create_btn.connect_clicked(move |btn| {
            let relay = relay_entry.text().trim().to_string();
            let admin = admin_entry.text().trim().to_string();
            if relay.is_empty() || admin.is_empty() {
                create_status.set_text("URL du relais et jeton admin requis.");
                return;
            }
            btn.set_sensitive(false);
            create_status.set_text("Création…");
            let cfg = config::config_path();
            let ui = ui.clone();
            let state = state.clone();
            let sync_label = sync_label.clone();
            let body = body.clone();
            let btn = btn.clone();
            let create_status = create_status.clone();
            spawn_remote(
                async move {
                    remote::init(&cfg, &relay, &admin)
                        .await
                        .map_err(|e| e.to_string())
                },
                move |res| match res {
                    Ok(blob) => {
                        start_auto_sync(&ui, &state, &sync_label);
                        show_pairing_blob(&body, &blob);
                    }
                    Err(e) => {
                        btn.set_sensitive(true);
                        create_status.set_text(&format!("Échec : {e}"));
                    }
                },
            );
        });
    }
}

/// Replace the dialog body with the freshly-minted pairing blob: a QR to scan
/// from another device, plus the raw text to copy.
fn show_pairing_blob(body: &gtk::Box, blob: &str) {
    while let Some(child) = body.first_child() {
        body.remove(&child);
    }
    let intro = gtk::Label::new(Some(
        "Groupe créé. Scannez ce code depuis l'autre appareil, ou copiez le texte.",
    ));
    intro.set_halign(gtk::Align::Start);
    intro.set_wrap(true);
    body.append(&intro);

    if let Ok((rgba, side)) = qr_rgba(blob, 6) {
        let bytes = glib::Bytes::from_owned(rgba);
        let texture = gtk::gdk::MemoryTexture::new(
            side as i32,
            side as i32,
            gtk::gdk::MemoryFormat::R8g8b8a8,
            &bytes,
            side * 4,
        );
        let pic = gtk::Picture::for_paintable(&texture);
        pic.set_size_request(side as i32, side as i32);
        pic.set_halign(gtk::Align::Center);
        pic.set_margin_top(6);
        pic.set_margin_bottom(6);
        body.append(&pic);
    }

    let entry = gtk::Entry::new();
    entry.set_text(blob);
    entry.set_editable(false);
    entry.set_hexpand(true);
    body.append(&entry);
    let copy = gtk::Button::with_label("Copier le code");
    copy.set_halign(gtk::Align::End);
    {
        let blob = blob.to_string();
        copy.connect_clicked(move |btn| {
            btn.clipboard().set_text(&blob);
            btn.set_label("Copié");
        });
    }
    body.append(&copy);
}

/// Enrolled view: sync status plus admin device management (list + revoke).
fn build_enrolled_body(body: &gtk::Box, settings: &config::Settings) {
    let status = gtk::Label::new(Some("Cet appareil est synchronisé."));
    status.set_halign(gtk::Align::Start);
    body.append(&status);
    for line in [
        format!("Relais : {}", settings.relay_url),
        format!("Groupe : {}", settings.group_id),
    ] {
        let l = gtk::Label::new(Some(&line));
        l.set_halign(gtk::Align::Start);
        l.set_wrap(true);
        l.set_selectable(true);
        l.add_css_class("dim-label");
        body.append(&l);
    }

    body.append(&section_title("Gérer les appareils"));
    let admin_entry = gtk::Entry::new();
    admin_entry.set_visibility(false);
    admin_entry.set_placeholder_text(Some("Jeton admin"));
    body.append(&admin_entry);
    let list_btn = gtk::Button::with_label("Lister les appareils");
    list_btn.set_halign(gtk::Align::End);
    body.append(&list_btn);
    let dev_list = gtk::ListBox::new();
    dev_list.set_selection_mode(gtk::SelectionMode::None);
    dev_list.add_css_class("boxed-list");
    dev_list.set_margin_top(6);
    body.append(&dev_list);
    let dev_status = gtk::Label::new(None);
    dev_status.set_halign(gtk::Align::Start);
    dev_status.add_css_class("dim-label");
    body.append(&dev_status);

    // `refresh` re-fetches the device list; revoke buttons call it again, so it
    // is held indirectly (an Rc cell) to allow the self-reference.
    type Refresh = Rc<dyn Fn()>;
    let holder: Rc<RefCell<Option<Refresh>>> = Rc::new(RefCell::new(None));
    let refresh: Refresh = Rc::new({
        let admin_entry = admin_entry.clone();
        let dev_list = dev_list.clone();
        let dev_status = dev_status.clone();
        let holder = holder.clone();
        move || {
            let admin = admin_entry.text().trim().to_string();
            if admin.is_empty() {
                dev_status.set_text("Jeton admin requis.");
                return;
            }
            while let Some(child) = dev_list.first_child() {
                dev_list.remove(&child);
            }
            dev_status.set_text("Chargement…");
            let cfg = config::config_path();
            let dev_list = dev_list.clone();
            let dev_status = dev_status.clone();
            let holder = holder.clone();
            let admin_for_rows = admin.clone();
            spawn_remote(
                async move {
                    remote::devices(&cfg, &admin)
                        .await
                        .map_err(|e| e.to_string())
                },
                move |res| match res {
                    Ok(devices) => {
                        dev_status.set_text("");
                        for (id, active) in &devices {
                            let row = adw::ActionRow::new();
                            row.set_title(id);
                            row.set_subtitle(if *active { "actif" } else { "révoqué" });
                            if *active {
                                let rb = gtk::Button::with_label("Révoquer");
                                rb.add_css_class("destructive-action");
                                rb.set_valign(gtk::Align::Center);
                                let id = id.clone();
                                let admin = admin_for_rows.clone();
                                let holder = holder.clone();
                                let dev_status = dev_status.clone();
                                rb.connect_clicked(move |b| {
                                    b.set_sensitive(false);
                                    let cfg = config::config_path();
                                    let id = id.clone();
                                    let admin = admin.clone();
                                    let holder = holder.clone();
                                    let dev_status = dev_status.clone();
                                    spawn_remote(
                                        async move {
                                            remote::revoke(&cfg, &admin, &id)
                                                .await
                                                .map_err(|e| e.to_string())
                                        },
                                        move |r| match r {
                                            Ok(()) => {
                                                if let Some(f) = holder.borrow().as_ref() {
                                                    f();
                                                }
                                            }
                                            Err(e) => dev_status.set_text(&format!("Échec : {e}")),
                                        },
                                    );
                                });
                                row.add_suffix(&rb);
                            }
                            dev_list.append(&row);
                        }
                        if devices.is_empty() {
                            dev_status.set_text("Aucun appareil.");
                        }
                    }
                    Err(e) => dev_status.set_text(&format!("Échec : {e}")),
                },
            );
        }
    });
    *holder.borrow_mut() = Some(refresh.clone());
    list_btn.connect_clicked(move |_| refresh());
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
        let cur_body = buffer_source(&tab.buffer);
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
        Span, SpanKind, code_block, count_text, heading_level_of, heading_prefix, inline_spans,
        insert_link, link_at, parse_expanded, serialize_expanded, set_heading_line, spans,
        subtree_note_count, toggle_line_prefix, toggle_wrap, transform_block,
    };
    use std::collections::HashSet;

    fn span(start: usize, end: usize, kind: SpanKind) -> Span {
        Span { start, end, kind }
    }

    #[test]
    fn inline_spans_hides_markers_and_styles_content() {
        use SpanKind::*;
        assert_eq!(
            inline_spans("un **gras** ici"),
            vec![span(3, 5, Hidden), span(5, 9, Bold), span(9, 11, Hidden)],
        );
        assert_eq!(
            inline_spans("a *i* `c`"),
            vec![
                span(2, 3, Hidden),
                span(3, 4, Italic),
                span(4, 5, Hidden),
                span(6, 7, Hidden),
                span(7, 8, Code),
                span(8, 9, Hidden),
            ],
        );
        assert_eq!(
            inline_spans("~~x~~"),
            vec![span(0, 2, Hidden), span(2, 3, Strike), span(3, 5, Hidden)],
        );
    }

    #[test]
    fn inline_spans_stacks_bold_italic() {
        use SpanKind::*;
        assert_eq!(
            inline_spans("***x***"),
            vec![
                span(0, 3, Hidden),
                span(3, 4, Bold),
                span(3, 4, Italic),
                span(4, 7, Hidden),
            ],
        );
    }

    #[test]
    fn inline_spans_leaves_unterminated_literal() {
        assert_eq!(inline_spans("un **gras"), vec![]);
        assert_eq!(inline_spans("a * b"), vec![]);
    }

    #[test]
    fn inline_spans_hides_link_syntax_and_styles_label() {
        use SpanKind::*;
        // "voir [la doc](u)": hide "[", style "la doc", hide "](u)".
        assert_eq!(
            inline_spans("voir [la doc](u)"),
            vec![span(5, 6, Hidden), span(6, 12, Link), span(12, 16, Hidden)],
        );
    }

    #[test]
    fn inline_spans_keeps_marks_inside_link_label() {
        use SpanKind::*;
        // "[**b**](u)": label styled as a link, with its inner bold still parsed.
        assert_eq!(
            inline_spans("[**b**](u)"),
            vec![
                span(0, 1, Hidden),
                span(1, 6, Link),
                span(1, 3, Hidden),
                span(3, 4, Bold),
                span(4, 6, Hidden),
                span(6, 10, Hidden),
            ],
        );
    }

    #[test]
    fn inline_spans_leaves_malformed_link_literal() {
        // No "()" after the "]", or an empty label: nothing is hidden.
        assert_eq!(inline_spans("[x] sans"), vec![]);
        assert_eq!(inline_spans("[](u)"), vec![]);
    }

    #[test]
    fn spans_hides_quote_prefix_and_styles_content() {
        use SpanKind::*;
        // "> cité": hide "> ", style the rest as a quote, inline marks still apply.
        assert_eq!(
            spans("> cité **fort**"),
            vec![
                span(0, 2, Hidden),
                span(0, 15, Quote),
                span(7, 9, Hidden),
                span(9, 13, Bold),
                span(13, 15, Hidden),
            ],
        );
    }

    #[test]
    fn spans_hides_code_fences_and_marks_content() {
        use SpanKind::*;
        // "```\ncode\n```": fences hidden, the line between kept as CodeBlock.
        assert_eq!(
            spans("```\ncode\n```"),
            vec![
                span(0, 3, Hidden),
                span(4, 8, CodeBlock),
                span(9, 12, Hidden)
            ],
        );
        // A language spec on the opening fence is hidden with it.
        assert_eq!(
            spans("```rust\nx\n```"),
            vec![
                span(0, 7, Hidden),
                span(8, 9, CodeBlock),
                span(10, 13, Hidden)
            ],
        );
    }

    #[test]
    fn spans_code_block_suppresses_inline_and_headings() {
        use SpanKind::*;
        // Inside a fence, `#`/`**` are verbatim: no heading or bold spans.
        assert_eq!(
            spans("```\n# not a heading **x**\n```"),
            vec![
                span(0, 3, Hidden),
                span(4, 25, CodeBlock),
                span(26, 29, Hidden),
            ],
        );
    }

    #[test]
    fn link_at_returns_url_when_offset_is_in_the_label() {
        // "voir [la doc](https://x) fin": label chars are 6..12.
        let t = "voir [la doc](https://x) fin";
        assert_eq!(link_at(t, 6), Some("https://x".to_string()));
        assert_eq!(link_at(t, 11), Some("https://x".to_string()));
        // Outside the label (the `[`, the hidden url, plain text) → nothing.
        assert_eq!(link_at(t, 5), None); // on the `[`
        assert_eq!(link_at(t, 0), None); // in "voir"
        assert_eq!(link_at(t, 26), None); // in " fin"
    }

    #[test]
    fn link_at_uses_the_right_line_and_skips_code_blocks() {
        // Second line holds the link; offset is absolute across lines.
        let t = "intro\nvoir [doc](u) ici";
        assert_eq!(link_at(t, 12), Some("u".to_string())); // "doc" label
        // A link inside a fenced block is verbatim, not clickable.
        let c = "```\n[doc](u)\n```";
        assert_eq!(link_at(c, 6), None);
    }

    #[test]
    fn line_list_marker_detects_bullets_and_numbers() {
        use super::line_list_marker;
        assert_eq!(line_list_marker("- a"), Some((2, "•".to_string())));
        assert_eq!(line_list_marker("* a"), Some((2, "•".to_string())));
        assert_eq!(line_list_marker("+ a"), Some((2, "•".to_string())));
        assert_eq!(line_list_marker("3. b"), Some((3, "3.".to_string())));
        assert_eq!(line_list_marker("12. b"), Some((4, "12.".to_string())));
        // No space, wrong punctuation, or plain text → not a list.
        assert_eq!(line_list_marker("-a"), None);
        assert_eq!(line_list_marker("1.b"), None);
        assert_eq!(line_list_marker("1) b"), None);
        assert_eq!(line_list_marker("word"), None);
    }

    #[test]
    fn list_markers_uses_offsets_and_skips_code_blocks() {
        use super::list_markers;
        // A bullet on line 0, an ordered item on line 4; the fenced `- b` is
        // verbatim and yields no marker. Offsets are each line's start.
        let t = "- a\n```\n- b\n```\n2. c";
        assert_eq!(
            list_markers(t),
            vec![(0, "•".to_string()), (16, "2.".to_string())],
        );
    }

    #[test]
    fn spans_hides_list_markers_and_indents() {
        use SpanKind::*;
        assert_eq!(
            spans("- foo"),
            vec![span(0, 2, Hidden), span(0, 5, ListItem)]
        );
        assert_eq!(
            spans("2. x"),
            vec![span(0, 3, Hidden), span(0, 4, ListItem)]
        );
    }

    #[test]
    fn content_sig_is_stable_and_detects_changes() {
        // Guards the reload-diff: `maybe_reload` overwrites the buffer only when
        // the signature changes, so it must be stable when nothing changed and
        // differ on title/folder edits. Pure (no GTK), runs headless.
        use note_core::{NoteStore, ROOT_FOLDER};
        let mut d = NoteStore::new();
        let id = d.create_note(1).unwrap();
        d.set_title(&id, "One", 1).unwrap();
        let sig = super::content_sig(&d);
        assert_eq!(sig, super::content_sig(&d), "stable when unchanged");
        d.set_title(&id, "Two", 2).unwrap();
        assert_ne!(sig, super::content_sig(&d), "title edit detected");
        let sig2 = super::content_sig(&d);
        let f = d.create_folder("F", ROOT_FOLDER, 3).unwrap();
        d.move_note(&id, f.as_str(), 3).unwrap();
        assert_ne!(sig2, super::content_sig(&d), "folder move detected");
    }

    #[test]
    fn buffer_source_keeps_hidden_markers() {
        use gtk::prelude::*;
        // Regression for #154: a WYSIWYG-hidden marker (invisible tag) must still
        // be returned by buffer_source, else the note is saved/rewritten without
        // it. GTK needs a display; CI provides one via xvfb and sets
        // PN_REQUIRE_GTK so a missing display fails loudly instead of skipping.
        if gtk::init().is_err() {
            assert!(
                std::env::var_os("PN_REQUIRE_GTK").is_none(),
                "PN_REQUIRE_GTK is set but gtk::init() failed (no display?)"
            );
            return;
        }
        let b = gtk::TextBuffer::new(None);
        b.set_text("**bold**");
        let hide = gtk::TextTag::builder().invisible(true).build();
        b.tag_table().add(&hide);
        b.apply_tag(&hide, &b.iter_at_offset(0), &b.iter_at_offset(2));
        // The buggy read (include_hidden_chars = false) drops the hidden `**`.
        let stripped = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
        assert_eq!(stripped, "bold**");
        // buffer_source keeps the full source.
        assert_eq!(super::buffer_source(&b), "**bold**");
    }

    #[test]
    fn qr_rgba_has_the_right_shape_and_draws_modules() {
        let (buf, side) = super::qr_rgba("plainnote-pairing-blob", 3).unwrap();
        // Square RGBA buffer, side a multiple of the scale, at least a v1 QR
        // (21 modules) plus the 4-module quiet zone on each edge.
        assert_eq!(buf.len(), side * side * 4);
        assert_eq!(side % 3, 0);
        assert!(side >= (21 + 8) * 3);
        // The quiet zone keeps the top-left corner white; at least one dark
        // module is drawn somewhere (a finder pattern).
        assert_eq!(&buf[0..4], &[255, 255, 255, 255]);
        assert!(buf.chunks(4).any(|p| p == [0, 0, 0, 255]));
    }

    #[test]
    fn heading_prefix_detects_level_and_caps_at_3() {
        assert_eq!(heading_prefix("# Titre"), Some((1, 2)));
        assert_eq!(heading_prefix("### Sous"), Some((3, 4)));
        assert_eq!(heading_prefix("##### Deep"), Some((3, 6))); // capped at H3
        assert_eq!(heading_prefix("#pas-espace"), None);
        assert_eq!(heading_prefix("texte"), None);
    }

    #[test]
    fn spans_hides_heading_prefix_and_sizes_content() {
        use SpanKind::*;
        assert_eq!(spans("# Titre"), vec![span(0, 2, Hidden), span(2, 7, H1)],);
        // Heading content keeps its inline marks (offsets shifted past `## `).
        assert_eq!(
            spans("## a **b**"),
            vec![
                span(0, 3, Hidden),
                span(3, 10, H2),
                span(5, 7, Hidden),
                span(7, 8, Bold),
                span(8, 10, Hidden),
            ],
        );
    }

    #[test]
    fn spans_offsets_are_absolute_across_lines() {
        use SpanKind::*;
        // Line 0 "a" (no spans), line 1 "# T" starts at char offset 2.
        assert_eq!(spans("a\n# T"), vec![span(2, 4, Hidden), span(4, 5, H1)]);
    }

    #[test]
    fn spans_leaves_plain_lines_to_inline() {
        assert_eq!(spans("**x**"), inline_spans("**x**"));
    }

    #[test]
    fn inline_spans_uses_char_offsets() {
        // Accents count as one char each (offsets must be char, not byte).
        use SpanKind::*;
        assert_eq!(
            inline_spans("é **à**"),
            vec![span(2, 4, Hidden), span(4, 5, Bold), span(5, 7, Hidden)],
        );
    }

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
    fn toggle_wrap_wraps_a_selection() {
        // Select "gras" (3..7) in "un gras ici" -> bold it, selection on content.
        assert_eq!(
            toggle_wrap("un gras ici", 3, 7, "**"),
            ("un **gras** ici".to_string(), 5, 9),
        );
        // Empty selection -> insert the markers, caret between them.
        assert_eq!(toggle_wrap("ab", 1, 1, "**"), ("a****b".to_string(), 3, 3));
        assert_eq!(toggle_wrap("x", 0, 1, "`"), ("`x`".to_string(), 1, 2));
    }

    #[test]
    fn toggle_wrap_unwraps_when_markers_are_outside_the_selection() {
        // Regression: markers hidden, user selects only the visible content
        // "gras" (5..9) of "un **gras** ici". Un-bold must strip the markers and
        // leave no orphan `**` — this was the reported bug.
        assert_eq!(
            toggle_wrap("un **gras** ici", 5, 9, "**"),
            ("un gras ici".to_string(), 3, 7),
        );
    }

    #[test]
    fn toggle_wrap_unwraps_when_selection_includes_markers() {
        assert_eq!(
            toggle_wrap("un **gras** ici", 3, 11, "**"),
            ("un gras ici".to_string(), 3, 7),
        );
    }

    #[test]
    fn toggle_wrap_round_trips() {
        let (bolded, s, e) = toggle_wrap("un gras ici", 3, 7, "**");
        assert_eq!(
            toggle_wrap(&bolded, s, e, "**"),
            ("un gras ici".to_string(), 3, 7)
        );
    }

    #[test]
    fn toggle_wrap_uses_char_offsets() {
        // "é gras" — accents are one char; unwrap the already-bold "gras".
        assert_eq!(
            toggle_wrap("é **gras** ici", 4, 8, "**"),
            ("é gras ici".to_string(), 2, 6),
        );
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
    fn line_prefix_removes_ordered_and_quote_and_strips_star() {
        assert_eq!(toggle_line_prefix("1. a", "1. "), "a"); // remove ordered
        assert_eq!(toggle_line_prefix("> a", "> "), "a"); // remove quote
        assert_eq!(toggle_line_prefix("* a", "> "), "> a"); // strip `* `, add quote
        assert_eq!(toggle_line_prefix("3. a", "- "), "- a"); // strip `N. `, add bullet
    }

    #[test]
    fn heading_level_of_reads_the_level() {
        assert_eq!(heading_level_of("## Titre"), 2);
        assert_eq!(heading_level_of("###### x"), 6);
        assert_eq!(heading_level_of("#no-space"), 0);
        assert_eq!(heading_level_of("plain"), 0);
    }

    #[test]
    fn toggle_wrap_handles_italic_and_strike() {
        assert_eq!(
            toggle_wrap("un mot", 3, 6, "*"),
            ("un *mot*".to_string(), 4, 7)
        );
        assert_eq!(
            toggle_wrap("un *mot*", 4, 7, "*"),
            ("un mot".to_string(), 3, 6)
        );
        assert_eq!(
            toggle_wrap("un ~~mot~~", 5, 8, "~~"),
            ("un mot".to_string(), 3, 6)
        );
    }

    #[test]
    fn transform_block_applies_over_spanned_lines() {
        assert_eq!(
            transform_block("a\nb", 0, 3, |l| format!("- {l}")),
            ("- a\n- b".to_string(), 0, 7),
        );
        // No real selection: only the line under the caret is transformed.
        assert_eq!(
            transform_block("x\ny\nz", 2, 2, |l| format!("> {l}")),
            ("x\n> y\nz".to_string(), 2, 5),
        );
    }

    #[test]
    fn code_block_wraps_the_selection() {
        assert_eq!(code_block("hi", 0, 2), ("```\nhi\n```".to_string(), 6, 6));
        assert_eq!(code_block("", 0, 0), ("```\n\n```".to_string(), 4, 4));
    }

    #[test]
    fn insert_link_uses_label_or_placeholder() {
        assert_eq!(insert_link("", 0, 0), ("[texte](url)".to_string(), 8, 11));
        assert_eq!(insert_link("ab", 0, 2), ("[ab](url)".to_string(), 5, 8));
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
}
