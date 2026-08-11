# Android app — build & setup

The Android app (`android/`) is a Jetpack Compose UI over the Rust core, reached
through the `plain-note-mobile` UniFFI facade (see
`docs/design/mobile-bindings.md`). Kotlin holds UI only; all note logic lives in
Rust.

> **Status: builds a debug APK.** `./gradlew :app:assembleDebug` produces an APK
> that bundles the Rust `libplain_note_mobile.so` (arm64-v8a + x86_64) and the
> generated UniFFI bindings. The **runtime UI** still needs validation on a real
> device or emulator.

## Prerequisites

- Android SDK (API 34), build-tools 34, and **NDK 26.3.11579264** (matched by
  `ndkVersion` in `app/build.gradle.kts`). Android Studio, or the command-line
  tools + `sdkmanager`.
- A JDK **17** (Gradle 8.9 / AGP 8.5 do not support very new JDKs).
- A **rustup**-managed Rust toolchain with the Android targets (the system/distro
  Rust cannot add cross targets):
  ```sh
  rustup target add aarch64-linux-android x86_64-linux-android
  ```
- `sdk.dir` in `android/local.properties` (git-ignored), and `ANDROID_NDK_HOME`
  pointing at the installed NDK for the `rust-android-gradle` plugin.

Add `armv7-linux-androideabi` / `i686-linux-android` (and the matching entries in
the `cargo { targets = ... }` block) to also ship 32-bit ABIs.

## How it fits together

1. `rust-android-gradle` cross-compiles the workspace `mobile` crate to
   `libplain_note_mobile.so` for each ABI (task `cargoBuild`). Because the crate
   lives in a Cargo workspace, `cargo.targetDirectory` points the plugin at the
   workspace `target/` so it copies the libraries into the APK's jniLibs.
2. The `generateUniFFIBindings` task runs the `uniffi-bindgen` binary
   (`mobile`, feature `bindgen`) against the built arm64 `.so` to emit the Kotlin
   bindings (`dev.plainnote.core`) into `app/build/generated/uniffi/`, which is
   added to the main source set.
3. The Compose app calls the generated `NoteApp` through
   `data/NoteRepository.kt`.

## Build

From `android/` (the Gradle wrapper is committed):

```sh
./gradlew :app:assembleDebug
```

The APK is written to `app/build/outputs/apk/debug/app-debug.apk`. Install it on
a device with `adb install -r <apk>`. The store file is created in the app's
private `filesDir` (`plain-note/store.automerge`).

## Testing

Two layers (see epic #97):

**Unit tests (JVM, no device)** — the pure Kotlin editor transforms. Fast, run
in CI (`android tests` job):

```sh
./gradlew :app:testDebugUnitTest
```

**Screenshot tests (JVM, Roborazzi + Robolectric)** — golden PNGs of the
read-only rendering (`DocLines`), light and dark. Deterministic, run in CI
(`verifyRoborazziDebug` also runs the unit tests above):

```sh
./gradlew :app:recordRoborazziDebug   # regenerate goldens after an intended change
./gradlew :app:verifyRoborazziDebug   # fail on any visual diff
```

Goldens live under `app/src/test/roborazzi/` and are committed.

**Compose UI tests (instrumented, on an emulator)** — the WYSIWYG editor
gestures. Not run per-PR in CI (an emulator runner is out of scope); run them
locally against a booted emulator:

```sh
# Pin the target so a physical phone connected in parallel is never touched.
ANDROID_SERIAL=emulator-5554 ./gradlew :app:connectedDebugAndroidTest
```

Results land under `app/build/reports/androidTests/connected/`.

## Sync & pairing

One-shot sync and device pairing are wired to the facade (see
`docs/design/mobile-sync.md`): the list screen's toolbar has **Synchroniser** and
**Associer** actions. Pairing currently accepts the blob as pasted text; a QR
scanner (CameraX + ML Kit) replacing the text field is the planned follow-up.

## Not yet implemented

QR scanner UI, continuous/background sync (WorkManager), attachments, and
folders/trash/pin UI (the facade already supports the data operations).
