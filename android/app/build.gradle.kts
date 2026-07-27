// UNVERIFIED scaffolding. This wires three things that must be validated in
// Android Studio (see docs/android/README.md):
//   1. cross-compiling the `mobile` crate to Android .so via rust-android-gradle
//   2. generating the Kotlin UniFFI bindings from a built .so
//   3. the Compose app itself
// Version pins and the exact task wiring may need adjustment for your toolchain.

import org.jetbrains.kotlin.gradle.tasks.KotlinCompile

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.mozilla.rust-android-gradle.rust-android")
}

android {
    namespace = "dev.plainnote.app"
    compileSdk = 34

    defaultConfig {
        applicationId = "dev.plainnote.app"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
    }

    buildFeatures {
        compose = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }

    // The generated UniFFI bindings are compiled as ordinary sources.
    sourceSets["main"].java.srcDir(layout.buildDirectory.dir("generated/uniffi"))

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }
}

// Cross-compile the workspace `mobile` crate to Android ABIs. Produces
// libplain_note_mobile.so under app/build/rustJniLibs/android/<abi>/.
cargo {
    module = "../../mobile"
    libname = "plain_note_mobile"
    targets = listOf("arm64", "x86_64", "arm", "x86")
    profile = "release"
}

// Generate the Kotlin bindings by reading the metadata baked into one built .so.
val generateUniFFIBindings by tasks.registering(Exec::class) {
    dependsOn("cargoBuild")
    // Run cargo from the repo root (android/..).
    workingDir = rootProject.projectDir.parentFile
    commandLine(
        "cargo", "run", "-p", "plain-note-mobile", "--features", "bindgen",
        "--bin", "uniffi-bindgen", "--",
        "generate",
        "--library",
        "android/app/build/rustJniLibs/android/arm64-v8a/libplain_note_mobile.so",
        "--config", "mobile/uniffi.toml",
        "--language", "kotlin",
        "--out-dir", layout.buildDirectory.dir("generated/uniffi").get().asFile.absolutePath,
    )
    outputs.dir(layout.buildDirectory.dir("generated/uniffi"))
}

tasks.withType<KotlinCompile>().configureEach {
    dependsOn(generateUniFFIBindings)
}
tasks.named("preBuild").configure {
    dependsOn("cargoBuild")
}

dependencies {
    // UniFFI-generated Kotlin needs JNA (with the @aar classifier on Android).
    implementation("net.java.dev.jna:jna:5.14.0@aar")

    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.activity:activity-compose:1.9.2")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.6")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.1")

    val composeBom = platform("androidx.compose:compose-bom:2024.09.03")
    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
}
