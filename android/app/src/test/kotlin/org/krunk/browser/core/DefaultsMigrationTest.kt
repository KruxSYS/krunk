package org.krunk.browser.core

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Test
import org.krunk.ffi.KrunkStore
import org.krunk.ffi.SearchEngineId
import org.krunk.ffi.defaultSettings

class DefaultsMigrationTest {
    private val old = defaultSettings().copy(
        searchEngine = SearchEngineId.DUCK_DUCK_GO,
        homepage = "https://duckduckgo.com/",
    )

    @Test
    fun oldDefaultsMoveToGoogle() {
        val s = DefaultsMigration.migrate(old, 0)
        assertEquals(SearchEngineId.GOOGLE, s.searchEngine)
        assertEquals("https://www.google.com", s.homepage)
    }

    @Test
    fun ownChoicesAndMigratedProfilesStay() {
        val own = old.copy(searchEngine = SearchEngineId.BING, homepage = "https://a.example/")
        assertEquals(own, DefaultsMigration.migrate(own, 0))
        assertEquals(old, DefaultsMigration.migrate(old, DefaultsMigration.CURRENT))
        assertEquals("", DefaultsMigration.migrate(old.copy(homepage = ""), 0).homepage)
    }

    @Test
    fun theRepositoryMigratesOnceAndSetsTheMarker() {
        val store = KrunkStore.inMemory()
        try {
            store.saveSettings(old)
            assertEquals(SearchEngineId.GOOGLE, SettingsRepository(store).current.searchEngine)
            assertEquals("https://www.google.com", store.loadSettings().homepage)
            assertEquals(DefaultsMigration.CURRENT.toString(), store.kvGet(DefaultsMigration.KEY))
            // The user goes back to DuckDuckGo: the next start keeps it.
            store.saveSettings(old)
            assertEquals(SearchEngineId.DUCK_DUCK_GO, SettingsRepository(store).current.searchEngine)
        } finally {
            store.destroy()
        }
    }

    @Test
    fun aNewInstallStartsWithGoogle() {
        val dir = File(System.getProperty("java.io.tmpdir"), "krunk-defaults-${System.nanoTime()}").apply { mkdirs() }
        try {
            val store = KrunkStore.open(File(dir, "browser.db").path)
            val s = SettingsRepository(store).current
            assertEquals(SearchEngineId.GOOGLE, s.searchEngine)
            assertEquals("https://www.google.com", s.homepage)
            store.destroy()
        } finally {
            dir.deleteRecursively()
        }
    }
}
