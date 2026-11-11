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
        // The add-on blocklist does not apply to built-in extensions, and Krunk has no others.
        "extensions.blocklist.enabled" to false,
        // Region lookup at Mozilla's location service.
        "browser.region.update.enabled" to false,
        // The gate's state at start-up.
        PREF_GATE_BLOCKER to if (automaticUpdates) "open" else "closed",
        PREF_GATE_ENGINE to if (automaticUpdates) "allow" else engineGate,
    )

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
     * What the gate does with the engine's own background requests by default. `log` until the
     * M4 review of what it would cancel on real sites (see docs/ANDROID_DEV.md).
     */
    const val ENGINE_GATE_DEFAULT = "log"
}
