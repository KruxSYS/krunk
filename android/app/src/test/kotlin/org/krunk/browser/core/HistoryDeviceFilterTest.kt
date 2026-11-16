package org.krunk.browser.core

import java.time.ZoneId
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.ffi.HistoryDevice
import org.krunk.ffi.KrunkStore

class HistoryDeviceFilterTest {
    private fun label(key: String, name: String?) =
        HistoryRepository.label(key, name, "This device", "Unknown device", "All") { "Other device $it" }

    @Test
    fun chipsAlwaysStartWithThisDeviceAndEndWithAll() {
        assertEquals(
            listOf(HistoryChip("this", null), HistoryChip("all", null)),
            HistoryRepository.chips(emptyList()),
        )
        val devices = listOf(
            HistoryDevice("this", null, 4),
            HistoryDevice("dev-b", "Laptop", 2),
            HistoryDevice("dev-a", null, 1),
            HistoryDevice("unknown", null, 3),
        )
        assertEquals(
            listOf(
                HistoryChip("this", null),
                HistoryChip("dev-b", "Laptop"),
                HistoryChip("dev-a", null),
                HistoryChip("unknown", null),
                HistoryChip("all", null),
            ),
            HistoryRepository.chips(devices),
        )
        // Without visits of its own, this device still has its chip; no unknown chip.
        assertEquals(
            listOf("this", "dev-b", "all"),
            HistoryRepository.chips(listOf(HistoryDevice("dev-b", "Laptop", 2))).map { it.filter },
        )
    }

    @Test
    fun labelsFallBackToAShortIdForADeviceWithoutName() {
        assertEquals("This device", label("this", null))
        assertEquals("Unknown device", label("unknown", null))
        assertEquals("All", label("all", null))
        assertEquals("Laptop", label("3fa2c1d4-aaaa", "Laptop"))
        assertEquals("Other device 3fa2c1", label("3fa2c1d4-aaaa", null))
        assertEquals("Other device 3fa2c1", label("3fa2c1d4-aaaa", "  "))
    }

    @Test
    fun aVisitsDeviceMapsToItsFilterKey() {
        assertEquals("this", HistoryRepository.keyOf(""))
        assertEquals("unknown", HistoryRepository.keyOf(null))
        assertEquals("unknown", HistoryRepository.keyOf("#1"))
        assertEquals("dev-a", HistoryRepository.keyOf("dev-a"))
    }

    @Test
    fun aSelectionWhoseDeviceIsGoneFallsBackToThisDevice() {
        val chips = HistoryRepository.chips(listOf(HistoryDevice("dev-a", "Phone", 1)))
        assertEquals("dev-a", HistoryRepository.keep("dev-a", chips))
        assertEquals("all", HistoryRepository.keep("all", chips))
        assertEquals("this", HistoryRepository.keep("dev-gone", chips))
        assertEquals("this", HistoryRepository.keep("unknown", chips))
    }

    @Test
    fun pagesReadInsideTheFilter() = runBlocking {
        val store = KrunkStore.inMemory()
        val repo = HistoryRepository(store, Dispatchers.Unconfined)
        store.recordVisit("https://example.com/a", "A", 1_000)
        store.recordVisit("https://example.com/b", "B", 2_000)
        val zone = ZoneId.of("UTC")
        assertEquals(2, repo.page("", null, 10, zone, HistoryRepository.THIS).items.size)
        assertEquals(2, repo.page("", null, 10, zone, HistoryRepository.ALL).items.size)
        assertEquals(1, repo.page("b", null, 10, zone, HistoryRepository.THIS).items.size)
        assertTrue(repo.page("", null, 10, zone, HistoryRepository.UNKNOWN).items.isEmpty())
        assertTrue(repo.page("", null, 10, zone, HistoryRepository.THIS).items.all { it.entry.device == "" })
        assertEquals(listOf("this", "all"), HistoryRepository.chips(repo.devices()).map { it.filter })
        store.destroy()
    }
}
