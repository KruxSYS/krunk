package org.krunk.browser.ui.browser

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.browser.engine.BackAction
import org.krunk.browser.engine.PageState
import org.krunk.browser.engine.backAction

class NavRowTest {
    private val first = PageState(url = "https://example.com/", committed = true)
    private val middle = first.copy(canGoBack = true, canGoForward = true)

    @Test
    fun backAndForwardFollowThePagesHistory() {
        val table = listOf(
            // page, back enabled, forward enabled
            Triple(first, false, false),
            Triple(first.copy(canGoBack = true), true, false),
            Triple(first.copy(canGoForward = true), false, true),
            Triple(middle, true, true),
        )
        for ((page, back, forward) in table) {
            val row = navRow(page, hasSession = true, toolbarShowsHome = false)
            assertEquals(page.toString(), back, row.backEnabled)
            assertEquals(page.toString(), forward, row.forwardEnabled)
            // The history list opens from an enabled button only.
            assertEquals(back, row.backHasHistory)
            assertEquals(forward, row.forwardHasHistory)
        }
    }

    @Test
    fun aTabWithoutASessionHasNothingToGoTo() {
        // The new-tab page, a tab not loaded yet.
        for (row in listOf(navRow(null, hasSession = false, toolbarShowsHome = false), navRow(middle, false, false))) {
            assertFalse(row.backEnabled)
            assertFalse(row.forwardEnabled)
            assertEquals(BackAction.NONE, row.back)
        }
    }

    @Test
    fun homeIsInTheRowOnPhonesAndInTheToolbarOnTablets() {
        assertTrue(navRow(first, true, toolbarShowsHome = false).showHome)
        assertFalse(navRow(first, true, toolbarShowsHome = true).showHome)
        // The two rules agree at every width.
        for (width in listOf(320, 360, 599, 600, 900)) {
            val layout = toolbarLayout(width)
            assertEquals(width.toString(), !layout.showHome, navRow(first, true, layout.showHome).showHome)
        }
    }

    @Test
    fun backLeavesTheBlockedPanelBeforeItGoesBackInHistory() {
        val blocked = middle.copy(blocked = "https://ads.example/x")
        assertEquals(BackAction.LEAVE_BLOCKED, backAction(blocked))
        val row = navRow(blocked, true, false)
        assertTrue(row.backEnabled)
        // No history list for "leave the panel", and no Forward past a page that was not loaded.
        assertFalse(row.backHasHistory)
        assertFalse(row.forwardEnabled)
        // A tab that only ever had the blocked address: Back still leaves it (the tab closes).
        assertEquals(BackAction.LEAVE_BLOCKED, backAction(PageState(blocked = "https://ads.example/x")))
        assertEquals(BackAction.HISTORY, backAction(middle))
        assertEquals(BackAction.NONE, backAction(first))
        assertEquals(BackAction.NONE, backAction(null))
    }
}
