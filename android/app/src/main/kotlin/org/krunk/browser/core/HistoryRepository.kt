package org.krunk.browser.core

import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.krunk.ffi.HistoryDevice
import org.krunk.ffi.HistoryEntry
import org.krunk.ffi.KrunkStore

/**
 * One visit as the list shows it: the entry with its local calendar day (`YYYY-MM-DD`) and its
 * local time of day (`HH:mm`), both worked out when the page was read.
 */
data class HistoryItem(val entry: HistoryEntry, val day: String, val time: String)

/** Where the next page starts: the time and the id of the last row of the page before. */
data class HistoryCursor(val ts: Long, val id: Long)

/**
 * A page of visits, newest first. [next] is the cursor for the page after it, `null` when this
 * page was empty; [last] says that the store has no older visits.
 */
data class HistoryPage(val items: List<HistoryItem>, val next: HistoryCursor?, val last: Boolean)

/** History rows of one calendar day (local time), newest first. */
data class HistorySection(
    /** `YYYY-MM-DD` of the local day. */
    val day: String,
    val items: List<HistoryItem>,
)

/**
 * One chip of the device filter. [filter] is what the store takes (`"this"`, `"unknown"`, a sync
 * device id, `"all"`); [name] is the device's sync name (only for another device, may be absent).
 */
data class HistoryChip(val filter: String, val name: String?)

/** What a day header says: today, yesterday, or the date itself. */
enum class HistoryDayKind { TODAY, YESTERDAY, DATE }

/**
 * Paged history reads and deletions on the core store, off the main thread.
 *
 * Pages are cut by (time, id), so visits that share the millisecond at a page boundary are
 * neither lost nor repeated. The day and the time of each row are computed here, on the reading
 * thread, from the row's own timestamp with `java.time`: the offset is the zone's offset *at
 * that visit*, so the days stay right across a daylight-saving change (the `day` the FFI row
 * carries uses one offset for the whole page and is therefore not used). The list does no FFI
 * call and no date arithmetic per row on the main thread.
 */
class HistoryRepository(private val store: KrunkStore, private val io: CoroutineDispatcher = Dispatchers.IO) {
    /**
     * Visits matching [text] (empty = all) after [before] (`null` = from the newest), newest
     * first, at most [limit], with their day and time in [zone].
     */
    suspend fun page(
        text: String,
        before: HistoryCursor?,
        limit: Int = PAGE,
        zone: ZoneId = ZoneId.systemDefault(),
        filter: String = ALL,
    ): HistoryPage = withContext(io) {
        val n = limit.coerceIn(1, 1000)
        val rows = store.queryHistoryPageIn(text, before?.ts, before?.id, n.toUInt(), 0L, filter)
        HistoryPage(
            items = rows.map { item(it.entry, zone) },
            next = rows.lastOrNull()?.let { HistoryCursor(it.entry.ts, it.entry.id) },
            last = rows.size < n,
        )
    }

    /** The devices that have visits here (this first, unknown last). */
    suspend fun devices(): List<HistoryDevice> = withContext(io) { store.historyDevices() }

    /** Deletes visits with `from <= ts < to` inside [filter]. */
    suspend fun deleteRange(from: Long, to: Long, filter: String): Long =
        withContext(io) { store.deleteHistoryRangeIn(from, to, filter).toLong() }

    suspend fun delete(ids: List<Long>): Long = withContext(io) { store.deleteVisits(ids).toLong() }

    suspend fun deleteUrl(url: String) = withContext(io) { store.deleteUrl(url) }

    /** [groupByDay] off the calling thread. */
    suspend fun sections(items: List<HistoryItem>): List<HistorySection> = withContext(io) { groupByDay(items) }

    companion object {
        const val PAGE = 100

        /** Filter keys the store knows besides a sync device id. */
        const val ALL = "all"
        const val THIS = "this"
        const val UNKNOWN = "unknown"

        /**
         * The filter chips for [devices] (as `historyDevices` lists them): this device always
         * first (even without visits), the other devices in the given order, unknown when it has
         * visits, and all last.
         */
        fun chips(devices: List<HistoryDevice>): List<HistoryChip> {
            val others = devices.filter { it.key != THIS && it.key != UNKNOWN && it.key != ALL }
            return buildList {
                add(HistoryChip(THIS, null))
                others.forEach { add(HistoryChip(it.key, it.name?.takeIf { n -> n.isNotBlank() })) }
                if (devices.any { it.key == UNKNOWN }) add(HistoryChip(UNKNOWN, null))
                add(HistoryChip(ALL, null))
            }
        }

        /**
         * [selected] if [chips] still has it, else this device (the chip of a device whose
         * visits were all removed goes away).
         */
        fun keep(selected: String, chips: List<HistoryChip>): String =
            if (chips.any { it.filter == selected }) selected else THIS

        /** The filter key of a visit's `device`: `""` this, `null` (or a `#` stand-in) unknown. */
        fun keyOf(device: String?): String = when {
            device == null || device.startsWith("#") -> UNKNOWN
            device.isEmpty() -> THIS
            else -> device
        }

        /** The first characters of a device id, for a device without a name. */
        fun shortId(id: String): String = id.filter { it.isLetterOrDigit() }.take(6).ifEmpty { id.take(6) }

        /**
         * The label of the device with filter [key] and sync [name]. [other] gets the short id
         * of a device without a name ("Other device 3fa2c1").
         */
        fun label(
            key: String,
            name: String?,
            thisDevice: String,
            unknown: String,
            all: String,
            other: (String) -> String,
        ): String = when (key) {
            THIS -> thisDevice
            UNKNOWN -> unknown
            ALL -> all
            else -> name?.takeIf { it.isNotBlank() } ?: other(shortId(key))
        }

        private val TIME: DateTimeFormatter = DateTimeFormatter.ofPattern("HH:mm")

        /** The row for [entry] as seen in [zone]. */
        fun item(entry: HistoryEntry, zone: ZoneId): HistoryItem {
            val at = Instant.ofEpochMilli(entry.ts).atZone(zone)
            return HistoryItem(entry, at.toLocalDate().toString(), at.toLocalTime().format(TIME))
        }

        /**
         * Groups rows (already newest first) into runs of the same local day. Only compares the
         * day keys the rows already carry.
         */
        fun groupByDay(items: List<HistoryItem>): List<HistorySection> {
            val out = ArrayList<HistorySection>()
            var day: String? = null
            var bucket = ArrayList<HistoryItem>()
            for (item in items) {
                if (item.day != day) {
                    if (day != null) out += HistorySection(day, bucket)
                    day = item.day
                    bucket = ArrayList()
                }
                bucket += item
            }
            if (day != null) out += HistorySection(day, bucket)
            return out
        }

        /** How the header of [day] reads when the current time is [now] in [zone]. */
        fun dayKind(day: String, now: Long, zone: ZoneId = ZoneId.systemDefault()): HistoryDayKind {
            val today = Instant.ofEpochMilli(now).atZone(zone).toLocalDate()
            return when (day) {
                today.toString() -> HistoryDayKind.TODAY

                // The calendar day before, not "24 hours ago": right on 23- and 25-hour days too.
                today.minusDays(1).toString() -> HistoryDayKind.YESTERDAY

                else -> HistoryDayKind.DATE
            }
        }
    }
}
