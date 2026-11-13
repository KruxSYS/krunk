package org.krunk.browser.vault

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.ffi.KrunkStore

class VaultLockSettingTest {
    @Test
    fun defaultIsFifteenMinutes() {
        val setting = VaultLockSetting({ null }, { _, _ -> })
        assertEquals(15, setting.current)
        assertEquals(15, setting.minutes.value)
        assertTrue(15 in VaultLockSetting.CHOICES)
    }

    @Test
    fun onlyOneToOneThousandFourHundredFortyMinutes() {
        val stored = HashMap<String, String>()
        val setting = VaultLockSetting(stored::get, stored::set)
        for (bad in listOf(0, -1, 1441, Int.MAX_VALUE)) assertFalse(setting.set(bad))
        assertEquals(15, setting.current)
        assertTrue(stored.isEmpty())
        for (good in listOf(1, 5, 60, 1440)) {
            assertTrue(setting.set(good))
            assertEquals(good, setting.current)
            assertEquals(good.toString(), stored[VaultLockSetting.KEY])
        }
    }

    @Test
    fun aStoredValueOutOfRangeIsTheDefault() {
        for (raw in listOf("0", "1441", "", "soon", "-5", "1.5")) assertEquals(raw, 15, VaultLockSetting.parse(raw))
        assertEquals(30, VaultLockSetting.parse(" 30 "))
        assertEquals(15, VaultLockSetting.parse(null))
    }

    @Test
    fun aStoreThatFailsKeepsTheLastValue() {
        val unreadable = VaultLockSetting({ error("no database") }, { _, _ -> error("no database") })
        assertEquals(15, unreadable.current)
        assertFalse(unreadable.set(5))
        assertEquals(15, unreadable.current)
    }

    @Test
    fun theGraceIsOneOfFourValuesAndSixtySecondsByDefault() {
        val stored = HashMap<String, String>()
        val setting = VaultLockSetting(stored::get, stored::set)
        assertEquals(60, setting.currentGraceSeconds)
        assertEquals(60, setting.graceSeconds.value)
        assertEquals(listOf(0, 30, 60, 300), VaultLockSetting.GRACE_CHOICES)
        for (bad in listOf(-1, 1, 15, 31, 59, 120, 301, 3600, Int.MAX_VALUE)) assertFalse(setting.setGraceSeconds(bad))
        assertEquals(60, setting.currentGraceSeconds)
        assertTrue(stored.isEmpty())
        for (good in listOf(0, 30, 300, 60)) {
            assertTrue(setting.setGraceSeconds(good))
            assertEquals(good, setting.currentGraceSeconds)
            assertEquals(good.toString(), stored[VaultLockSetting.GRACE_KEY])
        }
        // The two settings do not touch each other.
        assertEquals(15, setting.current)
        assertTrue(setting.set(5))
        assertEquals(60, setting.currentGraceSeconds)
    }

    @Test
    fun aStoredGraceThatIsNotOnTheListIsTheDefault() {
        for (raw in listOf(
            "45",
            "-30",
            "",
            "never",
            "30.0",
            "600",
        )) {
            assertEquals(raw, 60, VaultLockSetting.parseGrace(raw))
        }
        assertEquals(60, VaultLockSetting.parseGrace(null))
        assertEquals(0, VaultLockSetting.parseGrace("0"))
        assertEquals(
            300,
            VaultLockSetting(mapOf(VaultLockSetting.GRACE_KEY to "300")::get, { _, _ ->
            }).currentGraceSeconds,
        )
        val failing = VaultLockSetting({ error("no database") }, { _, _ -> error("no database") })
        assertEquals(60, failing.currentGraceSeconds)
        assertFalse(failing.setGraceSeconds(0))
        assertEquals(60, failing.currentGraceSeconds)
    }

    @Test
    fun survivesInTheCoreStore() {
        val store = KrunkStore.inMemory()
        assertTrue(VaultLockSetting(store::kvGet, store::kvSet).set(5))
        assertEquals(5, VaultLockSetting(store::kvGet, store::kvSet).current)
        assertEquals("5", store.kvGet("android.vault.lock_minutes"))
        assertTrue(VaultLockSetting(store::kvGet, store::kvSet).setGraceSeconds(300))
        assertEquals(300, VaultLockSetting(store::kvGet, store::kvSet).currentGraceSeconds)
        assertEquals("300", store.kvGet("android.vault.background_grace_s"))
        store.destroy()
    }
}
