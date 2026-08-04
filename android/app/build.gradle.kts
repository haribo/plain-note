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
    id("io.github.takahirom.roborazzi")
}

android {
    namespace = "dev.plainnote.app"
    compileSdk = 34
    ndkVersion = "26.3.11579264"

    defaultConfig {
        applicationId = "dev.plainnote.app"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
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

    // Roborazzi renders composables through Robolectric on the JVM.
    testOptions {
        unitTests {
            isIncludeAndroidResources = true
        }
    }
}

// Cross-compile the workspace `mobile` crate to Android ABIs. Produces
// libplain_note_mobile.so under app/build/rustJniLibs/android/<abi>/.
cargo {
    module = "../../mobile"
    libname = "plain_note_mobile"
    // arm64 = modern devices, x86_64 = emulator. Add "arm"/"x86" for 32-bit later.
    targets = listOf("arm64", "x86_64")
    profile = "release"
    // The crate is part of a Cargo workspace, so its build output lands in the
    // workspace target dir (repo/target), not mobile/target. Point the plugin
    // there so it copies the built .so into rustJniLibs.
    targetDirectory = rootProject.projectDir.parentFile.resolve("target").absolutePath
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
        "target/aarch64-linux-android/release/libplain_note_mobile.so",
        "--config", "mobile/uniffi.toml",
        "--language", "kotlin",
        "--out-dir", layout.buildDirectory.dir("generated/uniffi").get().asFile.absolutePath,
    )
    outputs.dir(layout.buildDirectory.dir("generated/uniffi"))
    // The built .so is not a declared input, so always regenerate — otherwise a
    // changed model would compile against stale bindings.
    outputs.upToDateWhen { false }
}

tasks.withType<KotlinCompile>().configureEach {
    dependsOn(generateUniFFIBindings)
}
tasks.named("preBuild").configure {
    dependsOn("cargoBuild")
}

dependencies {
    // Shared Compose BOM — used by main, unit-test (Roborazzi) and androidTest.
    val composeBom = platform("androidx.compose:compose-bom:2024.09.03")

    // UniFFI-generated Kotlin needs JNA (with the @aar classifier on Android).
    implementation("net.java.dev.jna:jna:5.14.0@aar")

    // JVM unit tests for the pure Kotlin editor transforms (no device, no native
    // lib: the UniFFI bindings load libplain_note_mobile.so lazily on the first
    // FFI call, and these tests only construct data classes + call transforms).
    testImplementation("junit:junit:4.13.2")
    // Deterministic coroutine testing for the ViewModel (virtual time + Main).
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.8.1")

    // Screenshot tests (L3): Roborazzi renders composables via Robolectric on the
    // JVM — deterministic golden PNGs, no device.
    testImplementation(composeBom)
    testImplementation("androidx.compose.ui:ui-test-junit4")
    testImplementation("androidx.test.ext:junit:1.2.1")
    testImplementation("org.robolectric:robolectric:4.13")
    testImplementation("io.github.takahirom.roborazzi:roborazzi:1.26.0")
    testImplementation("io.github.takahirom.roborazzi:roborazzi-compose:1.26.0")

    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.activity:activity-compose:1.9.2")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.6")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.1")
    // Periodic background sync (runs in the app process).
    implementation("androidx.work:work-runtime-ktx:2.9.1")
    // QR scanning for device pairing (self-contained, no Google Play Services).
    implementation("com.journeyapps:zxing-android-embedded:4.3.0")

    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")

    // Instrumented Compose UI tests (L2, run on an emulator, not per-PR in CI).
    androidTestImplementation(composeBom)
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}
