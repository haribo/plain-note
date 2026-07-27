# Android app — build & setup

The Android app (`android/`) is a Jetpack Compose UI over the Rust core, reached
through the `plain-note-mobile` UniFFI facade (see
`docs/design/mobile-bindings.md`). Kotlin holds UI only; all note logic lives in
Rust.

> **Status: unverified scaffolding.** This project was authored without an
> Android toolchain and has **not been compiled or run**. Expect to finalize
> version pins and the Gradle wrapper on first import into Android Studio.

## Prerequisites

- Android Studio (Koala or newer) with the Android SDK (API 34) and **NDK**.
- Rust toolchain with the Android targets:
  ```sh
  rustup target add aarch64-linux-android x86_64-linux-android \
      armv7-linux-androideabi i686-linux-android
  ```
- The `rust-android-gradle` plugin invokes `cargo` directly; no `cargo-ndk` is
  required, but `ANDROID_NDK_HOME` (or `ndk.dir`) must point at the installed
  NDK.

## How it fits together

1. `rust-android-gradle` cross-compiles the workspace `mobile` crate to
   `libplain_note_mobile.so` for each ABI (task `cargoBuild`), placing them under
   `app/build/rustJniLibs/android/<abi>/` so they ship in the APK.
2. The `generateUniFFIBindings` task runs the `uniffi-bindgen` binary
   (`mobile`, feature `bindgen`) against one built `.so` to emit the Kotlin
   bindings (`dev.plainnote.core`) into `app/build/generated/uniffi/`, which is
   added to the main source set.
3. The Compose app calls the generated `NoteApp` through
   `data/NoteRepository.kt`.

## Build

From `android/` after generating the Gradle wrapper (Android Studio does this on
import, or run `gradle wrapper`):

```sh
./gradlew :app:assembleDebug
```

The store file is created in the app's private `filesDir`
(`plain-note/store.automerge`).

## Known gaps to validate

- Version pins in `build.gradle.kts` / `app/build.gradle.kts` (AGP, Kotlin,
  Compose BOM, plugin) — align with what your Android Studio provides.
- Task ordering between `cargoBuild`, `generateUniFFIBindings`, and Kotlin
  compilation.
- The Gradle wrapper jar is not committed; generate it on import.

## Not yet implemented

Sync + QR pairing (needs a design note), attachments, and folders/trash/pin UI
(the facade already supports them).
