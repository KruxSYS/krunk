package org.krunk.browser.tabs

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.browser.engine.PageState
import org.krunk.browser.ui.tabs.switcherColumns
import org.krunk.browser.ui.tabs.tabMatches

class SwitcherLogicTest {
    private fun tab(title: String, url: String) = TabUi(
        id = 1u,
        url = url,
        displayUrl = url,
        title = title,
        private = false,
        live = true,
        internalPage = null,
        page = PageState(url = url, title = title),
    )

    @Test
    fun columnsGrowWithWidth() {
        assertEquals(2, switcherColumns(360))
        assertEquals(2, switcherColumns(599))
        assertEquals(3, switcherColumns(600))
        assertEquals(3, switcherColumns(800))
        assertEquals(4, switcherColumns(840))
        assertEquals(4, switcherColumns(1280))
    }

    @Test
    fun searchMatchesTitleOrUrlIgnoringCase() {
        val t = tab("Krunk Browser", "https://krunk.kruxsys.com/docs")
        assertTrue(tabMatches(t, ""))
        assertTrue(tabMatches(t, "  "))
        assertTrue(tabMatches(t, "browser"))
        assertTrue(tabMatches(t, "209.WF"))
        assertTrue(tabMatches(t, " docs "))
        assertFalse(tabMatches(t, "firefox"))
    }
}
