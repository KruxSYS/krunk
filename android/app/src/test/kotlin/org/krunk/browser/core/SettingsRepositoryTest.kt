package org.krunk.browser.core

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.ffi.KrunkStore

class SettingsRepositoryTest {
    @Test
    fun concurrentChangesAllSurvive() = runBlocking {
        val store = KrunkStore.inMemory()
        val repo = SettingsRepository(store)
        val start = repo.current.tabSleepMinutes
        // Twenty changes at once, each building on "the current value": without one writer at
        // a time several would start from the same snapshot and overwrite each other.
        val results = (1..20).map {
            async(Dispatchers.Default) { repo.update { s -> s.copy(tabSleepMinutes = s.tabSleepMinutes + 1u) } }
        }.awaitAll()
        assertTrue(results.all { it.isSuccess })
        assertEquals(start + 20u, repo.current.tabSleepMinutes)
        assertEquals(start + 20u, store.loadSettings().tabSleepMinutes)
        // Two different fields changed together are both kept.
        listOf(
            async(Dispatchers.Default) { repo.update { it.copy(httpsOnly = true) } },
            async(Dispatchers.Default) { repo.update { it.copy(searchSuggestions = false) } },
        ).awaitAll()
        assertTrue(repo.current.httpsOnly)
        assertTrue(!repo.current.searchSuggestions)
        store.destroy()
    }
}
