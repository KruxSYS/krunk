package org.krunk.browser.engine.blocker

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.browser.engine.PageState
import org.krunk.browser.engine.blockedByBlocker
import org.krunk.browser.engine.blockedDismissed
import org.krunk.browser.engine.displayedUrl
import org.krunk.browser.engine.hasPageUnderBlocked
import org.krunk.browser.engine.locationChanged
import org.krunk.browser.engine.pageStarted
import org.krunk.browser.engine.pageStopped
import org.krunk.browser.engine.requested

class BlockedPageTest {
    private val ad = "https://googleads.g.doubleclick.net/pagead/x"
    private val page = PageState().locationChanged(
        "about:blank",
    ).pageStarted("https://example.com/").locationChanged("https://example.com/").pageStopped()

    @Test
    fun theReportGoesToTheTabThatWasLoadingThatAddress() {
        val loadingAd = page.pageStarted(ad)
        val pages = mapOf(1u to page, 2u to loadingAd, 3u to page.pageStarted("https://other.example/"))
        assertEquals(2u, blockedTab(ad, pages, active = 1u))
        // No tab is loading it (closed meanwhile, or went elsewhere): nothing is shown.
        assertNull(blockedTab(ad, pages - 2u, active = 1u))
        assertNull(blockedTab(ad, emptyMap(), active = null))
        // Two tabs loading the same address: the active one, else the newest.
        val two = mapOf(4u to loadingAd, 7u to loadingAd)
        assertEquals(4u, blockedTab(ad, two, active = 4u))
        assertEquals(7u, blockedTab(ad, two, active = 1u))
        // A tab already showing the panel for it is not picked twice.
        assertNull(blockedTab(ad, mapOf(2u to loadingAd.blockedByBlocker(ad)), active = 2u))
        // A page that did load that address (after "open once") is not a blocked load.
        assertNull(blockedTab(ad, mapOf(2u to loadingAd.locationChanged(ad)), active = 2u))
    }

    @Test
    fun theBlockedPanelShowsTheBlockedAddressUntilTheTabGoesElsewhere() {
        val blocked = page.pageStarted(ad).blockedByBlocker(ad).pageStopped()
        assertEquals(ad, blocked.blocked)
        assertFalse(blocked.loading)
        // The address bar names what was blocked, not the page under the panel.
        assertEquals(ad, displayedUrl("https://example.com/", false, blocked, hasSession = true, openedByPage = false))
        // The committed page (and with it history and the lock) is untouched.
        assertEquals("https://example.com/", blocked.url)
        assertTrue(blocked.hasPageUnderBlocked())

        // Go back: the page under it. A new load, typed or by a link: the panel goes.
        assertNull(blocked.blockedDismissed().blocked)
        assertNull(blocked.requested("https://example.org/").blocked)
        assertNull(blocked.pageStarted("https://example.org/").blocked)
    }

    @Test
    fun aTabThatNeverHadAPageHasNothingToGoBackTo() {
        // Opened from another app straight onto a blocked address.
        val fresh = PageState().locationChanged(
            "about:blank",
        ).requested(ad).pageStarted(ad).blockedByBlocker(ad).pageStopped()
        assertFalse(fresh.hasPageUnderBlocked())
        assertEquals(ad, displayedUrl(ad, false, fresh, hasSession = true, openedByPage = false))
    }

    @Test
    fun openOnceLoadsTheAddressAndThePanelIsGone() {
        val blocked = page.pageStarted(ad).blockedByBlocker(ad).pageStopped()
        val reloaded = blocked.requested(ad).pageStarted(ad).locationChanged(ad).pageStopped()
        assertNull(reloaded.blocked)
        assertEquals(ad, reloaded.url)
    }
}
