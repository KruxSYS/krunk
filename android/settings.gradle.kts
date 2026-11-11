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
        google()
        mavenCentral()
        // GeckoView (org.mozilla.geckoview) is published by Mozilla only.
        maven {
            url = uri("https://maven.mozilla.org/maven2/")
            content { includeGroupByRegex("org\\.mozilla.*") }
        }
    }
}

rootProject.name = "krunk-android"
include(":app")
