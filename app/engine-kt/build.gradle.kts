// The Gobley module (ffi-contract.md "Frontend: consuming it via Gobley"):
// builds the ffi/ crate with cargo per target and generates Kotlin bindings
// from its #[uniffi::export] items. Bindings are generated at build time,
// never committed. This module is the ONLY place the app touches Rust.
import gobley.gradle.GobleyHost
import gobley.gradle.cargo.dsl.jvm
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.kotlin.multiplatform)
    alias(libs.plugins.kotlin.atomicfu)
    alias(libs.plugins.android.library)
    alias(libs.plugins.gobley.cargo)
    alias(libs.plugins.gobley.uniffi)
}

cargo {
    // The frozen backend crate — lives in the Rust workspace above app/.
    packageDirectory = layout.projectDirectory.dir("../../ffi")
    // Desktop builds ship the host's dylib only; cross-compiled JVM rust
    // libraries (linux-arm64 from Windows, etc.) need cross toolchains this
    // repo doesn't set up. Multi-platform desktop jars are a distribution
    // problem, not a walking-skeleton problem.
    builds.jvm {
        embedRustLibrary = (rustTarget == GobleyHost.current.rustTarget)
    }
}

uniffi {
    // The ffi crate uses uniffi::setup_scaffolding! (proc-macro mode), so
    // bindings come from the built library, not a UDL file.
    generateFromLibrary {
        packageName = "chat.engine"
    }
}

kotlin {
    androidTarget {
        compilerOptions {
            jvmTarget = JvmTarget.JVM_17
        }
    }
    jvmToolchain(17)
    jvm("desktop")
    // iOS targets exist only when building on a Mac (architecture.md T8 split
    // gate: Android+desktop unblock now; iOS is a timeboxed follow-up).
    if (GobleyHost.Platform.MacOS.isCurrent) {
        iosArm64()
        iosSimulatorArm64()
        iosX64()
    }

    sourceSets {
        commonTest.dependencies {
            implementation(kotlin("test"))
        }
    }
}

android {
    namespace = "chat.engine"
    compileSdk = libs.versions.android.compileSdk.get().toInt()

    defaultConfig {
        minSdk = libs.versions.android.minSdk.get().toInt()
        // arm64 covers real devices; x86_64 covers the emulator.
        ndk.abiFilters.addAll(listOf("arm64-v8a", "x86_64"))
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

java {
    toolchain {
        languageVersion = JavaLanguageVersion.of(17)
    }
}
