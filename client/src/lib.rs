//! plain-note-client — shared client logic for the CLI and GUI.
//!
//! Local persistence ([`store`]), sync configuration ([`config`]), note/folder
//! command logic ([`commands`]), and networked flows ([`remote`]: enrollment,
//! sync, attachments). Both `plain-note` (CLI) and `plain-note-gui` build on this
//! so there is one implementation of the store path, config, and sync.

pub mod commands;
pub mod config;
pub mod remote;
pub mod store;
