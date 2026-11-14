package org.krunk.browser.core

import java.time.LocalDateTime
import java.time.ZoneId
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.ffi.KrunkStore

class HistoryRepositoryTest {
    private val berlin = ZoneId.of("Europe/Berlin")

    private fun at(text: String, zone: ZoneId = berlin): Long =
        LocalDateTime.parse(text).atZone(zone).toInstant().toEpochMilli()

    private suspend fun readAll(repo: HistoryRepository, text: String, limit: Int): List<HistoryItem> {
        val seen = ArrayList<HistoryItem>()
        var cursor: HistoryCursor? = null
        while (true) {
            val page = repo.page(text, cursor, limit, berlin)
            assertTrue(page.items.size <= limit)
            seen += page.items
            if (page.next != null) cursor = page.next
            if (page.last) break
        }
        return seen
    }

    @Test
    fun pagingAcrossVisitsOfOneMillisecondLosesAndRepeatsNothing() = runBlocking {
        val store = KrunkStore.inMemory()
        val repo = HistoryRepository(store, Dispatchers.Unconfined)
        val ids = ArrayList<Long>()
        ids += store.recordVisit("https://example.com/old", "Old", 1_000)!!
        // Five visits in the same millisecond: every page boundary of size 2 falls inside them.
        for (i in 0 until 5) ids += store.recordVisit("https://example.com/same$i", "Same $i", 2_000)!!
        ids += store.recordVisit("https://example.com/new", "New", 3_000)!!

        for (limit in listOf(1, 2, 3, 4, 100)) {
            val seen = readAll(repo, "", limit).map { it.entry.id }
            assertEquals("limit $limit: nothing lost", ids.toSet(), seen.toSet())
            assertEquals("limit $limit: nothing twice", seen.size, seen.toSet().size)
            assertEquals("limit $limit: newest first", ids.sortedDescending(), seen)
        }
        // A page that fills the limit exactly is not the last one; the one after is empty and
        // leaves no cursor.
        val full = repo.page("", null, 7, berlin)
        assertFalse(full.last)
        val after = repo.page("", full.next, 7, berlin)
        assertTrue(after.items.isEmpty())
        assertTrue(after.last)
        assertNull(after.next)
        // Search pages the same way.
        assertEquals(5, readAll(repo, "same", 2).size)
        store.destroy()
    }

    @Test
    fun deletingTheLastRowShownDoesNotMoveTheNextPage() = runBlocking {
        val store = KrunkStore.inMemory()
        val repo = HistoryRepository(store, Dispatchers.Unconfined)
        for (i in 1..6) store.recordVisit("https://example.com/$i", "P$i", 1_000L * i)
        val first = repo.page("", null, 3, berlin)
        repo.delete(listOf(first.items.last().entry.id))
        val second = repo.page("", first.next, 3, berlin)
        assertEquals(listOf("P3", "P2", "P1"), second.items.map { it.entry.title })
        store.destroy()
    }

    @Test
    fun rowsCarryTheLocalDayAndTime() = runBlocking {
        val store = KrunkStore.inMemory()
        val repo = HistoryRepository(store, Dispatchers.Unconfined)
        // 23:30 UTC on 10 November is already the 11th in Berlin.
        store.recordVisit("https://example.com/a", "A", at("2026-11-10T23:30:00", ZoneId.of("UTC")))
        val item = repo.page("", null, 10, berlin).items.single()
        assertEquals("2026-11-11", item.day)
        assertEquals("00:30", item.time)
        store.destroy()
    }

    @Test
    fun groupsIntoRunsOfTheSameLocalDay() = runBlocking {
        val store = KrunkStore.inMemory()
        val repo = HistoryRepository(store, Dispatchers.Unconfined)
        store.recordVisit("https://example.com/1", "1", at("2026-11-12T00:00:00"))
        store.recordVisit("https://example.com/2", "2", at("2026-11-12T23:59:59"))
        store.recordVisit("https://example.com/3", "3", at("2026-11-13T00:00:01"))
        store.recordVisit("https://example.com/4", "4", at("2026-11-14T09:15:00"))
        store.recordVisit("https://example.com/5", "5", at("2026-11-14T18:00:00"))
        val items = readAll(repo, "", 2)
        val sections = repo.sections(items)
        assertEquals(listOf("2026-11-14", "2026-11-13", "2026-11-12"), sections.map { it.day })
        assertEquals(listOf(listOf("5", "4"), listOf("3"), listOf("2", "1")), sections.map { s -> s.items.map { it.entry.title } })
        assertEquals(listOf("18:00", "09:15"), sections[0].items.map { it.time })
        assertTrue(HistoryRepository.groupByDay(emptyList()).isEmpty())
        // Removing a row regroups without a day left empty.
        val without = HistoryRepository.groupByDay(items.filterNot { it.entry.title == "3" })
        assertEquals(listOf("2026-11-14", "2026-11-12"), without.map { it.day })
        store.destroy()
    }

    @Test
    fun daysUseTheOffsetOfEachVisitAcrossADaylightSavingChange() {
        // Berlin left summer time on 25 October 2026 (03:00 -> 02:00). Half past midnight on the
        // 25th is UTC+2, half past midnight on the 26th is UTC+1: one fixed offset for both
        // would put one of them on the wrong day.
        val before = at("2026-10-25T00:30:00")
        val after = at("2026-10-26T00:30:00")
        assertEquals(25L * 3_600_000, after - before)
        fun entry(id: Long, ts: Long) = org.krunk.ffi.HistoryEntry(id, "https://example.com/$id", "", ts, 1)
        val rows = listOf(entry(2, after), entry(1, before)).map { HistoryRepository.item(it, berlin) }
        assertEquals(listOf("2026-10-26", "2026-10-25"), rows.map { it.day })
        assertEquals(listOf("00:30", "00:30"), rows.map { it.time })
    }

    @Test
    fun dayHeadersNameTodayAndYesterdayByCalendarDay() {
        val now = at("2026-10-26T00:10:00")
        assertEquals(HistoryDayKind.TODAY, HistoryRepository.dayKind("2026-10-26", now, berlin))
        // Yesterday was 25 hours long: "now minus 24 h" would still be it, "minus 25 h" too.
        assertEquals(HistoryDayKind.YESTERDAY, HistoryRepository.dayKind("2026-10-25", now, berlin))
        assertEquals(HistoryDayKind.DATE, HistoryRepository.dayKind("2026-10-24", now, berlin))
        assertEquals(HistoryDayKind.DATE, HistoryRepository.dayKind("2026-10-27", now, berlin))
        assertEquals(HistoryDayKind.DATE, HistoryRepository.dayKind("odd", now, berlin))
    }
}
