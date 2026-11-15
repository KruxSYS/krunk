package org.krunk.browser.sync

import java.io.File
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.browser.vault.FakeWrapKeyStore
import org.krunk.browser.vault.KeystoreKeyWrapper
import org.krunk.browser.vault.WrapFailure

/** Krunk account and sync (0.6.0): the transport's rules, the schedule, the key and the backups. */
class SyncTest {
    @Test
    fun onlyApiPathsTheCoreMakesAreSent() {
        for (ok in listOf(
            "link/authorize",
            "records?since=12&limit=500",
            "approvals/abc-1/confirm",
            "keys/chain?from=anchor",
            // The core's real key refresh (core/src/sync/client.rs).
            "keys/chain?from=3,1",
        )) {
            assertTrue(ok, SyncHttp.pathAllowed(ok))
        }
        for (bad in listOf(
            "",
            "/api/v1/x",
            "../admin",
            "a//b",
            "https://evil.example/",
            "a b",
            "x\ny",
            "x".repeat(600),
        )) {
            assertFalse(bad, SyncHttp.pathAllowed(bad))
        }
    }

    @Test
    fun releaseBuildsTalkHttpsOnly() {
        assertTrue(SyncHttp.baseAllowed("https://krunk.209.wf", allowHttp = false))
        assertFalse(SyncHttp.baseAllowed("http://10.250.136.3:18082", allowHttp = false))
        assertTrue(SyncHttp.baseAllowed("http://10.250.136.3:18082", allowHttp = true))
        assertFalse(SyncHttp.baseAllowed("ftp://x", allowHttp = true))
    }

    @Test
    fun theScheduleRunsWhenDueOrHalfAMinuteAfterAChange() {
        assertFalse(SyncController.due(now = 1_000, nextRunMs = 2_000, pendingSince = null))
        assertTrue(SyncController.due(now = 2_000, nextRunMs = 2_000, pendingSince = null))
        assertFalse(SyncController.due(now = 10_000, nextRunMs = 999_999, pendingSince = 0 + 1))
        assertTrue(SyncController.due(now = 31_000, nextRunMs = 999_999, pendingSince = 1_000))
        assertFalse(SyncController.due(now = 31_000, nextRunMs = null, pendingSince = null))
    }

    @Test
    fun syncsKeyIsMadeAtTheFirstSignInAndApartFromTheVaults() {
        val store = FakeWrapKeyStore()
        val sync = KeystoreKeyWrapper(store, KeystoreKeyWrapper.SYNC_ALIAS, createOnProtect = true)
        val vault = KeystoreKeyWrapper(store)
        assertFalse(sync.keyPresent())
        val blob = sync.protect(byteArrayOf(1, 2, 3))
        assertTrue(sync.keyPresent())
        assertFalse("the vault's key is another one", vault.keyPresent())
        assertArrayEquals(byteArrayOf(1, 2, 3), sync.unprotect(blob))
        // The vault's wrapper still never makes a key by itself.
        try {
            vault.protect(byteArrayOf(1))
            throw AssertionError("made a vault key")
        } catch (_: WrapFailure) {
        }
        // Gone (e.g. a new phone): what was wrapped can't be opened, nothing new is made by unprotect.
        sync.deleteKey()
        try {
            sync.unprotect(blob)
            throw AssertionError("opened without the key")
        } catch (_: WrapFailure) {
        }
        assertFalse(sync.keyPresent())
    }

    @Test
    fun theSyncFolderIsInNoBackup() {
        val filesDir = File("/data/user/0/org.krunk.browser/files")
        assertEquals(File(filesDir, "krunk-sync"), SyncController.dir(filesDir))
        val backup = File("src/main/res/xml/backup_rules.xml").readText()
        val extraction = File("src/main/res/xml/data_extraction_rules.xml").readText()
        assertTrue(backup.contains("""<exclude domain="file" path="krunk-sync" />"""))
        val cloud = extraction.substringAfter("<cloud-backup>").substringBefore("</cloud-backup>")
        assertTrue(cloud.contains("""<exclude domain="file" path="krunk-sync" />"""))
        // Device transfer takes files/krunk only, which krunk-sync is not inside.
        val transfer = extraction.substringAfter("<device-transfer>").substringBefore("</device-transfer>")
        val includes = Regex("""<include domain="([a-z]+)" path="([^"]+)" />""").findAll(transfer).map {
            it.groupValues[1] to
                it.groupValues[2]
        }.toList()
        assertEquals(listOf("file" to "krunk"), includes)
        assertFalse(SyncController.dir(filesDir).path.startsWith(File(filesDir, "krunk").path + File.separator))
    }
}
