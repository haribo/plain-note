//! Plain Note — command-line client (`pn`).
//!
//! Parses arguments, wires the clock/editor/paths, and prints. All note logic
//! lives in `commands` (pure, testable) and all networked logic in `remote`.

mod commands;
mod config;
mod remote;
mod store;

use anyhow::Result;
use clap::{Parser, Subcommand};
use note_core::NoteMeta;

use crate::store::LocalStore;

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
            print_table(&commands::list(&store, folder.as_deref(), tag.as_deref())?);
        }
        Command::Search { query } => {
            print_table(&commands::search(&store, &query)?);
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
        Command::SetFolder { id, folder } => {
            commands::set_folder(&store, now, &id, &folder)?;
        }
        Command::Tag { id, tag } => {
            commands::add_tag(&store, now, &id, &tag)?;
        }
        Command::Untag { id, tag } => {
            commands::remove_tag(&store, now, &id, &tag)?;
        }
        Command::Rm { id } => {
            let id = commands::delete(&store, &id)?;
            println!("deleted {}", short(id.as_str()));
        }
        Command::Remote { cmd } => run_remote(cmd).await?,
        Command::Sync => {
            let seq = remote::sync(&config::config_path(), &store).await?;
            println!("synced (seq {seq})");
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
