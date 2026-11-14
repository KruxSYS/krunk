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
    fun signInPrefsKeepTheEngineToTheExactOriginAndOutOfPrivateTabs() {
        for (auto in listOf(false, true)) {
            val prefs = EngineConfig.prefs(automaticUpdates = auto).toMap()
            // A stand-in entry is good for a form of exactly the tab's origin: no sibling
            // subdomain (a frame of another origin), no https twin of an http page.
            assertEquals(false, prefs["signon.includeOtherSubdomainsInLookup"])
            assertEquals(false, prefs["signon.schemeUpgrades"])
            assertEquals(false, prefs["signon.privateBrowsingCapture.enabled"])
            assertEquals(false, prefs["signon.generation.enabled"])
            assertEquals(false, prefs["signon.recipes.remoteRecipes.enabled"])
        }
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

    @Test
    fun anEarlierRemoteSettingsSyncIsRemovedAndNothingElse() {
        val files = java.nio.file.Files.createTempDirectory("krunk-engine").toFile()
        try {
            val profile = java.io.File(files, "mozilla/wns7zrb4.default")
            val idb = java.io.File(profile, "storage/permanent/chrome/idb")
            java.io.File(idb, "3870112724rsegmnoittet-es.files").mkdirs()
            java.io.File(idb, "3870112724rsegmnoittet-es.files/1").writeText("attachment")
            val store = listOf("sqlite", "sqlite-wal", "sqlite-shm").map {
                java.io.File(idb, "3870112724rsegmnoittet-es.$it")
            }
            store.forEach { it.writeText("x") }
            // What must stay: another database of the engine, a site's storage, certificate data, Krunk's own files.
            val keep = listOf(
                java.io.File(idb, "1657114595AmcateirvtiSty.sqlite"),
                java.io.File(profile, "storage/default/https+++example.com/idb/123rsegmnoittet-es.sqlite"),
                java.io.File(profile, "security_state/20261018-0-compat.filter.delta"),
                java.io.File(profile, "cert9.db"),
                java.io.File(files, "krunk/browser.db"),
                java.io.File(files, "krunk-engine.yaml"),
            )
            keep.forEach {
                it.parentFile!!.mkdirs()
                it.writeText("keep")
            }
            assertEquals(4, EngineConfig.dropRemoteSettingsStore(files))
            assertTrue(store.none { it.exists() })
            assertFalse(java.io.File(idb, "3870112724rsegmnoittet-es.files").exists())
            assertTrue(keep.all { it.exists() })
            // Nothing to do the second time, on a profile that never synced, or before a profile exists.
            assertEquals(0, EngineConfig.dropRemoteSettingsStore(files))
            assertEquals(0, EngineConfig.dropRemoteSettingsStore(java.io.File(files, "krunk")))
            assertEquals(0, EngineConfig.dropRemoteSettingsStore(java.io.File(files, "missing")))
        } finally {
            files.deleteRecursively()
        }
    }

    @Test
    fun theStoreIsClearedOncePerChangeNotAtEveryStart() {
        val files = java.nio.file.Files.createTempDirectory("krunk-engine").toFile()
        try {
            // The first start of this version (an upgrade from 0.1.x looks like this): clear.
            assertEquals(null, EngineConfig.lastStart(files))
            assertTrue(
                EngineConfig.mustDropRemoteSettingsStore(EngineConfig.lastStart(files), automaticUpdates = false),
            )
            EngineConfig.rememberStart(files, automaticUpdates = false)
            // Every later start with automatic updates off: leave the engine's files alone.
            assertFalse(
                EngineConfig.mustDropRemoteSettingsStore(EngineConfig.lastStart(files), automaticUpdates = false),
            )
            // With automatic updates on the engine may sync: never clear.
            assertFalse(
                EngineConfig.mustDropRemoteSettingsStore(EngineConfig.lastStart(files), automaticUpdates = true),
            )
            EngineConfig.rememberStart(files, automaticUpdates = true)
            assertFalse(
                EngineConfig.mustDropRemoteSettingsStore(EngineConfig.lastStart(files), automaticUpdates = true),
            )
            // Switched off again: what that sync downloaded goes at the next start, once.
            assertTrue(
                EngineConfig.mustDropRemoteSettingsStore(EngineConfig.lastStart(files), automaticUpdates = false),
            )
            EngineConfig.rememberStart(files, automaticUpdates = false)
            assertFalse(
                EngineConfig.mustDropRemoteSettingsStore(EngineConfig.lastStart(files), automaticUpdates = false),
            )
            // An unreadable note counts as "not known": clear.
            assertTrue(EngineConfig.mustDropRemoteSettingsStore("garbage", automaticUpdates = false))
        } finally {
            files.deleteRecursively()
        }
    }
}
