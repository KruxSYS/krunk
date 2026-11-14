package org.krunk.browser.engine.permissions

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.browser.engine.permissions.PermissionPolicy.Decision
import org.krunk.ffi.KrunkStore

class PermissionPolicyTest {
    private class MapStore : PermissionStore {
        val rows = HashMap<Pair<String, String>, Boolean>()

        override fun get(origin: String, kind: String) = rows[origin to kind]

        override fun set(origin: String, kind: String, allow: Boolean) {
            rows[origin to kind] = allow
        }

        override fun remove(origin: String, kind: String) {
            rows.remove(origin to kind)
        }
    }

    private val site = "https://a.test"
    private val tab = 1u
    private val store = MapStore()
    private val policy = PermissionPolicy(store)

    private fun decide(kind: PermissionKind, private: Boolean = false, origin: String? = site, tabId: UInt = tab) =
        policy.decide(tabId, origin, private, listOf(kind))

    @Test
    fun everyTypeHasItsHandling() {
        val expected = mapOf(
            PermissionKind.CAMERA to PermissionDefault.ASK,
            PermissionKind.MICROPHONE to PermissionDefault.ASK,
            PermissionKind.GEOLOCATION to PermissionDefault.ASK,
            PermissionKind.NOTIFICATIONS to PermissionDefault.ASK,
            PermissionKind.PERSISTENT_STORAGE to PermissionDefault.ASK,
            PermissionKind.DRM to PermissionDefault.ASK,
            PermissionKind.STORAGE_ACCESS to PermissionDefault.ASK,
            PermissionKind.LOCAL_DEVICE to PermissionDefault.ASK,
            PermissionKind.LOCAL_NETWORK to PermissionDefault.ASK,
            PermissionKind.XR to PermissionDefault.DENY,
            PermissionKind.AUTOPLAY_AUDIBLE to PermissionDefault.DENY,
            PermissionKind.AUTOPLAY_INAUDIBLE to PermissionDefault.ALLOW,
        )
        assertEquals(PermissionKind.entries.toSet(), expected.keys)
        for ((kind, default) in expected) {
            val want = when (default) {
                PermissionDefault.ASK -> Decision.Ask(listOf(kind))
                PermissionDefault.ALLOW -> Decision.Allow
                PermissionDefault.DENY -> Decision.Deny(stored = false)
            }
            assertEquals(kind.name, want, decide(kind))
        }
    }

    @Test
    fun theEnginesTypesMapToKindsAndTrackingIsNone() {
        // PermissionDelegate.PERMISSION_* of GeckoView 160, by number.
        val byNumber = mapOf(
            0 to "geolocation",
            1 to "notifications",
            2 to "persistentStorage",
            3 to "xr",
            4 to "autoplayInaudible",
            5 to "autoplayAudible",
            6 to "drm",
            8 to "storageAccess",
            9 to "localDevice",
            10 to "localNetwork",
        )
        for ((number, key) in byNumber) assertEquals(key, PermissionKind.ofGecko(number)?.key)
        assertNull(PermissionKind.ofGecko(PermissionKind.GECKO_TRACKING))
        assertNull(PermissionKind.ofGecko(-1))
        assertNull(PermissionKind.ofGecko(11))
        assertEquals(PermissionKind.CAMERA, PermissionKind.ofKey("camera"))
        assertEquals(PermissionKind.MICROPHONE, PermissionKind.ofKey("microphone"))
        assertNull(PermissionKind.ofKey("sound"))
        assertEquals(PermissionKind.entries.size, PermissionKind.entries.map { it.key }.toSet().size)
    }

    @Test
    fun aStoredAnswerShortCircuits() {
        store.set(site, "geolocation", true)
        store.set(site, "notifications", false)
        assertEquals(Decision.Allow, decide(PermissionKind.GEOLOCATION))
        assertEquals(Decision.Deny(stored = true), decide(PermissionKind.NOTIFICATIONS))
        // Another origin (also another port or scheme) has its own answers.
        assertEquals(
            Decision.Ask(listOf(PermissionKind.GEOLOCATION)),
            decide(PermissionKind.GEOLOCATION, origin = "http://a.test"),
        )
        // The defaults can be overruled per site, both ways.
        store.set(site, "autoplayAudible", true)
        store.set(site, "autoplayInaudible", false)
        store.set(site, "xr", true)
        assertEquals(Decision.Allow, decide(PermissionKind.AUTOPLAY_AUDIBLE))
        assertEquals(Decision.Deny(stored = true), decide(PermissionKind.AUTOPLAY_INAUDIBLE))
        assertEquals(Decision.Allow, decide(PermissionKind.XR))
    }

    @Test
    fun askResetsToAPrompt() {
        policy.answered(tab, site, false, listOf(PermissionKind.CAMERA), allow = true, remember = true)
        assertEquals(Decision.Allow, decide(PermissionKind.CAMERA))
        store.remove(site, "camera")
        policy.changedByUser()
        assertEquals(Decision.Ask(listOf(PermissionKind.CAMERA)), decide(PermissionKind.CAMERA))
        // The same after a remembered deny, which also counted for the page load.
        policy.answered(tab, site, false, listOf(PermissionKind.CAMERA), allow = false, remember = true)
        assertEquals(Decision.Deny(stored = true), decide(PermissionKind.CAMERA))
        store.remove(site, "camera")
        policy.changedByUser()
        assertEquals(Decision.Ask(listOf(PermissionKind.CAMERA)), decide(PermissionKind.CAMERA))
    }

    @Test
    fun aChangeOnTheScreenCountsAtTheNextRequest() {
        store.set(site, "geolocation", true)
        assertEquals(Decision.Allow, decide(PermissionKind.GEOLOCATION))
        store.set(site, "geolocation", false)
        policy.changedByUser()
        assertEquals(Decision.Deny(stored = true), decide(PermissionKind.GEOLOCATION))
    }

    @Test
    fun rememberStoresAndNotRememberingDoesNot() {
        policy.answered(tab, site, false, listOf(PermissionKind.GEOLOCATION), allow = true, remember = false)
        assertTrue(store.rows.isEmpty())
        assertEquals(Decision.Ask(listOf(PermissionKind.GEOLOCATION)), decide(PermissionKind.GEOLOCATION))
        policy.answered(tab, site, false, listOf(PermissionKind.GEOLOCATION), allow = true, remember = true)
        assertEquals(mapOf((site to "geolocation") to true), store.rows)
        assertTrue(policy.isRow(site, false, PermissionKind.GEOLOCATION))
        assertFalse(policy.isDenyRow(site, false, PermissionKind.GEOLOCATION))
    }

    @Test
    fun aDenyIsNotAskedAgainUntilTheNextDocument() {
        // A dismissed prompt: deny, not remembered.
        policy.answered(tab, site, false, listOf(PermissionKind.NOTIFICATIONS), allow = false, remember = false)
        assertTrue(store.rows.isEmpty())
        assertEquals(Decision.Deny(stored = false), decide(PermissionKind.NOTIFICATIONS))
        // Other kinds and other tabs are not affected.
        assertEquals(Decision.Ask(listOf(PermissionKind.CAMERA)), decide(PermissionKind.CAMERA))
        assertEquals(
            Decision.Ask(listOf(PermissionKind.NOTIFICATIONS)),
            decide(PermissionKind.NOTIFICATIONS, tabId = 2u),
        )
        policy.newDocument(tab)
        assertEquals(Decision.Ask(listOf(PermissionKind.NOTIFICATIONS)), decide(PermissionKind.NOTIFICATIONS))
    }

    @Test
    fun androidRefusingCountsForThePageEvenOverAStoredAllow() {
        store.set(site, "camera", true)
        policy.deniedForNow(tab, listOf(PermissionKind.CAMERA))
        assertEquals(Decision.Deny(stored = false), decide(PermissionKind.CAMERA))
        policy.newDocument(tab)
        assertEquals(Decision.Allow, decide(PermissionKind.CAMERA))
        policy.deniedForNow(tab, listOf(PermissionKind.CAMERA))
        policy.tabClosed(tab)
        assertEquals(Decision.Allow, decide(PermissionKind.CAMERA))
    }

    @Test
    fun aPrivateTabNeverStoresAndForgetsAtTheEndOfTheSession() {
        // What is stored for normal tabs is not used in a private one.
        store.set(site, "camera", true)
        assertEquals(Decision.Ask(listOf(PermissionKind.CAMERA)), decide(PermissionKind.CAMERA, private = true))
        val before = HashMap(store.rows)
        policy.answered(tab, site, true, listOf(PermissionKind.GEOLOCATION), allow = true, remember = true)
        policy.answered(tab, site, true, listOf(PermissionKind.NOTIFICATIONS), allow = false, remember = true)
        assertEquals(before, store.rows)
        assertEquals(2, policy.privateAnswerCount)
        assertEquals(Decision.Allow, decide(PermissionKind.GEOLOCATION, private = true))
        assertEquals(Decision.Deny(stored = true), decide(PermissionKind.NOTIFICATIONS, private = true))
        // The engine's copy of a private answer is never one of the table's rows.
        assertFalse(policy.isRow(site, true, PermissionKind.GEOLOCATION))
        assertFalse(policy.isDenyRow(site, true, PermissionKind.NOTIFICATIONS))
        // Nothing of it reaches a normal tab.
        assertEquals(Decision.Ask(listOf(PermissionKind.GEOLOCATION)), decide(PermissionKind.GEOLOCATION, tabId = 2u))
        // It outlives a reload within the session, and ends with the session.
        policy.newDocument(tab)
        assertEquals(Decision.Allow, decide(PermissionKind.GEOLOCATION, private = true))
        policy.privateSessionEnded()
        assertEquals(0, policy.privateAnswerCount)
        assertEquals(
            Decision.Ask(listOf(PermissionKind.GEOLOCATION)),
            decide(PermissionKind.GEOLOCATION, private = true),
        )
        assertEquals(
            Decision.Ask(listOf(PermissionKind.NOTIFICATIONS)),
            decide(PermissionKind.NOTIFICATIONS, private = true),
        )
        assertEquals(before, store.rows)
    }

    @Test
    fun cameraAndMicrophoneTogether() {
        val both = listOf(PermissionKind.CAMERA, PermissionKind.MICROPHONE)
        assertEquals(Decision.Ask(both), policy.decide(tab, site, false, both))
        store.set(site, "camera", true)
        // Only what has no answer yet is asked for.
        assertEquals(Decision.Ask(listOf(PermissionKind.MICROPHONE)), policy.decide(tab, site, false, both))
        store.set(site, "microphone", false)
        assertEquals(Decision.Deny(stored = true), policy.decide(tab, site, false, both))
        store.set(site, "microphone", true)
        assertEquals(Decision.Allow, policy.decide(tab, site, false, both))
    }

    @Test
    fun aPageWithoutAWebOriginIsAskedEveryTime() {
        assertFalse(policy.canRemember(null))
        assertTrue(policy.canRemember(site))
        policy.answered(tab, null, false, listOf(PermissionKind.CAMERA), allow = true, remember = true)
        assertTrue(store.rows.isEmpty())
        assertEquals(Decision.Ask(listOf(PermissionKind.CAMERA)), decide(PermissionKind.CAMERA, origin = null))
    }

    @Test
    fun anAllowForStorageAccessIsNotRemembered() {
        policy.answered(tab, site, false, listOf(PermissionKind.STORAGE_ACCESS), allow = true, remember = true)
        assertTrue(store.rows.isEmpty())
        policy.answered(tab, site, false, listOf(PermissionKind.STORAGE_ACCESS), allow = false, remember = true)
        assertEquals(mapOf((site to "storageAccess") to false), store.rows)
    }

    @Test
    fun theSharedTableIsTheStore() {
        val krunk = KrunkStore.inMemory()
        try {
            val table = TablePermissionStore(krunk) { 1_000L }
            val onTable = PermissionPolicy(table)
            for (kind in PermissionKind.entries) {
                if (!kind.rememberAllow) continue
                onTable.answered(tab, "https://a.test", false, listOf(kind), allow = true, remember = true)
            }
            onTable.answered(tab, "https://b.test:8443", false, listOf(PermissionKind.NOTIFICATIONS), false, true)
            // A row of a kind this version does not know (Windows) is listed and left alone.
            krunk.sitePermissionSet("https://a.test/", "sound", false, 5)
            val rows = table.list()
            assertEquals(PermissionKind.entries.count { it.rememberAllow } + 2, rows.size)
            assertTrue(rows.any { it.origin == "https://a.test" && it.kind == "sound" && !it.allow })
            assertEquals(true, krunk.sitePermission("https://a.test/any/page", "camera"))
            assertEquals(Decision.Allow, onTable.decide(2u, "https://a.test", false, listOf(PermissionKind.CAMERA)))
            assertEquals(
                Decision.Deny(stored = true),
                onTable.decide(2u, "https://b.test:8443", false, listOf(PermissionKind.NOTIFICATIONS)),
            )
            // "Ask" (no row) and "Reset permissions".
            table.remove("https://a.test", "camera")
            assertEquals(
                Decision.Ask(listOf(PermissionKind.CAMERA)),
                onTable.decide(2u, "https://a.test", false, listOf(PermissionKind.CAMERA)),
            )
            table.clearOrigin("https://a.test")
            assertEquals(listOf("https://b.test:8443"), table.list().map { it.origin })
            // A row removed behind the policy's back (the site-data screen) counts at once.
            krunk.sitePermissionsClearOrigin("https://b.test:8443/")
            assertEquals(
                Decision.Ask(listOf(PermissionKind.NOTIFICATIONS)),
                onTable.decide(3u, "https://b.test:8443", false, listOf(PermissionKind.NOTIFICATIONS)),
            )
            table.set("https://c.test", "geolocation", true)
            table.clear()
            assertTrue(table.list().isEmpty())
            // What is not a site is not stored, and does not throw.
            table.set("about:blank", "camera", true)
            assertNull(table.get("about:blank", "camera"))
        } finally {
            krunk.destroy()
        }
    }

    @Test
    fun theEnginesEntriesFollowTheTable() {
        store.set(site, "geolocation", true)
        store.set(site, "notifications", false)
        // Agrees already.
        assertNull(EngineAgreement.correction(0, site, false, EngineAgreement.ALLOW, store))
        assertNull(EngineAgreement.correction(1, site, false, EngineAgreement.DENY, store))
        // Differs from the row: the row's value.
        assertEquals(EngineAgreement.ALLOW, EngineAgreement.correction(0, site, false, EngineAgreement.DENY, store))
        assertEquals(EngineAgreement.DENY, EngineAgreement.correction(1, site, false, EngineAgreement.ALLOW, store))
        // No row (a one-time allow a dead process left, a default): forgotten.
        assertEquals(EngineAgreement.FORGET, EngineAgreement.correction(4, site, false, EngineAgreement.ALLOW, store))
        assertEquals(
            EngineAgreement.FORGET,
            EngineAgreement.correction(0, "https://b.test", false, EngineAgreement.ALLOW, store),
        )
        // "Ask" in the engine is as good as no entry.
        assertNull(EngineAgreement.correction(4, site, false, EngineAgreement.PROMPT, store))
        assertEquals(EngineAgreement.ALLOW, EngineAgreement.correction(0, site, false, EngineAgreement.PROMPT, store))
        // Not Krunk's: private entries, tracking exceptions, storage access, unknown types, no web origin.
        assertNull(EngineAgreement.correction(0, site, true, EngineAgreement.DENY, store))
        assertNull(EngineAgreement.correction(PermissionKind.GECKO_TRACKING, site, false, EngineAgreement.ALLOW, store))
        assertNull(EngineAgreement.correction(8, site, false, EngineAgreement.ALLOW, store))
        assertNull(EngineAgreement.correction(42, site, false, EngineAgreement.ALLOW, store))
        assertNull(EngineAgreement.correction(0, null, false, EngineAgreement.ALLOW, store))
    }

    @Test
    fun engineEntriesWithoutARowEndWithThePage() {
        val entries = EngineEntries<String>()
        entries.add(1u, "a")
        entries.add(1u, "b")
        entries.add(2u, "c")
        assertEquals(3, entries.size)
        assertEquals(listOf("a", "b"), entries.take(1u))
        assertEquals(emptyList<String>(), entries.take(1u))
        assertEquals(listOf("c"), entries.takeAll())
        assertEquals(0, entries.size)
    }

    @Test
    fun androidPermissionsPerKind() {
        assertEquals(listOf(AndroidPermissions.CAMERA), androidPermissionsFor(PermissionKind.CAMERA, 35))
        assertEquals(listOf(AndroidPermissions.RECORD_AUDIO), androidPermissionsFor(PermissionKind.MICROPHONE, 35))
        assertEquals(
            listOf(AndroidPermissions.FINE_LOCATION, AndroidPermissions.COARSE_LOCATION),
            androidPermissionsFor(PermissionKind.GEOLOCATION, 35),
        )
        assertEquals(
            listOf(AndroidPermissions.POST_NOTIFICATIONS),
            androidPermissionsFor(PermissionKind.NOTIFICATIONS, 33),
        )
        assertEquals(emptyList<String>(), androidPermissionsFor(PermissionKind.NOTIFICATIONS, 32))
        for (kind in PermissionKind.entries - setOf(
            PermissionKind.CAMERA,
            PermissionKind.MICROPHONE,
            PermissionKind.GEOLOCATION,
            PermissionKind.NOTIFICATIONS,
        )) {
            assertEquals(kind.name, emptyList<String>(), androidPermissionsFor(kind, 35))
        }
    }

    @Test
    fun androidsAnswer() {
        val location = listOf(AndroidPermissions.FINE_LOCATION, AndroidPermissions.COARSE_LOCATION)
        // The approximate location alone is a grant.
        assertTrue(AndroidPermissions.satisfied(location, setOf(AndroidPermissions.COARSE_LOCATION)))
        assertFalse(AndroidPermissions.satisfied(location, emptySet()))
        assertEquals(listOf(AndroidPermissions.Group.LOCATION), AndroidPermissions.missingGroups(location, emptySet()))
        val media = listOf(AndroidPermissions.CAMERA, AndroidPermissions.RECORD_AUDIO)
        assertFalse(AndroidPermissions.satisfied(media, setOf(AndroidPermissions.CAMERA)))
        assertEquals(
            listOf(AndroidPermissions.Group.MICROPHONE),
            AndroidPermissions.missingGroups(media, setOf(AndroidPermissions.CAMERA)),
        )
        assertEquals(
            listOf(AndroidPermissions.Group.CAMERA, AndroidPermissions.Group.MICROPHONE),
            AndroidPermissions.missingGroups(media, emptySet()),
        )
        assertTrue(AndroidPermissions.satisfied(media, media.toSet()))
        assertTrue(AndroidPermissions.satisfied(emptyList(), emptySet()))
        assertEquals(AndroidPermissions.Group.OTHER, AndroidPermissions.groupOf("android.permission.READ_CONTACTS"))
    }

    @Test
    fun camerasAreNamedByTheirSide() {
        assertEquals(CameraChoice.Facing.FRONT, CameraChoice.of("1", "Camera 1, Facing front, Orientation 270").facing)
        assertEquals(CameraChoice.Facing.BACK, CameraChoice.of("0", "Camera 0, Facing back, Orientation 90").facing)
        assertEquals(CameraChoice.Facing.OTHER, CameraChoice.of("2", "USB camera").facing)
        assertEquals(CameraChoice.Facing.OTHER, CameraChoice.of("3", null).facing)
        // A name is bounded and has no control characters.
        val long = CameraChoice.of("4", "x\n" + "y".repeat(500))
        assertEquals(60, long.name.length)
        assertFalse('\n' in long.name)
    }
}
