import com.android.build.api.artifact.SingleArtifact
import java.security.MessageDigest
import java.util.Properties
import java.util.zip.ZipFile
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

// uBlock Origin, the bundled ad and tracker blocker: the XPI pinned in ../ubo.lock is fetched and
// hash-checked by scripts/update-ubo.sh (a mismatch fails the build), unpacked as it is into the
// app's assets (git-ignored), and every packaged APK is compared with the XPI file by file.
// KRUNK_UBO_LOCK / KRUNK_UBO_CACHE: see the script.
val uboLock = file(System.getenv("KRUNK_UBO_LOCK") ?: rootDir.resolve("ubo.lock").path)
val uboProps = Properties().apply { uboLock.inputStream().use { load(it) } }
val uboVersion: String = uboProps.getProperty("UBO_VERSION")
val uboXpiUrl: String = uboProps.getProperty("UBO_XPI")
val uboCacheDir = System.getenv("KRUNK_UBO_CACHE") ?: "${System.getProperty("user.home")}/.cache/krunk/ubo"
val uboXpi = file("$uboCacheDir/$uboVersion/${uboXpiUrl.substringAfterLast('/')}")
val uboDst = layout.projectDirectory.dir("src/main/assets/extensions/ublock0")
val uboAssetPrefix = "assets/extensions/ublock0/"

val verifyUbo = tasks.register<Exec>("verifyUbo") {
    group = "build"
    description = "Fetches the pinned uBlock Origin XPI if needed and checks its SHA-256 against ubo.lock"
    commandLine("bash", rootDir.resolve("scripts/update-ubo.sh").path, "--check")
}

val unpackUbo = tasks.register<Sync>("unpackUbo") {
    group = "build"
    description = "Unpacks the checked uBlock Origin XPI into the app assets, unmodified"
    dependsOn(verifyUbo)
    from({ zipTree(uboXpi) })
    into(uboDst)
}

abstract class CheckUboInApkTask : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val xpi: RegularFileProperty

    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val apkDir: DirectoryProperty

    @get:Input
    abstract val prefix: Property<String>

    private fun digests(zip: ZipFile, under: String): Map<String, String> {
        val out = HashMap<String, String>()
        for (entry in zip.entries()) {
            if (entry.isDirectory || !entry.name.startsWith(under)) continue
            val md = MessageDigest.getInstance("SHA-256")
            zip.getInputStream(entry).use { input ->
                val buf = ByteArray(1 shl 16)
                while (true) {
                    val n = input.read(buf)
                    if (n < 0) break
                    md.update(buf, 0, n)
                }
            }
            out[entry.name.removePrefix(under)] = md.digest().joinToString("") { "%02x".format(it) }
        }
        return out
    }

    @TaskAction
    fun run() {
        val want = ZipFile(xpi.get().asFile).use { digests(it, "") }
        val apks = apkDir.get().asFile.listFiles { f -> f.name.endsWith(".apk") }.orEmpty().sortedBy { it.name }
        if (apks.isEmpty()) throw GradleException("checkUboInApk: no APK in ${apkDir.get().asFile}")
        for (apk in apks) {
            val have = ZipFile(apk).use { digests(it, prefix.get()) }
            val problems = ArrayList<String>()
            for ((name, sum) in want) {
                when (have[name]) {
                    null -> problems += "missing: $name"
                    sum -> Unit
                    else -> problems += "changed: $name"
                }
            }
            for (name in have.keys) if (name !in want) problems += "added: $name"
            if (problems.isNotEmpty()) {
                throw GradleException(
                    "uBlock Origin in ${apk.name} differs from the pinned XPI (${problems.size} files): " +
                        problems.sorted().take(10).joinToString("; "),
                )
            }
            logger.lifecycle("uBlock Origin: ${want.size} files in ${apk.name} are identical to the pinned XPI")
        }
    }
}

// Release signing: a properties file outside the repository (storeFile, storePassword, keyAlias,
// keyPassword), by default ~/.config/krunk/android-release.properties, or the path in
// KRUNK_ANDROID_SIGNING. Without it a release build fails, so an APK signed with the debug key is
// never mistaken for a release; -Pkrunk.allowDebugSignedRelease=true signs with the debug key on
// purpose (a local build of the minified variant).
val signingPropsPath = System.getenv("KRUNK_ANDROID_SIGNING")
    ?: "${System.getProperty("user.home")}/.config/krunk/android-release.properties"
val signingProps = file(signingPropsPath)
    .takeIf { it.isFile }
    ?.let { f -> Properties().apply { f.inputStream().use { load(it) } } }
val allowDebugSignedRelease =
    providers.gradleProperty("krunk.allowDebugSignedRelease").map { it.toBoolean() }.getOrElse(false)

// The update check (direct flavour, 0.5.0): release builds ask https://krunk.209.wf and nothing
// else. Only the qa build type (and debug builds) take another address, for the end-to-end tests
// against a staging copy of the site: -Pkrunk.qaUpdateBase=http://10.250.136.3:18090. Setting it
// while a release variant is built fails the build, so a published APK never carries one.
val releaseUpdateBase = "https://krunk.209.wf"
val qaUpdateBase: String? = providers.gradleProperty("krunk.qaUpdateBase").orNull?.takeIf { it.isNotBlank() }
// Krunk account and sync (0.6.0): release builds talk to https://krunk.209.wf only (HTTPS, the
// approval page must be on that site). The qa build type (and debug builds) take a staging stack
// for the end-to-end tests: -Pkrunk.qaSyncBase=http://10.250.136.3:18082. Like qaUpdateBase it
// fails any release build, so a published APK never carries one.
val releaseSyncBase = "https://krunk.209.wf"
val qaSyncBase: String? = providers.gradleProperty("krunk.qaSyncBase").orNull?.takeIf { it.isNotBlank() }
// The QA build of the 0.6.0 code: lower than 0.6.0 (8), installs over 0.5.0 (7) with -r.
val qaVersionCode = 7

/**
 * Sync's build fields: the site, whether plain HTTP is allowed (a staging stack only), the site
 * the approval page must be on (empty = any https page, staging only) and the request log
 * (method, path and status in logcat; never in release builds).
 */
fun com.android.build.api.dsl.ApplicationBuildType.syncFields(staging: String?) {
    buildConfigField("String", "SYNC_BASE_URL", "\"${staging ?: releaseSyncBase}\"")
    buildConfigField("boolean", "SYNC_ALLOW_HTTP", (staging != null).toString())
    buildConfigField("String", "SYNC_VERIFY_BASE", if (staging != null) "\"\"" else "\"$releaseSyncBase\"")
    buildConfigField("boolean", "SYNC_LOG", (name != "release").toString())
}

fun isReleaseTask(name: String) = Regex("(package|bundle|sign|assemble)(Direct|Play)?Release").containsMatchIn(name)
fun isSignedTask(name: String) = Regex("(package|bundle|sign)(Direct|Play)?(Release|Qa)").containsMatchIn(name)

gradle.taskGraph.whenReady {
    val mine = allTasks.filter { it.project == project }
    if (qaSyncBase != null && mine.any { isReleaseTask(it.name) }) {
        throw GradleException(
            "Krunk: -Pkrunk.qaSyncBase is only for the qa and debug builds; a release build always uses " +
                "$releaseSyncBase. Build the release variants in a separate Gradle run without it.",
        )
    }
    if (qaUpdateBase != null && mine.any { isReleaseTask(it.name) }) {
        throw GradleException(
            "Krunk: -Pkrunk.qaUpdateBase is only for the qa and debug builds; a release build always uses " +
                "$releaseUpdateBase. Build the release variants in a separate Gradle run without it.",
        )
    }
    val packagesRelease = mine.any { isSignedTask(it.name) }
    if (packagesRelease && signingProps == null && !allowDebugSignedRelease) {
        throw GradleException(
            "Krunk: no Android release signing properties at $signingPropsPath. A release APK must carry the " +
                "release key; for a local, debug-signed build of the release variant pass " +
                "-Pkrunk.allowDebugSignedRelease=true.",
        )
    }
}

androidComponents {
    onVariants { variant ->
        // One cargo build per profile, whatever the flavour: debug -> debug, release and qa ->
        // release.
        val profile = if (variant.buildType == "debug") "debug" else "release"
        val cargoName = "cargoBuild${profile.replaceFirstChar { it.uppercase() }}"
        if (variant.buildType == "qa") {
            variant.outputs.forEach { it.versionCode.set(qaVersionCode) }
        }
        val cargo = if (cargoName in tasks.names) {
            tasks.named<CargoBuildTask>(cargoName)
        } else {
            tasks.register<CargoBuildTask>(cargoName) {
                group = "build"
                description = "Builds libkrunk_ffi.so (cargo-ndk, $profile) and the UniFFI Kotlin bindings"
                this.profile.set(profile)
                srcRoot.set(project.rootDir.parentFile)
                ffiSources.set(project.rootDir.parentFile.resolve("ffi/src"))
                coreSources.set(project.rootDir.parentFile.resolve("core/src"))
                // Everything that changes the built library: manifests and profiles, the lock file,
                // the build script and its pins, and the wallpaper list compiled into the core.
                manifests.from(
                    project.rootDir.parentFile.resolve("Cargo.toml"),
                    project.rootDir.parentFile.resolve("Cargo.lock"),
                    project.rootDir.parentFile.resolve("core/Cargo.toml"),
                    project.rootDir.parentFile.resolve("core/build.rs"),
                    project.rootDir.parentFile.resolve("ffi/Cargo.toml"),
                    project.rootDir.parentFile.resolve("ffi/uniffi.toml"),
                    project.rootDir.parentFile.resolve("ui/shared/wallpapers/wallpapers.json"),
                    project.rootDir.resolve("scripts/build-rust.sh"),
                    project.rootDir.resolve("toolchain.lock"),
                )
                jniLibsOut.set(layout.buildDirectory.dir("rust/$profile/jniLibs"))
                kotlinOut.set(layout.buildDirectory.dir("rust/$profile/kotlin"))
            }
        }
        variant.sources.jniLibs?.addGeneratedSourceDirectory(cargo, CargoBuildTask::jniLibsOut)
        variant.sources.kotlin?.addGeneratedSourceDirectory(cargo, CargoBuildTask::kotlinOut)

        val variantName = variant.name.replaceFirstChar { it.uppercase() }
        val checkUbo = tasks.register<CheckUboInApkTask>("checkUboInApk$variantName") {
            group = "verification"
            description = "Compares uBlock Origin inside the ${variant.name} APKs with the pinned XPI, file by file"
            xpi.set(uboXpi)
            apkDir.set(variant.artifacts.get(SingleArtifact.APK))
            prefix.set(uboAssetPrefix)
        }
        tasks.matching { it.name == "assemble$variantName" }.configureEach { dependsOn(checkUbo) }
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

    // aapt's default pattern also drops every asset folder whose name starts with "_": that would
    // remove uBlock Origin's `_locales`. This is the default without that rule.
    androidResources {
        ignoreAssetsPattern = "!.svn:!.git:!.ds_store:!*.scc:.*:!CVS:!thumbs.db:!picasa.ini:!*~"
    }

    defaultConfig {
        applicationId = "org.krunk.browser"
        minSdk = 26
        targetSdk = 35
        versionCode = 8
        versionName = "0.6.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk {
            // Devices (arm64-v8a) and the emulator (x86_64); GeckoView's other ABIs are dropped.
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
        buildConfigField("String", "GECKOVIEW_VERSION", "\"${libs.versions.geckoview.get()}\"")
        buildConfigField("String", "SITE_URL", "\"https://krunk.209.wf\"")
        buildConfigField("String", "UBO_VERSION", "\"$uboVersion\"")
        buildConfigField("String", "UBO_ID", "\"${uboProps.getProperty("UBO_ID")}\"")
        buildConfigField("String", "UBO_SOURCE_URL", "\"$uboXpiUrl\"")
    }

    splits {
        abi {
            // APK splits for the website APKs. AGP 9 refuses them while an App Bundle is built
            // (issuetracker 402800800); the bundle makes its own per-ABI splits (bundle {} below),
            // so they are off in a Gradle run that builds a bundle.
            isEnable = gradle.startParameter.taskNames.none { it.contains("bundle", ignoreCase = true) }
            reset()
            include("arm64-v8a", "x86_64")
            isUniversalApk = true
        }
    }

    // Two channels with the same package and key (0.5.0): "direct" is the website APK with the
    // signed update check (src/direct: the code, the screen, REQUEST_INSTALL_PACKAGES); "play" is
    // for Google Play, which updates the app itself (src/play: no update code at all).
    flavorDimensions += "channel"
    productFlavors {
        create("direct") {
            dimension = "channel"
            buildConfigField("String", "CHANNEL", "\"direct\"")
        }
        create("play") {
            dimension = "channel"
            buildConfigField("String", "CHANNEL", "\"play\"")
        }
    }

    // The App Bundle for Play: per-ABI splits (the default, stated for the reader).
    bundle {
        abi {
            enableSplit = true
        }
    }

    // Release signing: see signingProps above.
    signingConfigs {
        if (signingProps != null) {
            create("release") {
                storeFile = file(signingProps.getProperty("storeFile"))
                storePassword = signingProps.getProperty("storePassword")
                keyAlias = signingProps.getProperty("keyAlias")
                keyPassword = signingProps.getProperty("keyPassword")
                enableV2Signing = true
                enableV3Signing = true
            }
        }
    }

    buildTypes {
        debug {
            applicationIdSuffix = ""
            buildConfigField("String", "UPDATE_BASE_URL", "\"${qaUpdateBase ?: releaseUpdateBase}\"")
            buildConfigField("boolean", "UPDATE_ALLOW_HTTP", "true")
            syncFields(qaSyncBase)
        }
        release {
            signingConfig = if (signingProps != null) {
                signingConfigs.getByName("release")
            } else {
                // Only reached with -Pkrunk.allowDebugSignedRelease=true (the task graph check above).
                signingConfigs.getByName("debug")
            }
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            buildConfigField("String", "UPDATE_BASE_URL", "\"$releaseUpdateBase\"")
            buildConfigField("boolean", "UPDATE_ALLOW_HTTP", "false")
            syncFields(null)
        }
        // The release build of the same code with a lower versionCode and, for the end-to-end
        // update tests only, another update address. Never published.
        create("qa") {
            initWith(getByName("release"))
            versionNameSuffix = "-qa"
            matchingFallbacks += "release"
            buildConfigField("String", "UPDATE_BASE_URL", "\"${qaUpdateBase ?: releaseUpdateBase}\"")
            buildConfigField("boolean", "UPDATE_ALLOW_HTTP", "true")
            syncFields(qaSyncBase)
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
    dependsOn(copyWallpapers, unpackUbo)
}

// The krunk-guard extension's rewriter (which requests are cleaned of tracking parameters, the
// cap, the time limit) is plain JavaScript without browser APIs: its tests run with node and are
// part of the unit tests.
val guardJsTest = tasks.register<Exec>("guardJsTest") {
    group = "verification"
    description = "Runs the tests of the krunk-guard rewriter with node"
    inputs.dir("src/main/assets/extensions/krunk-guard")
    inputs.dir("src/test/js")
    commandLine("node", "--test", "src/test/js/rewriter.test.js")
}

// The same for the plain part of the krunk-sitedata extension (which site a cookie or stored
// data is counted for).
val siteDataJsTest = tasks.register<Exec>("siteDataJsTest") {
    group = "verification"
    description = "Runs the tests of the krunk-sitedata extension's plain part with node"
    inputs.dir("src/main/assets/extensions/krunk-sitedata")
    inputs.file("src/test/js/sitedata.test.js")
    commandLine("node", "--test", "src/test/js/sitedata.test.js")
}

tasks.matching { it.name.startsWith("test") && it.name.endsWith("UnitTest") }.configureEach {
    dependsOn(guardJsTest, siteDataJsTest)
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
    // The JNA AAR has no desktop native library: the plain jar gives the test JVM libjnidispatch.so.
    testImplementation(libs.jna)
    testImplementation(libs.kotlinx.coroutines.test)

    androidTestImplementation(platform(libs.compose.bom))
    androidTestImplementation(libs.compose.ui.test.junit4)
    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(libs.androidx.test.ext.junit)
    androidTestImplementation(libs.androidx.test.espresso.core)
}
