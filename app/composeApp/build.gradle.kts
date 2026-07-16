// The Compose Multiplatform UI (frontend track). Deliberately a walking
// shell: one screen proving the FFI seam end-to-end. Real screens (the
// wireframes) land in step 6, gated on DT4 (DESIGN.md tokens) — do not build
// per-screen ad-hoc styling here before the design system exists.
import org.jetbrains.compose.desktop.application.dsl.TargetFormat
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.kotlin.multiplatform)
    alias(libs.plugins.kotlin.serialization)
    alias(libs.plugins.compose.compiler)
    alias(libs.plugins.compose.multiplatform)
    alias(libs.plugins.android.application)
}

kotlin {
    androidTarget {
        compilerOptions {
            jvmTarget = JvmTarget.JVM_17
        }
    }
    jvmToolchain(17)
    jvm("desktop")

    sourceSets {
        commonMain.dependencies {
            implementation(project(":engine-kt"))
            implementation(compose.runtime)
            implementation(compose.foundation)
            implementation(compose.material3)
            implementation(compose.ui)
            implementation(libs.kotlinx.coroutines.core)
            // Directory REST client (T27 onboarding gate) — CIO is pure-Kotlin,
            // no native engine dependency needed on Android or desktop.
            implementation(libs.ktor.client.core)
            implementation(libs.ktor.client.cio)
            implementation(libs.ktor.client.content.negotiation)
            implementation(libs.ktor.serialization.kotlinx.json)
            implementation(libs.kotlinx.serialization.json)
        }
        commonTest.dependencies {
            implementation(kotlin("test"))
        }
        // DirectoryClient's error paths (ET7) are tested against a stub JDK
        // HttpServer, which is JVM-only — hence desktopTest, not commonTest.
        //
        // ET4 adds the Compose harness here for the same reason: `runComposeUiTest`
        // needs a real toolkit, and the stub server that drives these flows is the
        // JVM one already in use. The bugs this catches (CQ-1, ET8's catch block)
        // live in composable wiring that no unit test can reach.
        val desktopTest by getting {
            dependencies {
                implementation(kotlin("test"))
                @OptIn(org.jetbrains.compose.ExperimentalComposeLibrary::class)
                implementation(compose.uiTest)
                implementation(compose.desktop.currentOs)
            }
        }
        androidMain.dependencies {
            implementation(libs.androidx.activity.compose)
        }
        val desktopMain by getting {
            dependencies {
                implementation(compose.desktop.currentOs)
                implementation(libs.kotlinx.coroutines.swing)
            }
        }
    }
}

android {
    namespace = "chat.app"
    compileSdk = libs.versions.android.compileSdk.get().toInt()

    defaultConfig {
        applicationId = "chat.app"
        minSdk = libs.versions.android.minSdk.get().toInt()
        targetSdk = libs.versions.android.targetSdk.get().toInt()
        versionCode = 1
        versionName = "0.1"
        // Ship only ABIs libchat_ffi.so is built for (engine-kt). Without
        // this, JNA's all-ABI libjnidispatch.so makes the APK installable on
        // devices where the Rust library is missing — install fine, crash on
        // first FFI call.
        ndk.abiFilters.addAll(listOf("arm64-v8a", "x86_64"))
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }

    buildTypes {
        getByName("release") {
            // Placeholder signing so `assembleRelease` links; real signing is
            // a distribution decision (architecture.md Distribution section).
            signingConfig = signingConfigs.getByName("debug")
        }
    }
}

compose.desktop {
    application {
        mainClass = "chat.app.MainKt"
        nativeDistributions {
            targetFormats(TargetFormat.Msi, TargetFormat.Dmg, TargetFormat.Deb)
            packageName = "chat"
            packageVersion = "1.0.0"
        }
    }
}
