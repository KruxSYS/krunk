package org.krunk.browser.engine

import java.io.File

/**
 * What Krunk tells the engine before it starts, so that it makes no request of its own.
 *
 * Left alone, GeckoView contacts Mozilla (Remote Settings: certificate revocation data,
 * intermediate certificates, tracking lists, experiment collections; a connectivity check) and
 * Google (Safe Browsing list updates, even with Safe Browsing set to "none" in the content
 * blocking settings) within the first minute of a fresh install. Krunk 0.1.x shipped that way.
 *
 * The values go into a small YAML file that the runtime reads at every start
 * (`GeckoRuntimeSettings.Builder.configFilePath`; the engine applies the prefs to the default
 * branch, so the age of the profile does not matter). Nothing in here needs Android: the list is
 * pinned by a JVM test.
 *
 * [automaticUpdates] is the user's "Update automatically" switch. Off (the default): Remote
 * Settings is disabled as a whole, which also means no revocation-list (CRLite, OneCRL) updates
 * and no intermediate-certificate preloads. On: Remote Settings runs as Mozilla ships it, and the
 * request gate of the `krunk-guard` extension starts open for the blocker's list updates. Safe
 * Browsing and the connectivity check are off either way.
 */
object EngineConfig {
    const val FILE_NAME = "krunk-engine.yaml"

    /** Read by `krunk-guard` (gate-api.js) when it starts, before any message can reach it. */
    const val PREF_GATE_BLOCKER = "krunk.guard.blockerUpdates"
    const val PREF_GATE_ENGINE = "krunk.guard.engineRequests"

    /** Environment variables for the engine process. */
    fun env(automaticUpdates: Boolean): Map<String, String> =
        if (automaticUpdates) emptyMap() else mapOf("MOZ_REMOTE_SETTINGS_DISABLED" to "1")

    /**
     * Prefs, in the order they are written. [engineGate] is what the gate does with the engine's
     * own background requests while automatic updates are off: `block` or `log`.
     */
    fun prefs(automaticUpdates: Boolean, engineGate: String = ENGINE_GATE_DEFAULT): List<Pair<String, Any>> = listOf(
        // Connectivity and captive-portal probes (http://firefox-portal-detection.com/success.txt).
        "network.connectivity-service.enabled" to false,
        "network.captive-portal-service.enabled" to false,
        // Safe Browsing: the v5 provider updates from safebrowsing.googleapis.com on its own
        // switch; the older providers and the download checks (which send file details) follow.
        "browser.safebrowsing.provider.google5.enabled" to false,
        "browser.safebrowsing.provider.google4.updateURL" to "",
        "browser.safebrowsing.provider.google4.gethashURL" to "",
        "browser.safebrowsing.provider.google.updateURL" to "",
        "browser.safebrowsing.provider.google.gethashURL" to "",
        "browser.safebrowsing.downloads.enabled" to false,
        "browser.safebrowsing.downloads.remote.enabled" to false,
        "browser.safebrowsing.blockedURIs.enabled" to false,
        "browser.safebrowsing.passwords.enabled" to false,
        // Sign-ins (0.3.0). Krunk's vault decides what is filled and saved: the engine neither
        // asks to save in private tabs nor suggests passwords of its own or a mail mask.
        "signon.privateBrowsingCapture.enabled" to false,
        "signon.generation.enabled" to false,
        "signon.firefoxRelay.feature" to "disabled",
        // The add-on blocklist does not apply to built-in extensions, and Krunk has no others.
        "extensions.blocklist.enabled" to false,
        // Region lookup at Mozilla's location service.
        "browser.region.update.enabled" to false,
        // The gate's state at start-up.
        PREF_GATE_BLOCKER to if (automaticUpdates) "open" else "closed",
        PREF_GATE_ENGINE to if (automaticUpdates) "allow" else engineGate,
    )

    /**
     * Removes what an earlier Remote Settings sync left in the engine's profile under [filesDir]
     * (0.1.x synced at every start; so does a profile on which automatic updates were once on).
     * Called before the runtime is created when [mustDropRemoteSettingsStore] says so; returns
     * how many files and folders went.
     *
     * Why: parts of the engine read some of these collections with signature verification at
     * every start. With downloaded data in the store that means fetching the signature's
     * certificate chain from `content-signature-2.cdn.mozilla.net`, before any extension runs
     * and so before the request gate exists. It was seen on the first start after an upgrade
     * from 0.1.1. With an empty store they fall back to the data inside the app package and ask
     * nobody. Downloaded certificate data (`security_state`, the certificate database) is a
     * different store and stays.
     */
    fun dropRemoteSettingsStore(filesDir: File): Int {
        val profiles = File(filesDir, PROFILES_DIR).listFiles { f -> f.isDirectory }.orEmpty()
        val stale = profiles
            .map { File(it, REMOTE_SETTINGS_IDB_DIR) }
            .flatMap { dir -> dir.listFiles { f -> REMOTE_SETTINGS_IDB_NAME in f.name }.orEmpty().toList() }
        return stale.count { it.deleteRecursively() }
    }

    /**
     * Whether the store has to be cleared at this start: automatic updates are off now, and the
     * last start was not already one with them off ([lastStart] is what [rememberStart] wrote
     * then; null for the first start of this version, which is how an upgrade from 0.1.x looks).
     * Not at every start: the engine creates its (empty) database again each time, and taking
     * files away under it is only worth it when they can hold downloaded data.
     */
    fun mustDropRemoteSettingsStore(lastStart: String?, automaticUpdates: Boolean): Boolean =
        !automaticUpdates && lastStart != STARTED_QUIET

    /** Reads what [rememberStart] wrote at the last start; null when there is none. */
    fun lastStart(filesDir: File): String? = try {
        File(filesDir, START_MARKER).takeIf { it.isFile }?.readText()?.trim()
    } catch (_: java.io.IOException) {
        null
    }

    /** Notes for the next start whether this one ran with automatic updates. */
    fun rememberStart(filesDir: File, automaticUpdates: Boolean) {
        File(filesDir, START_MARKER).writeText(if (automaticUpdates) STARTED_UPDATING else STARTED_QUIET)
    }

    private const val START_MARKER = "krunk-engine.updates"
    private const val STARTED_QUIET = "off"
    private const val STARTED_UPDATING = "on"

    /** GeckoView keeps its profiles here, under the app's files folder. */
    private const val PROFILES_DIR = "mozilla"

    /** The engine's own (not a site's) IndexedDB databases inside a profile. */
    private const val REMOTE_SETTINGS_IDB_DIR = "storage/permanent/chrome/idb"

    /**
     * How IndexedDB spells the database "remote-settings" in its file names: the name's letters
     * taken alternately from both ends, after a number (`3870112724rsegmnoittet-es.sqlite`, its
     * `-wal` and `-shm`, and the `.files` folder).
     */
    private const val REMOTE_SETTINGS_IDB_NAME = "rsegmnoittet-es"

    /** The file's content. */
    fun yaml(automaticUpdates: Boolean, engineGate: String = ENGINE_GATE_DEFAULT): String = buildString {
        val env = env(automaticUpdates)
        if (env.isNotEmpty()) {
            append("env:\n")
            for ((name, value) in env) append("  ").append(name).append(": ").append(quote(value)).append('\n')
        }
        append("prefs:\n")
        for ((name, value) in prefs(automaticUpdates, engineGate)) {
            append("  ").append(name).append(": ")
            append(if (value is String) quote(value) else value.toString())
            append('\n')
        }
    }

    /**
     * Writes the file into [dir] (replacing it in one step) and returns it. Called before the
     * runtime is created, at every start.
     */
    fun write(dir: File, automaticUpdates: Boolean, engineGate: String = ENGINE_GATE_DEFAULT): File {
        val file = File(dir, FILE_NAME)
        val text = yaml(automaticUpdates, engineGate)
        if (file.isFile && runCatching { file.readText() }.getOrNull() == text) return file
        val tmp = File(dir, "$FILE_NAME.tmp")
        tmp.writeText(text)
        if (!tmp.renameTo(file)) {
            file.writeText(text)
            tmp.delete()
        }
        return file
    }

    private fun quote(s: String): String = "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\""

    /**
     * What the gate does with the engine's own background requests while automatic updates are
     * off: it cancels them. With the settings above the engine makes none on a fresh profile
     * (nothing was seen while browsing real sites, `.pipeline/evidence-android-0.2.0/m4`); this
     * is the second layer, for a profile that still holds data from 0.1.x and for whatever a
     * later engine version adds. Requests of pages, of Krunk's extensions and OCSP checks are
     * never touched. `log` instead of `block` only reports them (debug builds show both).
     */
    const val ENGINE_GATE_DEFAULT = "block"
}
