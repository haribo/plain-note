// Root build file. Version pins are a starting point — confirm against the
// versions your Android Studio / NDK provide (see docs/android/README.md).

plugins {
    id("com.android.application") version "8.5.2" apply false
    id("org.jetbrains.kotlin.android") version "2.0.20" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.0.20" apply false
    id("org.mozilla.rust-android-gradle.rust-android") version "0.9.6" apply false
}
