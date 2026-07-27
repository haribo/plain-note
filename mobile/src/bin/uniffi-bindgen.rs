//! Standalone UniFFI binding generator for the Android build.
//!
//! Run with `cargo run -p plain-note-mobile --features bindgen --bin
//! uniffi-bindgen -- generate --language kotlin ...` to emit the Kotlin
//! bindings consumed by the Android project.

fn main() {
    uniffi::uniffi_bindgen_main()
}
