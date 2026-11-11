package org.krunk.browser.core

import java.util.TimeZone
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.krunk.ffi.HistoryEntry
import org.krunk.ffi.KrunkStore
import org.krunk.ffi.dateYmd

/** History entries of one calendar day (local time), newest first. */
data class HistorySection(
    /** `YYYY-MM-DD` of the local day. */
    val day: String,
    val entries: List<HistoryEntry>,
)

/**
 * Paged history reads and deletions on the core store, off the main thread. The grouping helper
 * is pure so the day boundaries can be tested without a device.
 */
class HistoryRepository(private val store: KrunkStore, private val io: CoroutineDispatcher = Dispatchers.IO) {
    /**
     * Visits matching [text] (empty = all) older than [before] (ms, exclusive; `null` = newest),
     * newest first, at most [limit].
     */
    suspend fun page(text: String, before: Long?, limit: Int = PAGE): List<HistoryEntry> =
        withContext(io) { store.queryHistory(text, before, limit.coerceIn(1, 1000).toUInt()) }

    suspend fun delete(ids: List<Long>): Long = withContext(io) { store.deleteVisits(ids).toLong() }

    suspend fun deleteUrl(url: String) = withContext(io) { store.deleteUrl(url) }

    suspend fun clear() = withContext(io) { store.clearHistory() }

    companion object {
        const val PAGE = 100

        /**
         * The local calendar day of a visit as `YYYY-MM-DD`: the core's UTC `date_ymd` shifted by
         * the zone's offset at that instant, so "Today" ends at local midnight.
         */
        fun localDay(ts: Long, zone: TimeZone = TimeZone.getDefault()): String = dateYmd(ts + zone.getOffset(ts))

        /** Groups entries (already newest first) into runs of the same local day. */
        fun groupByDay(entries: List<HistoryEntry>, zone: TimeZone = TimeZone.getDefault()): List<HistorySection> {
            val out = ArrayList<HistorySection>()
            var day: String? = null
            var bucket = ArrayList<HistoryEntry>()
            for (e in entries) {
                val d = localDay(e.ts, zone)
                if (d != day) {
                    if (day != null) out += HistorySection(day, bucket)
                    day = d
                    bucket = ArrayList()
                }
                bucket += e
            }
            if (day != null) out += HistorySection(day, bucket)
            return out
        }
    }
}
