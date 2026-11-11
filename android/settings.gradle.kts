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
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        // GeckoView (org.mozilla.*) is published by Mozilla only: this repository serves that
        // group and nothing else, and no other repository may serve it.
        exclusiveContent {
            forRepository { maven { url = uri("https://maven.mozilla.org/maven2/") } }
            filter { includeGroupByRegex("org\\.mozilla.*") }
        }
        google()
        mavenCentral()
    }
}

rootProject.name = "krunk-android"
include(":app")
