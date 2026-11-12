package org.krunk.browser.engine

import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.YearMonth
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PromptValuesTest {
    // ----- date and time values -----

    @Test
    fun valuesAreWrittenInTheFieldsExactFormats() {
        assertEquals("2026-01-05", DateValues.date(LocalDate.of(2026, 1, 5)))
        assertEquals("0987-12-31", DateValues.date(LocalDate.of(987, 12, 31)))
        assertEquals("09:05", DateValues.time(LocalTime.of(9, 5)))
        assertEquals("00:00", DateValues.time(LocalTime.MIDNIGHT))
        assertEquals("2026-11-12T23:59", DateValues.dateTime(LocalDate.of(2026, 11, 12), LocalTime.of(23, 59)))
        assertEquals("2026-02", DateValues.month(YearMonth.of(2026, 2)))
        assertEquals("2026-W46", DateValues.week(LocalDate.of(2026, 11, 12)))
    }

    @Test
    fun whatThePageGaveIsReadLeniently() {
        assertEquals(LocalDate.of(2026, 11, 12), DateValues.parseDate(" 2026-11-12 "))
        assertEquals(LocalTime.of(7, 30), DateValues.parseTime("07:30"))
        assertEquals(LocalTime.of(7, 30), DateValues.parseTime("07:30:15"))
        assertEquals(LocalTime.of(7, 30), DateValues.parseTime("07:30:15.250"))
        assertEquals(LocalDateTime.of(2026, 11, 12, 7, 30), DateValues.parseDateTime("2026-11-12T07:30"))
        assertEquals(LocalDateTime.of(2026, 11, 12, 7, 30), DateValues.parseDateTime("2026-11-12 07:30:59"))
        assertEquals(YearMonth.of(2026, 2), DateValues.parseMonth("2026-02"))
    }

    @Test
    fun nothingAPageSendsCanThrow() {
        val dates =
            listOf(null, "", "x", "2026-13-01", "2026-02-30", "26-01-01", "2026-1-1", "9999999-01-01", "2026-11-12T")
        for (bad in dates) {
            assertNull(bad, DateValues.parseDate(bad))
        }
        for (bad in listOf(
            null,
            "",
            "24:00",
            "12:60",
            "7:30",
            "12:30:",
            "noon",
        )) {
            assertNull(bad, DateValues.parseTime(bad))
        }
        for (bad in listOf(
            null,
            "",
            "2026-11-12",
            "2026-11-12T25:00",
            "T07:30",
        )) {
            assertNull(bad, DateValues.parseDateTime(bad))
        }
        for (bad in listOf(null, "", "2026-00", "2026-13", "2026")) assertNull(bad, DateValues.parseMonth(bad))
        for (bad in listOf(
            null,
            "",
            "2026-W00",
            "2026-W54",
            "2026-46",
            "2026-W1",
        )) {
            assertNull(bad, DateValues.parseWeek(bad))
        }
    }

    @Test
    fun isoWeeksAtTheEdgesOfAYear() {
        // 2026 has 53 weeks: 1 January 2027 is a Friday in 2026-W53.
        assertEquals("2026-W53", DateValues.week(LocalDate.of(2027, 1, 1)))
        assertEquals("2027-W01", DateValues.week(LocalDate.of(2027, 1, 4)))
        // 29 December 2025 is a Monday and already in week 1 of 2026.
        assertEquals("2026-W01", DateValues.week(LocalDate.of(2025, 12, 29)))
        assertEquals("2025-W52", DateValues.week(LocalDate.of(2025, 12, 28)))
        // 2020 had 53 weeks, 2021 did not.
        assertEquals("2020-W53", DateValues.week(LocalDate.of(2021, 1, 3)))
        assertEquals(LocalDate.of(2020, 12, 28), DateValues.parseWeek("2020-W53"))
        assertNull(DateValues.parseWeek("2021-W53"))
        // A week is read as its Monday, and reading and writing agree for every week of some years.
        assertEquals(LocalDate.of(2026, 11, 9), DateValues.parseWeek("2026-W46"))
        assertEquals(LocalDate.of(2025, 12, 29), DateValues.parseWeek("2026-W01"))
        var monday = LocalDate.of(2018, 1, 1)
        while (monday.year < 2032) {
            assertEquals(monday, DateValues.parseWeek(DateValues.week(monday)))
            assertEquals(DateValues.week(monday), DateValues.week(monday.plusDays(6)))
            monday = monday.plusWeeks(1)
        }
        assertEquals(46 to 2026, DateValues.weekOf(LocalDate.of(2026, 11, 12)))
    }

    @Test
    fun thePickersUtcMillisecondsAndDaysAgree() {
        assertEquals(0L, DateValues.utcMillis(LocalDate.of(1970, 1, 1)))
        assertEquals(LocalDate.of(1970, 1, 1), DateValues.fromUtcMillis(0))
        assertEquals(LocalDate.of(1969, 12, 31), DateValues.fromUtcMillis(-1))
        for (day in listOf(LocalDate.of(1900, 1, 1), LocalDate.of(2026, 11, 12), LocalDate.of(2100, 12, 31))) {
            assertEquals(day, DateValues.fromUtcMillis(DateValues.utcMillis(day)))
            assertEquals(day, DateValues.fromUtcMillis(DateValues.utcMillis(day) + 86_399_999))
        }
    }

    @Test
    fun onlyDaysInsideTheFieldsRangeCanBePicked() {
        val day = LocalDate.of(2026, 11, 12)
        assertTrue(DateValues.allowed(DateKind.DATE, day, "", ""))
        assertTrue(DateValues.allowed(DateKind.DATE, day, "2026-11-12", "2026-11-12"))
        assertFalse(DateValues.allowed(DateKind.DATE, day, "2026-11-13", ""))
        assertFalse(DateValues.allowed(DateKind.DATE, day, "", "2026-11-11"))
        // An unreadable limit is no limit.
        assertTrue(DateValues.allowed(DateKind.DATE, day, "soon", "later"))
        // datetime-local limits carry a time; the day decides.
        assertTrue(DateValues.allowed(DateKind.DATE_TIME, day, "2026-11-12T18:00", "2026-11-12T20:00"))
        assertFalse(DateValues.allowed(DateKind.DATE_TIME, day, "2026-11-13T00:00", ""))
        // The last allowed week and month count to their last day.
        assertTrue(DateValues.allowed(DateKind.WEEK, LocalDate.of(2026, 11, 15), "2026-W46", "2026-W46"))
        assertFalse(DateValues.allowed(DateKind.WEEK, LocalDate.of(2026, 11, 16), "2026-W46", "2026-W46"))
        assertFalse(DateValues.allowed(DateKind.WEEK, LocalDate.of(2026, 11, 8), "2026-W46", "2026-W46"))
        assertTrue(DateValues.allowed(DateKind.MONTH, LocalDate.of(2026, 11, 1), "2026-11", "2026-11"))
        assertFalse(DateValues.allowed(DateKind.MONTH, LocalDate.of(2026, 12, 1), "2026-11", "2026-11"))
        assertFalse(DateValues.allowed(DateKind.MONTH, LocalDate.of(2026, 10, 1), "2026-11", "2026-11"))
    }

    @Test
    fun aPickerOpensOnTheValueOrOnTodayMovedIntoTheRange() {
        val today = LocalDate.of(2026, 11, 12)
        assertEquals(LocalDate.of(2001, 2, 3), DateValues.startDay(DateKind.DATE, "2001-02-03", "", "", today))
        assertEquals(today, DateValues.startDay(DateKind.DATE, "", "", "", today))
        assertEquals(today, DateValues.startDay(DateKind.DATE, "nonsense", "2020-01-01", "2030-01-01", today))
        assertEquals(LocalDate.of(2027, 1, 1), DateValues.startDay(DateKind.DATE, "", "2027-01-01", "", today))
        assertEquals(LocalDate.of(1999, 12, 31), DateValues.startDay(DateKind.DATE, "", "", "1999-12-31", today))
        assertEquals(LocalDate.of(2026, 11, 9), DateValues.startDay(DateKind.WEEK, "2026-W46", "", "", today))
        assertEquals(LocalDate.of(2026, 2, 1), DateValues.startDay(DateKind.MONTH, "2026-02", "", "", today))
    }

    @Test
    fun thePickersYearsAlwaysHoldTheValueAndTheLimits() {
        assertEquals(1900..2100, DateValues.years(DateKind.DATE, "", "", ""))
        assertEquals(1900..2100, DateValues.years(DateKind.DATE, "2026-11-12", "2000-01-01", "2030-01-01"))
        assertEquals(1850..2300, DateValues.years(DateKind.DATE, "", "1850-06-01", "2300-01-01"))
        assertEquals(1600..2100, DateValues.years(DateKind.MONTH, "1600-01", "", ""))
    }

    // ----- file fields -----

    private val known = mapOf("pdf" to "application/pdf", "jpg" to "image/jpeg", "png" to "image/png")

    private fun request(vararg accept: String, capture: Boolean = false, multiple: Boolean = false) =
        FileRequests.of(multiple, accept.toList(), capture) { known[it] }

    @Test
    fun acceptBecomesPickerTypes() {
        assertEquals(listOf("*/*"), request().types)
        assertEquals(listOf("image/*"), request("image/*").types)
        assertEquals(listOf("image/png", "application/pdf"), request(" Image/PNG ", ".pdf", "image/png").types)
        assertEquals(listOf("image/jpeg"), request("*.jpg").types)
        assertEquals(listOf("image/jpeg"), request("jpg").types)
        assertTrue(request("image/*", multiple = true).multiple)
    }

    @Test
    fun anEntryNobodyCanPlaceMeansAnything() {
        assertEquals(listOf("*/*"), request("image/*", ".xyz").types)
        assertEquals(listOf("*/*"), request("image/").types)
        assertEquals(listOf("*/*"), request("a b/c").types)
        assertEquals(listOf("*/*"), request("image/png", "*/*").types)
        assertEquals(listOf("image/png"), request("", "  ", "image/png").types)
    }

    @Test
    fun theCameraIsOfferedOnlyForAFieldThatAsksForAFreshPicture() {
        assertTrue(request("image/*", capture = true).camera)
        assertTrue(request(".jpg", ".png", capture = true).camera)
        assertFalse(request("image/*").camera)
        assertFalse(request("video/*", capture = true).camera)
        assertFalse(request("image/*", ".pdf", capture = true).camera)
        assertFalse(request(capture = true).camera)
    }

    @Test
    fun uploadNamesStayInsideTheUploadFolder() {
        assertEquals("report.pdf", UploadNames.safe("report.pdf"))
        assertEquals("passwd", UploadNames.safe("../../etc/passwd"))
        assertEquals("evil.exe", UploadNames.safe("C:\\Users\\x\\evil.exe"))
        assertEquals("upload", UploadNames.safe(null))
        assertEquals("upload", UploadNames.safe("  "))
        assertEquals("upload", UploadNames.safe("..."))
        assertEquals("upload", UploadNames.safe("dir/"))
        assertEquals("htaccess", UploadNames.safe(".htaccess"))
        assertEquals("ab.txt", UploadNames.safe("a\u0000b\n.txt"))
        assertEquals("name.txt", UploadNames.safe("na<>:\"|?*me.txt"))
        val long = UploadNames.safe("x".repeat(500) + ".jpeg")
        assertEquals(UploadNames.MAX, long.length)
        assertTrue(long.endsWith(".jpeg"))
        assertEquals(UploadNames.MAX, UploadNames.safe("y".repeat(500)).length)
    }

    // ----- share -----

    @Test
    fun aShareCarriesThePagesTextAndAddressOnce() {
        assertEquals("Look\nhttps://example.com/a", Shares.text("Look", "https://example.com/a"))
        assertEquals("https://example.com/a", Shares.text(null, " https://example.com/a "))
        assertEquals("Look", Shares.text("Look", ""))
        assertEquals(
            "See https://example.com/a now",
            Shares.text("See https://example.com/a now", "https://example.com/a"),
        )
        assertNull(Shares.text(null, null))
        assertNull(Shares.text("  ", ""))
        assertEquals(Shares.MAX_TEXT, Shares.text("z".repeat(Shares.MAX_TEXT + 50), null)!!.length)
    }

    @Test
    fun theShareAnswerFollowsWhatAndroidReports() {
        assertEquals(ShareOutcome.SHARED, Shares.outcome(started = true, picked = true))
        assertEquals(ShareOutcome.ABORTED, Shares.outcome(started = true, picked = false))
        assertEquals(ShareOutcome.FAILED, Shares.outcome(started = false, picked = false))
        assertEquals(ShareOutcome.FAILED, Shares.outcome(started = false, picked = true))
    }
}
