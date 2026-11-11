import javax.inject.Inject
import org.gradle.process.ExecOperations
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

// The Rust side: ../scripts/build-rust.sh builds libkrunk_ffi.so for both ABIs with cargo-ndk and
// generates the Kotlin bindings. One task per variant (debug -> cargo debug profile, release ->
// cargo release profile); outputs land in build/rust/<variant>/ and are wired into the variant's
// sources through the Variant API, so ./gradlew assembleDebug is the single build command.
val srcRoot = rootDir.parentFile
val wallpapersSrc = srcRoot.resolve("ui/shared/wallpapers")
val wallpapersDst = layout.projectDirectory.dir("src/main/assets/wallpapers")

abstract class CargoBuildTask : DefaultTask() {
    @get:Inject
    abstract val execOperations: ExecOperations

    @get:Input
    abstract val profile: Property<String>

    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val ffiSources: DirectoryProperty

    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val coreSources: DirectoryProperty

    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val manifests: ConfigurableFileCollection

    @get:Internal
    abstract val srcRoot: DirectoryProperty

    @get:OutputDirectory
    abstract val jniLibsOut: DirectoryProperty

    @get:OutputDirectory
    abstract val kotlinOut: DirectoryProperty

    @TaskAction
    fun run() {
        execOperations.exec {
            workingDir = srcRoot.get().asFile
            commandLine(
                "bash",
                srcRoot.get().asFile.resolve("android/scripts/build-rust.sh").path,
                profile.get(),
                jniLibsOut.get().asFile.path,
                kotlinOut.get().asFile.path,
            )
        }
    }
}

// The ten built-in new-tab wallpapers come from the desktop UI files (not duplicated in git).
val copyWallpapers = tasks.register<Copy>("copyWallpapers") {
    group = "build"
    description = "Copies ui/shared/wallpapers into the app assets"
    from(wallpapersSrc) { include("*.webp", "wallpapers.json") }
    into(wallpapersDst)
}

androidComponents {
    onVariants { variant ->
        val profile = if (variant.buildType == "release") "release" else "debug"
        val cargo = tasks.register<CargoBuildTask>("cargoBuild${variant.name.replaceFirstChar { it.uppercase() }}") {
            group = "build"
            description = "Builds libkrunk_ffi.so (cargo-ndk, $profile) and the UniFFI Kotlin bindings"
            this.profile.set(profile)
            srcRoot.set(project.rootDir.parentFile)
            ffiSources.set(project.rootDir.parentFile.resolve("ffi/src"))
            coreSources.set(project.rootDir.parentFile.resolve("core/src"))
            manifests.from(
                project.rootDir.parentFile.resolve("ffi/Cargo.toml"),
                project.rootDir.parentFile.resolve("ffi/uniffi.toml"),
                project.rootDir.parentFile.resolve("Cargo.lock"),
            )
            jniLibsOut.set(layout.buildDirectory.dir("rust/${variant.name}/jniLibs"))
            kotlinOut.set(layout.buildDirectory.dir("rust/${variant.name}/kotlin"))
        }
        variant.sources.jniLibs?.addGeneratedSourceDirectory(cargo, CargoBuildTask::jniLibsOut)
        variant.sources.kotlin?.addGeneratedSourceDirectory(cargo, CargoBuildTask::kotlinOut)
    }
}

android {
    namespace = "org.krunk.browser"
    // GeckoView 160 and Compose 2026.09 require compiling against API 37.2 (AAR metadata);
    // targetSdk stays 35 (the brief's target; behaviour opt-ins stay at Android 15).
    compileSdk {
        version = release(37) { minorApiLevel = 2 }
    }
    buildToolsVersion = "37.0.0"

    defaultConfig {
        applicationId = "org.krunk.browser"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk {
            // Devices (arm64-v8a) and the emulator (x86_64); GeckoView's other ABIs are dropped.
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
        buildConfigField("String", "GECKOVIEW_VERSION", "\"${libs.versions.geckoview.get()}\"")
        buildConfigField("String", "SITE_URL", "\"https://krunk.209.wf\"")
    }

    splits {
        abi {
            isEnable = true
            reset()
            include("arm64-v8a", "x86_64")
            isUniversalApk = true
        }
    }

    buildTypes {
        debug {
            applicationIdSuffix = ""
        }
        release {
            // Release signing is not configured yet (round 1): the APK is signed with the debug key.
            signingConfig = signingConfigs.getByName("debug")
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    compileOptions {
        // GeckoView uses Java 17 APIs.
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    packaging {
        jniLibs {
            // GeckoView needs its libraries extracted (it dlopen()s them by path).
            useLegacyPackaging = true
        }
    }

    lint {
        abortOnError = true
        warningsAsErrors = false
        checkReleaseBuilds = false
        lintConfig = file("lint.xml")
    }

    testOptions {
        unitTests.isReturnDefaultValues = true
        unitTests.all {
            // JVM unit tests load the host build of libkrunk_ffi.so through JNA.
            it.systemProperty("jna.library.path", srcRoot.resolve("target/debug").path)
            it.maxHeapSize = "2g"
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

// ktlint (pinned CLI, Android style from .editorconfig): ./gradlew ktlintCheck / ktlintFormat.
// The Gradle plugin does not see AGP 9's built-in Kotlin source sets, so the CLI runs directly.
val ktlint: Configuration = configurations.create("ktlint")
val ktlintPaths = listOf("src/**/*.kt", "*.kts", "!build/**")

tasks.register<JavaExec>("ktlintCheck") {
    group = "verification"
    description = "Checks Kotlin style with ktlint"
    classpath = ktlint
    mainClass.set("com.pinterest.ktlint.Main")
    args = listOf("--relative") + ktlintPaths
}

tasks.register<JavaExec>("ktlintFormat") {
    group = "formatting"
    description = "Fixes Kotlin style with ktlint"
    classpath = ktlint
    mainClass.set("com.pinterest.ktlint.Main")
    args = listOf("--relative", "-F") + ktlintPaths
}

tasks.named("preBuild") {
    dependsOn(copyWallpapers)
}

// -Pkrunk.noGms=true drops GeckoView's Google Play Services FIDO dependency (WebAuthn/passkeys
// via Play Services). Tried in the A0 spike; see docs/ANDROID_PLAN.md open questions.
val noGms = providers.gradleProperty("krunk.noGms").map { it.toBoolean() }.getOrElse(false)

dependencies {
    ktlint(libs.ktlint.cli) {
        attributes { attribute(Bundling.BUNDLING_ATTRIBUTE, objects.named(Bundling.EXTERNAL)) }
    }
    implementation(libs.geckoview) {
        if (noGms) exclude(group = "com.google.android.gms")
    }
    implementation(variantOf(libs.jna) { artifactType("aar") })
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.process)
    implementation(libs.kotlinx.coroutines.android)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.graphics)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.foundation)
    implementation(libs.compose.material3)
    implementation(libs.compose.material3.window.size)
    implementation(libs.compose.material.icons.extended)
    debugImplementation(libs.compose.ui.tooling)
    debugImplementation(libs.compose.ui.test.manifest)

    testImplementation(libs.junit)
    testImplementation(libs.kotlinx.coroutines.test)

    androidTestImplementation(platform(libs.compose.bom))
    androidTestImplementation(libs.compose.ui.test.junit4)
    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(libs.androidx.test.ext.junit)
    androidTestImplementation(libs.androidx.test.espresso.core)
}
