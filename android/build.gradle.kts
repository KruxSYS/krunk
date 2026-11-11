// Krunk for Android. Versions are pinned in gradle/libs.versions.toml (and listed in
// ../android/toolchain.lock); the native core is built by app/build.gradle.kts (task cargoBuild).
plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.kotlin.compose) apply false
}
