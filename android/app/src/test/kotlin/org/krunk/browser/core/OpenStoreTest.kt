package org.krunk.browser.core

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test
import org.krunk.ffi.defaultSettings

class OpenStoreTest {
    @Test
    fun aDatabaseThatCannotBeOpenedFallsBackToATemporaryStore() {
        // A path under a regular file can never be created or opened.
        val notADirectory = File.createTempFile("krunk", ".txt").apply { deleteOnExit() }
        val opened = openStore(File(notADirectory, "browser.db").path)
        assertNotNull(opened.error)
        // The session still works on it: settings load, rows can be written and read.
        opened.store.kvSet("k", "v")
        assertEquals("v", opened.store.kvGet("k"))
        assertEquals(defaultSettings(), SettingsRepository(opened.store).current)
        opened.store.destroy()
    }

    @Test
    fun aNormalDatabaseOpensWithoutAnError() {
        val dir = File(System.getProperty("java.io.tmpdir"), "krunk-open-${System.nanoTime()}").apply { mkdirs() }
        try {
            val opened = openStore(File(dir, "browser.db").path)
            assertNull(opened.error)
            opened.store.kvSet("k", "v")
            assertEquals("v", opened.store.kvGet("k"))
            opened.store.destroy()
        } finally {
            dir.deleteRecursively()
        }
    }
}
