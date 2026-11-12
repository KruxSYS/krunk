package org.krunk.browser.engine

import java.time.DateTimeException
import java.time.DayOfWeek
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.YearMonth
import java.time.ZoneOffset
import java.time.temporal.IsoFields
import java.time.temporal.TemporalAdjusters

/** Which of HTML's date and time fields a [PagePrompt.DateTime] is for. */
enum class DateKind { DATE, TIME, DATE_TIME, MONTH, WEEK }

/**
 * The value formats of HTML's date and time fields (`<input type=date|time|datetime-local|
 * month|week>`), read and written without Android: what the page gave is parsed leniently
 * (nothing a page sends can throw), what Krunk answers is always the exact format.
 */
object DateValues {
    private val date = Regex("""(\d{4,6})-(\d{2})-(\d{2})""")
    private val time = Regex("""(\d{2}):(\d{2})(?::(\d{2})(?:\.\d{1,3})?)?""")
    private val month = Regex("""(\d{4,6})-(\d{2})""")
    private val week = Regex("""(\d{4,6})-W(\d{2})""")

    /** The years a picker offers: wide enough for birthdays and bookings, and always the page's own range. */
    const val FIRST_YEAR = 1900
    const val LAST_YEAR = 2100

    fun parseDate(value: String?): LocalDate? {
        val m = date.matchEntire(value?.trim().orEmpty()) ?: return null
        return guarded { LocalDate.of(m.groupValues[1].toInt(), m.groupValues[2].toInt(), m.groupValues[3].toInt()) }
    }

    /** `HH:mm`, with seconds and a fraction allowed (and dropped: the pickers choose minutes). */
    fun parseTime(value: String?): LocalTime? {
        val m = time.matchEntire(value?.trim().orEmpty()) ?: return null
        return guarded { LocalTime.of(m.groupValues[1].toInt(), m.groupValues[2].toInt()) }
    }

    fun parseDateTime(value: String?): LocalDateTime? {
        val parts = value?.trim().orEmpty().split('T', ' ')
        if (parts.size != 2) return null
        val d = parseDate(parts[0]) ?: return null
        val t = parseTime(parts[1]) ?: return null
        return LocalDateTime.of(d, t)
    }

    fun parseMonth(value: String?): YearMonth? {
        val m = month.matchEntire(value?.trim().orEmpty()) ?: return null
        return guarded { YearMonth.of(m.groupValues[1].toInt(), m.groupValues[2].toInt()) }
    }

    /** The Monday of an ISO week (`2026-W46`); null for a week the year does not have (`2021-W53`). */
    fun parseWeek(value: String?): LocalDate? {
        val m = week.matchEntire(value?.trim().orEmpty()) ?: return null
        val year = m.groupValues[1].toInt()
        val number = m.groupValues[2].toInt()
        if (number < 1) return null
        return guarded {
            // 4 January is always in week 1 of its ISO year.
            val monday = LocalDate.of(year, 1, 4)
                .with(TemporalAdjusters.previousOrSame(DayOfWeek.MONDAY))
                .plusWeeks(number - 1L)
            monday.takeIf {
                it.get(IsoFields.WEEK_BASED_YEAR) == year &&
                    it.get(IsoFields.WEEK_OF_WEEK_BASED_YEAR) == number
            }
        }
    }

    fun date(value: LocalDate): String = "%04d-%02d-%02d".format(value.year, value.monthValue, value.dayOfMonth)

    fun time(value: LocalTime): String = "%02d:%02d".format(value.hour, value.minute)

    fun dateTime(day: LocalDate, at: LocalTime): String = date(day) + "T" + time(at)

    fun month(value: YearMonth): String = "%04d-%02d".format(value.year, value.monthValue)

    /** The ISO week a day is in. Its year is the week's, not the day's: 1 January 2027 is in `2026-W53`. */
    fun week(day: LocalDate): String =
        "%04d-W%02d".format(day.get(IsoFields.WEEK_BASED_YEAR), day.get(IsoFields.WEEK_OF_WEEK_BASED_YEAR))

    /** The week's number and year for a label ("Week 46, 2026"). */
    fun weekOf(day: LocalDate): Pair<Int, Int> =
        day.get(IsoFields.WEEK_OF_WEEK_BASED_YEAR) to day.get(IsoFields.WEEK_BASED_YEAR)

    /** Material's date picker counts in milliseconds of UTC midnights. */
    fun utcMillis(day: LocalDate): Long = day.atStartOfDay(ZoneOffset.UTC).toInstant().toEpochMilli()

    fun fromUtcMillis(millis: Long): LocalDate = LocalDate.ofEpochDay(Math.floorDiv(millis, MILLIS_PER_DAY))

    /** The day a field's value, minimum or maximum stands for (a week is its Monday, a month its first day). */
    fun dayOf(kind: DateKind, value: String?): LocalDate? = when (kind) {
        DateKind.DATE -> parseDate(value)
        DateKind.DATE_TIME -> parseDateTime(value)?.toLocalDate() ?: parseDate(value)
        DateKind.MONTH -> parseMonth(value)?.atDay(1)
        DateKind.WEEK -> parseWeek(value)
        DateKind.TIME -> null
    }

    /**
     * Whether a picker may offer [day] for a field limited to [min]..[max] (each in the field's
     * own format; an unreadable limit is no limit). For a week the whole week counts, so a week
     * that only partly lies inside the range can be picked through its days inside.
     */
    fun allowed(kind: DateKind, day: LocalDate, min: String?, max: String?): Boolean {
        val first = dayOf(kind, min)
        val last = dayOf(kind, max)?.let { end ->
            when (kind) {
                DateKind.WEEK -> end.plusDays(DAYS_AFTER_MONDAY)
                DateKind.MONTH -> end.with(TemporalAdjusters.lastDayOfMonth())
                else -> end
            }
        }
        return (first == null || !day.isBefore(first)) && (last == null || !day.isAfter(last))
    }

    /**
     * The day a picker opens on: the field's value, or [today] moved into the allowed range.
     */
    fun startDay(kind: DateKind, value: String?, min: String?, max: String?, today: LocalDate): LocalDate {
        dayOf(kind, value)?.let { return it }
        val first = dayOf(kind, min)
        val last = dayOf(kind, max)
        return when {
            first != null && today.isBefore(first) -> first
            last != null && today.isAfter(last) -> last
            else -> today
        }
    }

    /** The years a picker shows: [FIRST_YEAR]..[LAST_YEAR], widened to hold the value and the limits. */
    fun years(kind: DateKind, value: String?, min: String?, max: String?): IntRange {
        val known = listOfNotNull(dayOf(kind, value), dayOf(kind, min), dayOf(kind, max)).map { it.year }
        return minOf(FIRST_YEAR, known.minOrNull() ?: FIRST_YEAR)..maxOf(LAST_YEAR, known.maxOrNull() ?: LAST_YEAR)
    }

    private inline fun <T> guarded(make: () -> T?): T? = try {
        make()
    } catch (_: DateTimeException) {
        null
    }

    private const val MILLIS_PER_DAY = 86_400_000L
    private const val DAYS_AFTER_MONDAY = 6L
}

/** What the system's file picker is asked for. */
data class FileRequest(
    val multiple: Boolean,
    /** MIME types for the picker, never empty; [FileRequests.ANY] when the page accepts anything. */
    val types: List<String>,
    /** The page asked for a fresh photo (`capture` on an image field): offer the camera. */
    val camera: Boolean,
)

object FileRequests {
    const val ANY = "*/*"
    private val mime = Regex("""[a-z0-9][a-z0-9.+-]*/(\*|[a-z0-9][a-z0-9.+-]*)""")

    /**
     * Turns a file field's `accept` list into picker types. Entries are MIME types (all images, or
     * `application/pdf`) or file extensions (`.pdf`); an extension is looked up with
     * [typeOfExtension]. One entry nobody can place makes the request "anything": a picker
     * that hides the file the page wants is worse than one that shows too much.
     */
    fun of(
        multiple: Boolean,
        accept: List<String>,
        capture: Boolean,
        typeOfExtension: (String) -> String?,
    ): FileRequest {
        val types = LinkedHashSet<String>()
        var unknown = false
        for (raw in accept) {
            val entry = raw.trim().lowercase()
            when {
                entry.isEmpty() -> Unit

                '/' in entry -> if (mime.matches(entry)) types += entry else unknown = true

                else -> {
                    val type = typeOfExtension(entry.removePrefix("*").removePrefix("."))?.lowercase()
                    if (type != null && mime.matches(type)) types += type else unknown = true
                }
            }
        }
        val list = if (unknown || types.isEmpty() || ANY in types) listOf(ANY) else types.toList()
        return FileRequest(multiple, list, camera = capture && list.all { it.startsWith("image/") })
    }
}

/** Names for the copies of picked files that are handed to the engine. */
object UploadNames {
    const val MAX = 120
    private const val FALLBACK = "upload"

    /**
     * A file name that is safe inside Krunk's upload folder whatever the providing app called
     * the document: no path parts, no control characters, not hidden, not endless. The
     * extension survives shortening (sites look at it).
     */
    fun safe(name: String?): String {
        val cleaned = name.orEmpty()
            .substringAfterLast('/')
            .substringAfterLast('\\')
            .filter { it >= ' ' && it != '\u007f' && it !in "<>:\"|?*" }
            .trim()
            .trimStart('.')
        if (cleaned.isEmpty()) return FALLBACK
        if (cleaned.length <= MAX) return cleaned
        val dot = cleaned.lastIndexOf('.')
        val ext = if (dot > 0 && cleaned.length - dot <= MAX_EXTENSION) cleaned.substring(dot) else ""
        return cleaned.take(MAX - ext.length) + ext
    }

    private const val MAX_EXTENSION = 12
}

/** What `navigator.share()` is told. */
enum class ShareOutcome { SHARED, ABORTED, FAILED }

object Shares {
    /** What a page may put into one share (longer text is cut, not refused). */
    const val MAX_TEXT = 100_000

    /**
     * The text handed to the share sheet: the page's text and its address, each once. A page
     * that shares nothing at all cannot be shared (null).
     */
    fun text(text: String?, url: String?): String? {
        val t = text?.trim().orEmpty().take(MAX_TEXT)
        val u = url?.trim().orEmpty().take(MAX_TEXT)
        return when {
            t.isEmpty() && u.isEmpty() -> null
            u.isEmpty() -> t
            t.isEmpty() || u in t -> t.ifEmpty { u }
            else -> "$t\n$u"
        }
    }

    /**
     * The answer for the page. Android reports which app was picked, not whether that app
     * finished: picking one counts as shared, closing the sheet as aborted, and a device with
     * nothing to share to as failed.
     */
    fun outcome(started: Boolean, picked: Boolean): ShareOutcome = when {
        !started -> ShareOutcome.FAILED
        picked -> ShareOutcome.SHARED
        else -> ShareOutcome.ABORTED
    }
}
