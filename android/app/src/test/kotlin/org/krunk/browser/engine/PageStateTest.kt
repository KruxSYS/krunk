package org.krunk.browser.engine

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PageStateTest {
    private val bank = "https://bank.example/login"
    private val evil = "https://evil.example/"
    private val newTab = "https://krunk.invalid/newtab.html"

    private fun shown(model: String, page: PageState, openedByPage: Boolean = false, internal: Boolean = false) =
        displayedUrl(model, internal, page, hasSession = true, openedByPage = openedByPage)

    @Test
    fun anUnloadedTabShowsItsSavedAddress() {
        assertEquals(bank, displayedUrl(bank, false, PageState(), hasSession = false, openedByPage = false))
    }

    @Test
    fun aTypedAddressShowsWhileItLoadsAndTheCommittedOneAfter() {
        var s = PageState().locationChanged(ABOUT_BLANK) // the session's first blank page
        assertFalse(s.committed)
        s = s.requested(bank)
        assertEquals(bank, shown(bank, s))
        s = s.locationChanged("https://bank.example/login?next=1")
        assertTrue(s.committed)
        assertNull(s.pendingUrl)
        assertEquals("https://bank.example/login?next=1", shown(bank, s))
    }

    @Test
    fun aLoadThatNeverCommitsLeavesTheOldAddress() {
        var s = PageState().locationChanged(evil)
        s = s.requested(bank) // typed, but the server answers with a download or 204
        assertEquals(bank, shown(bank, s))
        s = s.pageStopped()
        assertEquals(evil, shown(bank, s))
        // ... and the tab model goes back to what is really shown.
        assertEquals(evil, modelUrlAfter(s, modelIsInternal = false, engineUrlIsInternal = false))
    }

    @Test
    fun aWindowOpenedByAPageShowsNothingUntilItCommits() {
        // window.open("https://bank.example/never-commits"): the model holds the requested address.
        var s = PageState().locationChanged(ABOUT_BLANK)
        assertEquals("", shown(bank, s, openedByPage = true))
        s = s.copy(loading = true, title = "Sign in")
        assertEquals("", shown(bank, s, openedByPage = true))
        s = s.pageStopped()
        assertEquals("", shown(bank, s, openedByPage = true))
        // Once the engine commits, its address is shown.
        s = s.locationChanged(evil)
        assertEquals(evil, shown(bank, s, openedByPage = true))
    }

    @Test
    fun aPageThatGoesBlankDoesNotKeepItsOldAddress() {
        var s = PageState().locationChanged(ABOUT_BLANK).locationChanged(bank)
        assertEquals(bank, shown(bank, s))
        s = s.locationChanged(ABOUT_BLANK) // navigated there by a related window
        assertEquals("", shown(bank, s))
        assertEquals(ABOUT_BLANK, modelUrlAfter(s, modelIsInternal = false, engineUrlIsInternal = false))
    }

    @Test
    fun theNewTabPageStaysWhileTheEngineIsParked() {
        var s = PageState().locationChanged(ABOUT_BLANK).locationChanged(bank)
        s = s.parked().locationChanged(ABOUT_BLANK)
        assertEquals(newTab, shown(newTab, s, internal = true))
        assertNull(modelUrlAfter(s, modelIsInternal = true, engineUrlIsInternal = false))
        // Going back into the page shows the page again.
        s = s.locationChanged(bank)
        assertEquals(bank, shown(newTab, s, internal = true))
        assertEquals(bank, modelUrlAfter(s, modelIsInternal = true, engineUrlIsInternal = false))
    }

    @Test
    fun anEngineAddressOnTheInternalOriginNeverBecomesANativePage() {
        val s = PageState().locationChanged("https://krunk.invalid/settings.html")
        assertNull(modelUrlAfter(s, modelIsInternal = false, engineUrlIsInternal = true))
    }

    @Test
    fun thePendingAddressIsNotTakenOverByTheModel() {
        val s = PageState().locationChanged(evil).requested(bank)
        assertNull(modelUrlAfter(s, modelIsInternal = false, engineUrlIsInternal = false))
    }

    @Test
    fun theErrorPageKeepsTheFailedAddress() {
        val page = "data:text/html;base64,AAAA"
        var s = PageState().locationChanged(evil).requested(bank)
        s = s.copy(loadError = LoadError(bank, 2, 3, page))
        s = s.locationChanged(page)
        assertEquals(bank, s.url)
        assertEquals(bank, s.loadError?.uri)
        // Any other data: page a site navigates to is shown as what it is, and ends the error.
        val other = "data:text/html,<h1>Sign in</h1>"
        s = s.locationChanged(other)
        assertEquals(other, s.url)
        assertNull(s.loadError)
    }

    @Test
    fun aCrashEndsLoadingAndFullScreenAndKeepsTheAddress() {
        var s = PageState().locationChanged(bank).copy(loading = true, progress = 0.4f, fullScreen = true)
        s = s.requested(evil).crashed()
        assertTrue(s.crashed)
        assertFalse(s.loading)
        assertFalse(s.fullScreen)
        assertEquals(0f, s.progress)
        assertEquals(bank, shown(bank, s))
        // A new load clears the flag.
        assertFalse(s.requested(bank).crashed)
    }

    @Test
    fun theLockIsTakenDownWhenALoadToAnotherSiteStarts() {
        var s = PageState().locationChanged(evil).securityChanged(true)
        // A link click or a redirect to another site: no lock until the engine reports on it.
        s = s.pageStarted(bank)
        assertNull(s.secure)
        s = s.locationChanged(bank)
        assertNull(s.secure)
        s = s.securityChanged(true).pageStopped()
        assertEquals(true, s.secure)
        // The report may also arrive before the location: it is kept.
        s = s.pageStarted(evil).securityChanged(false).locationChanged(evil).pageStopped()
        assertEquals(false, s.secure)
    }

    @Test
    fun theLockStaysForLoadsOnTheSameSite() {
        var s = PageState().locationChanged(bank).securityChanged(true)
        s = s.pageStarted("https://bank.example/account") // same site, also a reload
        assertEquals(true, s.secure)
        s = s.pageStarted("https://BANK.example:443/x".replace(":443", ""))
        assertEquals(true, s.secure)
        // Another port or scheme is another site.
        assertNull(s.pageStarted("https://bank.example:8443/").secure)
        assertNull(s.pageStarted("http://bank.example/").secure)
    }

    @Test
    fun aLoadThatNeverCommitsGivesTheOldLockBack() {
        var s = PageState().locationChanged(evil).securityChanged(true)
        s = s.requested(bank).pageStarted(bank) // answers with a download
        assertNull(s.secure)
        s = s.pageStopped()
        assertEquals(evil, s.url)
        assertEquals(true, s.secure)
        assertNull(s.secureBeforeLoad)
    }

    @Test
    fun originsIgnoreCaseAndUserPart() {
        assertEquals("https://example.com", originOf("HTTPS://Example.COM/a?b"))
        assertEquals("https://evil.example", originOf("https://bank.example@evil.example/"))
        assertEquals("http://example.com:8080", originOf("http://example.com:8080"))
        assertNull(originOf("about:blank"))
        assertNull(originOf("data:text/html,x"))
        assertNull(originOf(""))
        assertNull(originOf("https:///x"))
    }

    @Test
    fun aNewDocumentStartsWithoutThePreviousTitle() {
        var s = PageState().locationChanged(evil).copy(title = "Evil news")
        s = s.pageStarted(bank)
        assertEquals("", s.title)
        // A change inside the same document (no new load) keeps the title.
        val spa = PageState().locationChanged(evil).copy(title = "App").locationChanged("https://evil.example/#/inbox")
        assertEquals("App", spa.title)
    }

    @Test
    fun onlyPagesTheUserSawAreRecordedAsVisits() {
        assertTrue(isRecordableVisit(VisitFlags.TOP_LEVEL))
        // A redirect's destination is a visit (the engine adds how it was reached) ...
        assertTrue(isRecordableVisit(VisitFlags.TOP_LEVEL or 2))
        assertTrue(isRecordableVisit(VisitFlags.TOP_LEVEL or 4))
        // ... the addresses that only redirected are not, nor frames, nor failed loads.
        assertFalse(isRecordableVisit(VisitFlags.TOP_LEVEL or VisitFlags.REDIRECT_SOURCE))
        assertFalse(isRecordableVisit(VisitFlags.TOP_LEVEL or VisitFlags.REDIRECT_SOURCE_PERMANENT))
        assertFalse(isRecordableVisit(0))
        assertFalse(isRecordableVisit(VisitFlags.TOP_LEVEL or VisitFlags.UNRECOVERABLE_ERROR))
    }

    @Test
    fun theFirstBlankPageIsIgnoredOnlyOnce() {
        var s = PageState().requested(bank).locationChanged(ABOUT_BLANK)
        assertEquals(bank, s.pendingUrl)
        assertFalse(s.committed)
        s = s.locationChanged(ABOUT_BLANK)
        assertTrue(s.committed)
        assertEquals(ABOUT_BLANK, s.url)
    }
}
