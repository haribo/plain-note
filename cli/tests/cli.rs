//! End-to-end tests of the `pn` binary: argument parsing, local subcommand
//! dispatch, and output formatting. Each test uses an isolated temp store and
//! config (no relay), so only the offline command surface is exercised.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

/// `pn` wired to an isolated store/config, with `$EDITOR` unset so `--edit`
/// paths never block on an interactive editor.
fn pn(dir: &TempDir) -> Command {
    let mut c = Command::cargo_bin("pn").unwrap();
    c.env("PN_STORE", dir.path().join("store.automerge"))
        .env("PN_CONFIG", dir.path().join("config.json"))
        .env_remove("EDITOR")
        .env_remove("VISUAL");
    c
}

#[test]
fn new_prints_short_id_then_lists_and_shows() {
    let dir = TempDir::new().unwrap();
    let out = pn(&dir)
        .args(["new", "--title", "Réunion produit"])
        .assert()
        .success();
    let short = String::from_utf8(out.get_output().stdout.clone())
        .unwrap()
        .trim()
        .to_string();
    assert_eq!(short.len(), 8, "new prints an 8-char short id");

    pn(&dir)
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("Réunion produit"));

    pn(&dir).args(["show", &short]).assert().success();
}

#[test]
fn history_lists_versions_and_restore_reverts() {
    let dir = TempDir::new().unwrap();
    let out = pn(&dir).args(["new", "--title", "V1"]).assert().success();
    let id = String::from_utf8(out.get_output().stdout.clone())
        .unwrap()
        .trim()
        .to_string();
    // A second process is a distinct author, so a new version.
    pn(&dir).args(["set-title", &id, "V2"]).assert().success();

    let h = pn(&dir).args(["history", &id]).assert().success();
    let lines = String::from_utf8(h.get_output().stdout.clone()).unwrap();
    assert_eq!(lines.lines().count(), 2, "two versions: V1 then V2");

    pn(&dir)
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("V2"));

    // Restore to the oldest version (index 2) reverts the title to V1.
    pn(&dir).args(["restore", &id, "2"]).assert().success();
    pn(&dir)
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("V1"));
}

#[test]
fn restore_rejects_an_out_of_range_version() {
    let dir = TempDir::new().unwrap();
    let out = pn(&dir).args(["new", "--title", "X"]).assert().success();
    let id = String::from_utf8(out.get_output().stdout.clone())
        .unwrap()
        .trim()
        .to_string();
    pn(&dir).args(["restore", &id, "99"]).assert().failure();
}

#[test]
fn alias_n_and_ls_are_equivalent() {
    let dir = TempDir::new().unwrap();
    pn(&dir)
        .args(["n", "--title", "Via alias"])
        .assert()
        .success();
    pn(&dir)
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("Via alias"));
}

#[test]
fn search_finds_by_title_substring() {
    let dir = TempDir::new().unwrap();
    pn(&dir)
        .args(["new", "--title", "Findable note"])
        .assert()
        .success();
    // `s` is the search alias; case-insensitive substring.
    pn(&dir)
        .args(["s", "findable"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Findable note"));
}

#[test]
fn show_unknown_id_fails() {
    let dir = TempDir::new().unwrap();
    pn(&dir).args(["show", "zzzzzzzz"]).assert().failure();
}

#[test]
fn missing_subcommand_errors_and_help_succeeds() {
    let dir = TempDir::new().unwrap();
    pn(&dir).assert().failure(); // clap requires a subcommand
    pn(&dir)
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage"));
}
