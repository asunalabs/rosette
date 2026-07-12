// The Kotlin Multiplatform app (frontend track, T8). Rooted at app/ so the
// Rust workspace above stays cargo-only; the seam between the two builds is
// the ffi/ crate, consumed by :engine-kt via Gobley.
rootProject.name = "chat-app"

pluginManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        mavenCentral()
    }
}

include(":engine-kt")
include(":composeApp")
