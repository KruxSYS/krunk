package org.krunk.browser.engine

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class EngineConfigTest {
    @Test
    fun byDefaultRemoteSettingsSafeBrowsingAndTheConnectivityCheckAreOff() {
        val yaml = EngineConfig.yaml(automaticUpdates = false)
        assertTrue(yaml.startsWith("env:\n  MOZ_REMOTE_SETTINGS_DISABLED: \"1\"\nprefs:\n"))
        for (
        line in listOf(
            "  network.connectivity-service.enabled: false",
            "  network.captive-portal-service.enabled: false",
            "  browser.safebrowsing.provider.google5.enabled: false",
            "  browser.safebrowsing.provider.google4.updateURL: \"\"",
            "  browser.safebrowsing.provider.google4.gethashURL: \"\"",
            "  browser.safebrowsing.provider.google.updateURL: \"\"",
            "  browser.safebrowsing.provider.google.gethashURL: \"\"",
            "  browser.safebrowsing.downloads.enabled: false",
            "  browser.safebrowsing.downloads.remote.enabled: false",
            "  browser.safebrowsing.blockedURIs.enabled: false",
            "  browser.safebrowsing.passwords.enabled: false",
            "  extensions.blocklist.enabled: false",
            "  browser.region.update.enabled: false",
            "  krunk.guard.blockerUpdates: \"closed\"",
            "  krunk.guard.engineRequests: \"block\"",
        )
        ) {
            assertTrue(line, yaml.lines().contains(line))
        }
        // Nothing that slows page loads rides along (it was tried in the spike and dropped).
        assertFalse(yaml.contains("speculative"))
    }

    @Test
    fun automaticUpdatesTurnRemoteSettingsOnAndOpenTheGateButNothingElse() {
        val on = EngineConfig.yaml(automaticUpdates = true)
        assertFalse(on.contains("MOZ_REMOTE_SETTINGS_DISABLED"))
        assertFalse(on.contains("env:"))
        assertTrue(on.lines().contains("  krunk.guard.blockerUpdates: \"open\""))
        assertTrue(on.lines().contains("  krunk.guard.engineRequests: \"allow\""))
        // Safe Browsing and the connectivity check stay off in every case.
        val off = EngineConfig.prefs(automaticUpdates = false).toMap()
        val onPrefs = EngineConfig.prefs(automaticUpdates = true).toMap()
        assertEquals(off.keys, onPrefs.keys)
        for ((name, value) in off) {
            if (name.startsWith("krunk.guard.")) continue
            assertEquals(name, value, onPrefs[name])
        }
    }

    @Test
    fun theEngineGateFollowsItsArgumentOnlyWhileAutomaticUpdatesAreOff() {
        assertEquals("block", EngineConfig.prefs(false, "block").toMap()[EngineConfig.PREF_GATE_ENGINE])
        assertEquals("log", EngineConfig.prefs(false, "log").toMap()[EngineConfig.PREF_GATE_ENGINE])
        assertEquals("allow", EngineConfig.prefs(true, "block").toMap()[EngineConfig.PREF_GATE_ENGINE])
    }

    @Test
    fun theFileIsWrittenOnceAndRewrittenWhenTheSwitchChanges() {
        val dir = File(System.getProperty("java.io.tmpdir"), "krunk-engine-config-${System.nanoTime()}").apply {
            mkdirs()
        }
        try {
            val file = EngineConfig.write(dir, automaticUpdates = false)
            assertEquals(EngineConfig.yaml(false), file.readText())
            val stamp = file.lastModified()
            assertEquals(file, EngineConfig.write(dir, automaticUpdates = false))
            assertEquals(stamp, file.lastModified())
            EngineConfig.write(dir, automaticUpdates = true)
            assertEquals(EngineConfig.yaml(true), file.readText())
            assertEquals(listOf(EngineConfig.FILE_NAME), dir.list()!!.toList())
        } finally {
            dir.deleteRecursively()
        }
    }
}
