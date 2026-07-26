//! Plain Note — command-line client (`pn`).
//!
//! Drives `core` for local note management (create, edit, organize, search) over
//! an on-disk Automerge store, and for end-to-end encrypted sync against a relay.

mod config;
mod remote;
mod store;

use anyhow::Result;
use clap::{Parser, Subcommand};
use note_core::NoteMeta;

const SHORT_ID: usize = 8;

#[derive(Parser)]
#[command(
    name = "pn",
    version,
    about = "Plain Note — local-first encrypted notes"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a note; use --edit to open $EDITOR for the body
    #[command(visible_alias = "n")]
    New {
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        folder: Option<String>,
        #[arg(long)]
        edit: bool,
    },
    /// List notes, most recently updated first
    #[command(visible_alias = "ls")]
    List {
        #[arg(long)]
        folder: Option<String>,
        #[arg(long)]
        tag: Option<String>,
    },
    /// Print a note's Markdown body
    Show { id: String },
    /// Edit a note's body in $EDITOR
    #[command(visible_alias = "e")]
    Edit { id: String },
    /// Set a note's title
    SetTitle { id: String, title: String },
    /// Set a note's folder
    SetFolder { id: String, folder: String },
    /// Add a tag to a note
    Tag { id: String, tag: String },
    /// Remove a tag from a note
    Untag { id: String, tag: String },
    /// Search titles and bodies (case-insensitive substring)
    #[command(visible_aliases = ["s", "find"])]
    Search { query: String },
    /// Delete a note
    Rm { id: String },
    /// Relay enrollment (init a group / pair a device)
    Remote {
        #[command(subcommand)]
        cmd: RemoteCmd,
    },
    /// Sync with the relay (push local, pull remote)
    Sync,
}

#[derive(Subcommand)]
enum RemoteCmd {
    /// Create a new group on the relay and enroll this device (admin)
    Init {
        #[arg(long)]
        relay: String,
        #[arg(long)]
        admin: String,
    },
    /// Join an existing group from a pairing blob
    Pair { blob: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::New {
            title,
            folder,
            edit,
        } => cmd_new(title, folder, edit),
        Command::List { folder, tag } => cmd_list(folder, tag),
        Command::Show { id } => cmd_show(&id),
        Command::Edit { id } => cmd_edit(&id),
        Command::SetTitle { id, title } => cmd_set_title(&id, &title),
        Command::SetFolder { id, folder } => cmd_set_folder(&id, &folder),
        Command::Tag { id, tag } => cmd_tag(&id, &tag),
        Command::Untag { id, tag } => cmd_untag(&id, &tag),
        Command::Search { query } => cmd_search(&query),
        Command::Rm { id } => cmd_rm(&id),
        Command::Remote { cmd } => match cmd {
            RemoteCmd::Init { relay, admin } => remote::init(&relay, &admin).await,
            RemoteCmd::Pair { blob } => remote::pair(&blob).await,
        },
        Command::Sync => remote::sync().await,
    }
}

fn cmd_new(title: Option<String>, folder: Option<String>, edit: bool) -> Result<()> {
    let mut s = store::load()?;
    let now = store::now_millis();
    let id = s.create_note(now)?;
    if let Some(t) = title {
        s.set_title(&id, &t, now)?;
    }
    if let Some(f) = folder {
        s.set_folder(&id, &f, now)?;
    }
    if edit {
        let body = store::edit_in_editor("")?;
        s.replace_text(&id, &body, store::now_millis())?;
    }
    store::save(&mut s)?;
    println!("{}", short(id.as_str()));
    Ok(())
}

fn cmd_list(folder: Option<String>, tag: Option<String>) -> Result<()> {
    let s = store::load()?;
    let mut notes = s.list()?;
    notes.retain(|n| {
        folder.as_ref().is_none_or(|f| &n.folder == f)
            && tag.as_ref().is_none_or(|t| n.tags.iter().any(|x| x == t))
    });
    notes.sort_by_key(|n| std::cmp::Reverse(n.updated));
    print_table(&notes);
    Ok(())
}

fn cmd_show(id: &str) -> Result<()> {
    let s = store::load()?;
    let id = store::resolve_id(&s, id)?;
    let note = s
        .get_note(&id)?
        .ok_or_else(|| anyhow::anyhow!("note vanished"))?;
    print!("{}", note.text);
    if !note.text.ends_with('\n') {
        println!();
    }
    Ok(())
}

fn cmd_edit(id: &str) -> Result<()> {
    let mut s = store::load()?;
    let id = store::resolve_id(&s, id)?;
    let current = s
        .get_note(&id)?
        .ok_or_else(|| anyhow::anyhow!("note vanished"))?
        .text;
    let edited = store::edit_in_editor(&current)?;
    if edited != current {
        s.replace_text(&id, &edited, store::now_millis())?;
        store::save(&mut s)?;
    }
    Ok(())
}

fn cmd_set_title(id: &str, title: &str) -> Result<()> {
    let mut s = store::load()?;
    let id = store::resolve_id(&s, id)?;
    s.set_title(&id, title, store::now_millis())?;
    store::save(&mut s)
}

fn cmd_set_folder(id: &str, folder: &str) -> Result<()> {
    let mut s = store::load()?;
    let id = store::resolve_id(&s, id)?;
    s.set_folder(&id, folder, store::now_millis())?;
    store::save(&mut s)
}

fn cmd_tag(id: &str, tag: &str) -> Result<()> {
    let mut s = store::load()?;
    let id = store::resolve_id(&s, id)?;
    s.add_tag(&id, tag, store::now_millis())?;
    store::save(&mut s)
}

fn cmd_untag(id: &str, tag: &str) -> Result<()> {
    let mut s = store::load()?;
    let id = store::resolve_id(&s, id)?;
    s.remove_tag(&id, tag, store::now_millis())?;
    store::save(&mut s)
}

fn cmd_search(query: &str) -> Result<()> {
    let s = store::load()?;
    let mut hits = s.search(query)?;
    hits.sort_by_key(|n| std::cmp::Reverse(n.updated));
    print_table(&hits);
    Ok(())
}

fn cmd_rm(id: &str) -> Result<()> {
    let mut s = store::load()?;
    let id = store::resolve_id(&s, id)?;
    s.delete_note(&id)?;
    store::save(&mut s)?;
    println!("deleted {}", short(id.as_str()));
    Ok(())
}

fn short(id: &str) -> &str {
    &id[..SHORT_ID.min(id.len())]
}

fn print_table(notes: &[NoteMeta]) {
    if notes.is_empty() {
        eprintln!("no notes");
        return;
    }
    for n in notes {
        let title = if n.title.is_empty() {
            "(untitled)"
        } else {
            &n.title
        };
        let folder = if n.folder.is_empty() {
            String::new()
        } else {
            format!("  [{}]", n.folder)
        };
        let tags = if n.tags.is_empty() {
            String::new()
        } else {
            format!("  #{}", n.tags.join(" #"))
        };
        println!("{}  {title}{folder}{tags}", short(n.id.as_str()));
    }
}
