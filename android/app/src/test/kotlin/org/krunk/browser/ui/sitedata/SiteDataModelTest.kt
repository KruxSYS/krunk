package org.krunk.browser.ui.sitedata

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.browser.engine.EngineSiteData
import org.krunk.browser.engine.OriginUsage
import org.krunk.ffi.KrunkStore
import org.krunk.ffi.isSiteName
import org.krunk.ffi.siteIsRegistrable
import org.krunk.ffi.siteOfHost
import org.krunk.ffi.webOrigin

class SiteDataModelTest {
    /** A store with two visited sites and one site that only holds a permission answer. */
    private fun store(): KrunkStore = KrunkStore.inMemory().also { s ->
        s.recordVisit("https://www.news.example/a", "A", 1_000)
        s.recordVisit("https://m.news.example/b", "B", 9_000)
        s.recordVisit("https://plain.test/", "Plain", 5_000)
        s.sitePermissionSet("https://cam.test/", "camera", true, 1)
    }

    private fun list(store: KrunkStore, engine: EngineSiteData?, openTabs: List<String> = emptyList()): SiteList {
        val summaries =
            store.siteSummaries(SiteDataModel.cookieDomains(engine), SiteDataModel.extraOrigins(engine, openTabs))
        return SiteDataModel.build(summaries, engine, store.sitePermissionsList().map { it.origin }, ::siteOfHost)
    }

    private val full = EngineSiteData(
        cookies = mapOf("www.news.example" to 2, "news.example" to 1, "tracker.test" to 4),
        usage = listOf(
            OriginUsage("https://www.news.example", "", 1_000),
            OriginUsage("https://cdn.tracker.test", "news.example", 300),
            OriginUsage("https://app.storage.example.co.uk", "", 70),
            OriginUsage("https://gone.test", "forgotten.test", 5),
        ),
    )

    @Test
    fun withoutTheEngineTheSitesComeFromKrunksOwnRecords() {
        val s = store()
        val l = list(s, null, openTabs = listOf("http://10.0.2.2:18080"))
        assertEquals(SiteSource.HISTORY, l.source)
        assertEquals(listOf("10.0.2.2", "cam.test", "news.example", "plain.test"), l.rows.map { it.site })
        // Nothing is counted, and nothing is claimed to be zero.
        assertTrue(l.rows.all { it.cookies == null && it.bytes == null })
        val news = l.rows.first { it.site == "news.example" }
        assertEquals(listOf("https://m.news.example", "https://www.news.example"), news.origins)
        assertEquals(9_000L, news.lastUsed)
        assertTrue(l.rows.first { it.site == "cam.test" }.hasPermissions)
        assertFalse(news.hasPermissions)
        s.destroy()
    }

    @Test
    fun theEnginesAnswerGivesCountsAndLeavesOutSitesThatHoldNothing() {
        val s = store()
        val l = list(s, full, openTabs = listOf("https://open.test"))
        assertEquals(SiteSource.ENGINE, l.source)
        // plain.test is only in the history and open.test only in a tab: nothing to clear there.
        assertEquals(
            listOf("cam.test", "example.co.uk", "news.example", "tracker.test", "forgotten.test"),
            l.rows.map { it.site },
        )
        val news = l.rows.first { it.site == "news.example" }
        assertEquals(3, news.cookies)
        // Its own data and what a third party stored while embedded in it.
        assertEquals(1_300L, news.bytes)
        assertEquals(9_000L, news.lastUsed)
        val storage = l.rows.first { it.site == "example.co.uk" }
        assertEquals(
            Triple(0, 70L, listOf("https://app.storage.example.co.uk")),
            Triple(storage.cookies, storage.bytes, storage.origins),
        )
        val tracker = l.rows.first { it.site == "tracker.test" }
        assertEquals(Triple(4, 0L, emptyList<String>()), Triple(tracker.cookies, tracker.bytes, tracker.origins))
        // Data kept under a site Krunk has no record of is still listed, so it can be cleared.
        val forgotten = l.rows.first { it.site == "forgotten.test" }
        assertEquals(Triple(0, 5L, emptyList<String>()), Triple(forgotten.cookies, forgotten.bytes, forgotten.origins))
        val cam = l.rows.first { it.site == "cam.test" }
        assertEquals(Triple(0, 0L, true), Triple(cam.cookies, cam.bytes, cam.hasPermissions))
        s.destroy()
    }

    @Test
    fun halfAnAnswerKeepsEverySiteAndSaysWhatIsUnknown() {
        val s = store()
        val l = list(s, full.copy(usage = null))
        assertEquals(SiteSource.PARTIAL, l.source)
        assertEquals(listOf("cam.test", "news.example", "plain.test", "tracker.test"), l.rows.map { it.site })
        val plain = l.rows.first { it.site == "plain.test" }
        assertEquals(0, plain.cookies)
        assertNull(plain.bytes)
        val sizesOnly = list(s, full.copy(cookies = null))
        assertEquals(SiteSource.PARTIAL, sizesOnly.source)
        assertNull(sizesOnly.rows.first { it.site == "news.example" }.cookies)
        assertEquals(1_300L, sizesOnly.rows.first { it.site == "news.example" }.bytes)
        // An answer with neither part is no answer.
        assertEquals(SiteSource.HISTORY, list(s, EngineSiteData(null, null)).source)
        s.destroy()
    }

    @Test
    fun oneEntryPerCookieAndACapOnNonsense() {
        assertEquals(emptyList<String>(), SiteDataModel.cookieDomains(null))
        assertEquals(
            listOf("a.test", "a.test", "b.test"),
            SiteDataModel.cookieDomains(
                EngineSiteData(mapOf("a.test" to 2, "b.test" to 1, "" to 3, "c.test" to -1), null),
            ),
        )
        val huge = SiteDataModel.cookieDomains(EngineSiteData(mapOf("a.test" to Int.MAX_VALUE, "b.test" to 5), null))
        assertEquals(SiteDataModel.MAX_COOKIES, huge.size)
        // Only data a site holds under its own name names an origin of that site.
        assertEquals(
            listOf("https://www.news.example", "https://app.storage.example.co.uk", "https://open.test"),
            SiteDataModel.extraOrigins(full, listOf("https://open.test", "https://www.news.example")),
        )
    }

    @Test
    fun sortingByNameAndByLastUse() {
        val s = store()
        val rows = list(s, null).rows
        assertEquals(
            listOf("cam.test", "news.example", "plain.test"),
            SiteDataModel.sorted(rows.shuffled(), SiteOrder.NAME).map { it.site },
        )
        // Most recent first; a site never visited comes last.
        assertEquals(
            listOf("news.example", "plain.test", "cam.test"),
            SiteDataModel.sorted(rows, SiteOrder.LAST_USED).map { it.site },
        )
        s.destroy()
    }

    @Test
    fun searchLooksAtTheSiteAndItsAddresses() {
        val s = store()
        val rows = list(s, null).rows
        assertEquals(rows, SiteDataModel.search(rows, "  "))
        assertEquals(listOf("news.example"), SiteDataModel.search(rows, " NEWS ").map { it.site })
        assertEquals(listOf("news.example"), SiteDataModel.search(rows, "m.news").map { it.site })
        assertEquals(listOf("cam.test", "plain.test"), SiteDataModel.search(rows, ".test").map { it.site })
        assertTrue(SiteDataModel.search(rows, "nothing").isEmpty())
        s.destroy()
    }

    @Test
    fun aClearedSiteLeavesTheEnginesListUnlessAPermissionIsKept() {
        val s = store()
        s.sitePermissionSet("https://www.news.example/", "geolocation", false, 2)
        val l = list(s, full)
        val kept = SiteDataModel.cleared(l, "news.example", withPermissions = false)
        val news = kept.rows.first { it.site == "news.example" }
        assertEquals(Triple(0, 0L, true), Triple(news.cookies, news.bytes, news.hasPermissions))
        val gone = SiteDataModel.cleared(l, "news.example", withPermissions = true)
        assertFalse(gone.rows.any { it.site == "news.example" })
        // The other sites are as they were.
        assertEquals(l.rows.filter { it.site != "news.example" }, gone.rows)
        assertEquals(l, SiteDataModel.cleared(l, "unknown.test", withPermissions = true))
        s.destroy()
    }

    @Test
    fun aClearedSiteStaysInTheListFromTheHistory() {
        val s = store()
        val history = SiteDataModel.cleared(list(s, null), "cam.test", withPermissions = true)
        val cam = history.rows.first { it.site == "cam.test" }
        assertFalse(cam.hasPermissions)
        assertNull(cam.cookies)
        val partial = SiteDataModel.cleared(list(s, full.copy(usage = null)), "news.example", withPermissions = true)
        val news = partial.rows.first { it.site == "news.example" }
        assertEquals(0, news.cookies)
        assertNull(news.bytes)
        s.destroy()
    }

    @Test
    fun whichOpenPagesBelongToASite() {
        val site = ::siteOfHost
        assertTrue(SiteDataModel.isOfSite("https://www.shop.example.co.uk/cart?x=1", "example.co.uk", site))
        assertTrue(SiteDataModel.isOfSite("http://10.0.2.2:18080/page", "10.0.2.2", site))
        assertFalse(SiteDataModel.isOfSite("https://example.co.uk.evil.test/", "example.co.uk", site))
        assertFalse(SiteDataModel.isOfSite("about:blank", "example.co.uk", site))
        assertFalse(SiteDataModel.isOfSite("file:///sdcard/example.co.uk", "example.co.uk", site))
        assertFalse(SiteDataModel.isOfSite("https://exa mple/", "example.co.uk", site))
        assertEquals("a.test", SiteDataModel.hostOf("https://a.test:8443"))
        assertNull(SiteDataModel.hostOf("not a url"))
    }

    @Test
    fun theExtensionsHostsAndOriginsAreChecked() {
        assertEquals(".news.example", SiteDataModel.checkedHost(".news.example", ::isSiteName))
        assertEquals("10.0.2.2", SiteDataModel.checkedHost("10.0.2.2", ::isSiteName))
        for (bad in listOf("", ".", "a b", "x/y", "evil.example/../", "<b>")) {
            assertNull(bad, SiteDataModel.checkedHost(bad, ::isSiteName))
        }
        assertEquals(
            "https://a.test:8443" to "",
            SiteDataModel.checkedUsage("https://a.test:8443", "", ::webOrigin, ::isSiteName),
        )
        assertEquals(
            "https://w.test" to "top.test",
            SiteDataModel.checkedUsage("https://w.test", "top.test", ::webOrigin, ::isSiteName),
        )
        assertNull(SiteDataModel.checkedUsage("moz-extension://x", "", ::webOrigin, ::isSiteName))
        assertNull(SiteDataModel.checkedUsage("file:///sdcard/x", "", ::webOrigin, ::isSiteName))
        assertNull(SiteDataModel.checkedUsage("https://a.test", "a b", ::webOrigin, ::isSiteName))
    }

    @Test
    fun aSuffixSiteIsClearedByItsHostOnly() {
        // GeckoEngine.clearSiteData: the base-domain call only for these.
        for (site in listOf("example.com", "example.co.uk", "10.0.2.2")) assertTrue(site, siteIsRegistrable(site))
        for (site in listOf("github.io", "dev", "co.uk", "localhost")) assertFalse(site, siteIsRegistrable(site))
    }
}
