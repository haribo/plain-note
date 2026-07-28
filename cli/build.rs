//! Bakes a build stamp into `PN_VERSION` so `pn --version` is self-describing.
//!
//! The stamp is `PN_BUILD_INFO` when set (the snapshot pipeline passes
//! `develop-<date>-<sha7>`), else the short git sha, else `dev`.

use std::process::Command;

fn main() {
    let pkg = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let info = std::env::var("PN_BUILD_INFO")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(git_short)
        .unwrap_or_else(|| "dev".to_string());
    println!("cargo:rustc-env=PN_VERSION={pkg} ({info})");
    println!("cargo:rerun-if-env-changed=PN_BUILD_INFO");
    println!("cargo:rerun-if-changed=build.rs");
}

fn git_short() -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!sha.is_empty()).then_some(sha)
}
