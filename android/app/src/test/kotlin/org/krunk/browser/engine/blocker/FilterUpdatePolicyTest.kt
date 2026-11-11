package org.krunk.browser.engine.blocker

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class FilterUpdatePolicyTest {
    private val now = 1_800_000_000_000L
    private val closed = 0L
    private val window = now + FilterUpdatePolicy.WINDOW_MS

    @Test
    fun nothingIsFetchedWithoutAUserCause() {
        // cause, "update automatically", window end, time -> may the fetch reach the network?
        val table = listOf(
            // By default (switch off, no window) only the user's own press fetches.
            Row(FetchCause.USER, false, closed, now, true),
            Row(FetchCause.BLOCKER, false, closed, now, false),
            Row(FetchCause.START, false, closed, now, false),
            Row(FetchCause.TIMER, false, closed, now, false),
            // During the window a press opened, the blocker's requests pass ...
            Row(FetchCause.BLOCKER, false, window, now + 1, true),
            Row(FetchCause.BLOCKER, false, window, window - 1, true),
            // ... and not a moment longer.
            Row(FetchCause.BLOCKER, false, window, window, false),
            Row(FetchCause.BLOCKER, false, window, window + 60_000, false),
            // The switch the user turned on opens the gate for the blocker for good.
            Row(FetchCause.BLOCKER, true, closed, now, true),
            // Krunk's own start and timers never fetch, whatever the switch or the window.
            Row(FetchCause.START, true, window, now, false),
            Row(FetchCause.TIMER, true, window, now, false),
        )
        for (r in table) {
            assertEquals(
                r.toString(),
                r.allowed,
                FilterUpdatePolicy.mayFetch(r.cause, r.automatic, r.windowUntil, r.now),
            )
        }
    }

    private data class Row(
        val cause: FetchCause,
        val automatic: Boolean,
        val windowUntil: Long,
        val now: Long,
        val allowed: Boolean,
    )

    @Test
    fun onlyTheUserOpensAWindowAndItIsBounded() {
        assertEquals(now + 5 * 60_000L, FilterUpdatePolicy.windowFor(FetchCause.USER, now))
        for (cause in listOf(FetchCause.BLOCKER, FetchCause.START, FetchCause.TIMER)) {
            assertNull(cause.name, FilterUpdatePolicy.windowFor(cause, now))
            assertFalse(cause.name, FilterUpdatePolicy.mayStart(cause))
        }
        assertTrue(FilterUpdatePolicy.mayStart(FetchCause.USER))
    }

    @Test
    fun theGateIsClosedAtInstallAndAtEveryStartByDefault() {
        // No window has ever been opened and the switch is off: closed at any time.
        for (t in listOf(0L, now, now + 86_400_000L)) assertFalse(FilterUpdatePolicy.gateOpen(false, closed, t))
        assertTrue(FilterUpdatePolicy.gateOpen(true, closed, now))
        assertTrue(FilterUpdatePolicy.gateOpen(false, window, now))
    }

    @Test
    fun anUpdateCountsTheListsThatGotANewerCopy() {
        val before = listOf(
            FilterList("easylist", written = 0, filters = 86_127, inPackage = true),
            FilterList("ublock-filters", written = 100, filters = 56_394, inPackage = true),
            FilterList("adguard-mobile", written = 0, filters = 0, inPackage = false),
        )
        val after = listOf(
            FilterList("easylist", written = 500, filters = 86_200, inPackage = true),
            FilterList("ublock-filters", written = 100, filters = 56_394, inPackage = true),
            FilterList("adguard-mobile", written = 510, filters = 13_077, inPackage = false),
            FilterList("regional", written = 520, filters = 900, inPackage = false),
        )
        assertEquals(3, FilterLists.updated(before, after))
        // No network: the same lists come back unchanged.
        assertEquals(0, FilterLists.updated(before, before))
        assertEquals(0, FilterLists.updated(before, emptyList()))
    }
}
