//! Plain Note — command-line client (`pn`).
//!
//! Parses arguments, wires the clock/editor/paths, and prints. All note logic
//! lives in `commands` (pure, testable) and all networked logic in `remote`.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use plain_note_client::store::LocalStore;
use plain_note_client::{commands, config, remote, store};

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
        /// Folder id to place the note in (see `pn folder ls`)
        #[arg(long)]
        folder: Option<String>,
        #[arg(long)]
        edit: bool,
    },
    /// List notes, most recently updated first
    #[command(visible_alias = "ls")]
    List {
        /// Filter by folder id
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
    /// Move a note into a folder (omit --to for the top level)
    Mv {
        id: String,
        #[arg(long)]
        to: Option<String>,
    },
    /// Add a tag to a note
    Tag { id: String, tag: String },
    /// Remove a tag from a note
    Untag { id: String, tag: String },
    /// Search titles and bodies (case-insensitive substring)
    #[command(visible_aliases = ["s", "find"])]
    Search { query: String },
    /// Move a note to the trash
    Rm { id: String },
    /// Pin a note (surfaced first)
    Pin { id: String },
    /// Unpin a note
    Unpin { id: String },
    /// Trash: list, restore, or empty
    Trash {
        #[command(subcommand)]
        cmd: TrashCmd,
    },
    /// Attach a file to a note (encrypt + upload to the relay)
    Attach { id: String, file: PathBuf },
    /// List a note's attachments
    Attachments { id: String },
    /// Download and decrypt a note's attachment
    Fetch {
        id: String,
        attachment: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Remove an attachment reference from a note
    Detach { id: String, attachment: String },
    /// Manage folders
    Folder {
        #[command(subcommand)]
        cmd: FolderCmd,
    },
    /// Relay enrollment (init a group / pair a device)
    Remote {
        #[command(subcommand)]
        cmd: RemoteCmd,
    },
    /// Sync with the relay (push local, pull remote)
    Sync {
        /// Keep running: re-sync on local edits and periodically pull
        #[arg(long)]
        watch: bool,
    },
}

#[derive(Subcommand)]
enum FolderCmd {
    /// Create a folder (omit --parent for the top level)
    #[command(visible_alias = "n")]
    New {
        name: String,
        #[arg(long)]
        parent: Option<String>,
    },
    /// List folders as a tree
    #[command(visible_alias = "ls")]
    List,
    /// Rename a folder
    Rename { id: String, name: String },
    /// Move a folder under another (omit --to for the top level)
    Mv {
        id: String,
        #[arg(long)]
        to: Option<String>,
    },
    /// Delete a folder (its notes and subfolders move to its parent)
    Rm { id: String },
}

#[derive(Subcommand)]
enum TrashCmd {
    /// List trashed notes
    #[command(visible_alias = "ls")]
    List,
    /// Restore a trashed note
    Restore { id: String },
    /// Permanently delete all trashed notes
    Empty,
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
    /// List the devices registered in this group (admin)
    Devices {
        #[arg(long)]
        admin: String,
    },
    /// Revoke a device so it can no longer sync (admin)
    Revoke {
        device_id: String,
        #[arg(long)]
        admin: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let store = LocalStore::at_default();
    let now = store::now_millis();

    match cli.command {
        Command::New {
            title,
            folder,
            edit,
        } => {
            let body = if edit {
                Some(store::edit_in_editor("")?)
            } else {
                None
            };
            let id = commands::new_note(
                &store,
                now,
                title.as_deref(),
                folder.as_deref(),
                body.as_deref(),
            )?;
            println!("{}", short(id.as_str()));
        }
        Command::List { folder, tag } => {
            let notes = commands::list(&store, folder.as_deref(), tag.as_deref())?;
            print_notes(&store, &notes)?;
        }
        Command::Search { query } => {
            let notes = commands::search(&store, &query)?;
            print_notes(&store, &notes)?;
        }
        Command::Show { id } => {
            let note = commands::get(&store, &id)?;
            print!("{}", note.text);
            if !note.text.ends_with('\n') {
                println!();
            }
        }
        Command::Edit { id } => {
            let current = commands::get(&store, &id)?.text;
            let edited = store::edit_in_editor(&current)?;
            if edited != current {
                commands::set_body(&store, now, &id, &edited)?;
            }
        }
        Command::SetTitle { id, title } => {
            commands::set_title(&store, now, &id, &title)?;
        }
        Command::Mv { id, to } => {
            commands::move_note(&store, now, &id, to.as_deref())?;
        }
        Command::Tag { id, tag } => {
            commands::add_tag(&store, now, &id, &tag)?;
        }
        Command::Untag { id, tag } => {
            commands::remove_tag(&store, now, &id, &tag)?;
        }
        Command::Rm { id } => {
            let id = commands::trash(&store, now, &id)?;
            println!("moved {} to trash", short(id.as_str()));
        }
        Command::Pin { id } => {
            commands::set_pinned(&store, now, &id, true)?;
        }
        Command::Unpin { id } => {
            commands::set_pinned(&store, now, &id, false)?;
        }
        Command::Trash { cmd } => match cmd {
            TrashCmd::List => print_notes(&store, &commands::list_trashed(&store)?)?,
            TrashCmd::Restore { id } => {
                let id = commands::restore(&store, now, &id)?;
                println!("restored {}", short(id.as_str()));
            }
            TrashCmd::Empty => {
                let n = commands::empty_trash(&store)?;
                println!("purged {n} note(s)");
            }
        },
        Command::Attach { id, file } => {
            let aid = remote::attach(&config::config_path(), &store, now, &id, &file).await?;
            println!("attached {}", short(&aid));
        }
        Command::Attachments { id } => {
            let atts = commands::attachments(&store, &id)?;
            if atts.is_empty() {
                eprintln!("no attachments");
            }
            for (aid, name) in atts {
                println!("{}  {name}", short(&aid));
            }
        }
        Command::Fetch {
            id,
            attachment,
            out,
        } => {
            let path = remote::fetch(&config::config_path(), &store, &id, &attachment, out).await?;
            println!("wrote {}", path.display());
        }
        Command::Detach { id, attachment } => {
            let aid = commands::detach(&store, now, &id, &attachment)?;
            println!("detached {}", short(&aid));
        }
        Command::Folder { cmd } => run_folder(cmd, &store, now)?,
        Command::Remote { cmd } => run_remote(cmd).await?,
        Command::Sync { watch } => {
            if watch {
                remote::watch(&config::config_path(), &store).await?;
            } else {
                let seq = remote::sync(&config::config_path(), &store).await?;
                println!("synced (seq {seq})");
            }
        }
    }
    Ok(())
}

fn run_folder(cmd: FolderCmd, store: &LocalStore, now: note_core::Timestamp) -> Result<()> {
    match cmd {
        FolderCmd::New { name, parent } => {
            let id = commands::create_folder(store, now, &name, parent.as_deref())?;
            println!("{}", short(id.as_str()));
        }
        FolderCmd::List => {
            let rows = commands::list_folders(store)?;
            if rows.is_empty() {
                eprintln!("no folders");
            }
            for r in rows {
                println!("{}  {}", short(r.meta.id.as_str()), r.path);
            }
        }
        FolderCmd::Rename { id, name } => {
            commands::rename_folder(store, &id, &name)?;
        }
        FolderCmd::Mv { id, to } => {
            commands::move_folder(store, &id, to.as_deref())?;
        }
        FolderCmd::Rm { id } => {
            let id = commands::delete_folder(store, now, &id)?;
            println!("deleted folder {}", short(id.as_str()));
        }
    }
    Ok(())
}

async fn run_remote(cmd: RemoteCmd) -> Result<()> {
    let cfg = config::config_path();
    match cmd {
        RemoteCmd::Init { relay, admin } => {
            let blob = remote::init(&cfg, &relay, &admin).await?;
            println!("Sync initialized. Pair another device with:\n");
            println!("  pn remote pair {blob}\n");
        }
        RemoteCmd::Pair { blob } => {
            remote::pair(&cfg, &blob).await?;
            println!("Paired. Run `pn sync`.");
        }
        RemoteCmd::Devices { admin } => {
            let devices = remote::devices(&cfg, &admin).await?;
            if devices.is_empty() {
                println!("no devices");
            }
            for (id, is_self) in devices {
                let tag = if is_self { "  (this device)" } else { "" };
                println!("{id}{tag}");
            }
        }
        RemoteCmd::Revoke { device_id, admin } => {
            remote::revoke(&cfg, &admin, &device_id).await?;
            println!("revoked {device_id}");
        }
    }
    Ok(())
}

fn short(id: &str) -> &str {
    &id[..SHORT_ID.min(id.len())]
}

fn print_notes(store: &LocalStore, notes: &[note_core::NoteMeta]) -> Result<()> {
    if notes.is_empty() {
        eprintln!("no notes");
        return Ok(());
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
            format!("  [{}]", commands::folder_path(store, &n.folder)?)
        };
        let tags = if n.tags.is_empty() {
            String::new()
        } else {
            format!("  #{}", n.tags.join(" #"))
        };
        let pin = if n.pinned { "★ " } else { "" };
        println!("{}  {pin}{title}{folder}{tags}", short(n.id.as_str()));
    }
    Ok(())
}
