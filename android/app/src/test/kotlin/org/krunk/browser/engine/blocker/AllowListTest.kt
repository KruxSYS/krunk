package org.krunk.browser.engine.blocker

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.ffi.KrunkStore

class AllowListTest {
    @Test
    fun allowingASiteCoversItsPagesAndSubdomainsUntilItIsRemoved() {
        val list = AllowList(KrunkStore.inMemory())
        assertTrue(list.hosts().isEmpty())
        assertNull(list.entryFor("https://news.example.com/article"))

        assertEquals("example.com", list.allow("https://Example.com/some/page?x=1"))
        assertEquals("example.com", list.entryFor("https://example.com/"))
        assertEquals("example.com", list.entryFor("http://news.example.com/article"))
        assertNull(list.entryFor("https://example.org/"))
        assertNull(list.entryFor("https://notexample.com/"))

        // A second entry, given as a host; the list is sorted.
        assertEquals("10.250.136.3", list.allow("10.250.136.3"))
        assertEquals(listOf("10.250.136.3", "example.com"), list.hosts())
        assertEquals("10.250.136.3", list.entryFor("http://10.250.136.3:18080/ads.html"))

        assertTrue(list.remove("example.com"))
        assertFalse(list.remove("example.com"))
        assertNull(list.entryFor("https://news.example.com/article"))
        assertEquals(listOf("10.250.136.3"), list.hosts())
    }

    @Test
    fun whatIsNotAWebPageCannotBeAllowed() {
        val list = AllowList(KrunkStore.inMemory())
        for (bad in listOf(
            "",
            "about:blank",
            "krunk://settings",
            "file:///sdcard/x.html",
            "data:text/html,hi",
            "two words",
        )) {
            assertNull(bad, list.allow(bad))
            assertNull(bad, AllowList.siteOf(bad))
        }
        assertTrue(list.hosts().isEmpty())
        assertEquals("example.com", AllowList.siteOf("https://user:secret@example.com:8443/path"))
    }

    @Test
    fun theListSurvivesClosingTheDatabase() {
        val dir = File(System.getProperty("java.io.tmpdir"), "krunk-allow-${System.nanoTime()}").apply { mkdirs() }
        try {
            val path = File(dir, "browser.db").path
            AllowList(KrunkStore.open(path)).allow("https://shop.example.net/cart")
            // A new process opens the same file (app restart, and an upgrade keeps the file).
            assertEquals(listOf("shop.example.net"), AllowList(KrunkStore.open(path)).hosts())
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test
    fun privateTabsOfferNoSwitch() {
        assertTrue(AllowList.switchOffered(privateTab = false, url = "https://example.com/"))
        assertFalse(AllowList.switchOffered(privateTab = true, url = "https://example.com/"))
        assertFalse(AllowList.switchOffered(privateTab = false, url = "about:blank"))
    }

    @Test
    fun theBlockerGetsItsOwnDefaultsPlusKrunksHostsAndNothingElse() {
        val defaults = listOf("chrome-extension-scheme", "moz-extension-scheme")
        val text = AllowList.trustedSites(defaults, listOf("example.com", "10.250.136.3", "not a host", "example.com"))
        assertEquals("chrome-extension-scheme\nmoz-extension-scheme\nexample.com\n10.250.136.3", text)
        assertEquals("chrome-extension-scheme\nmoz-extension-scheme", AllowList.trustedSites(defaults, emptyList()))

        // The read-back check: order does not matter, content does.
        val hosts = listOf("example.com")
        assertTrue(
            AllowList.sameSites(
                listOf("example.com", "moz-extension-scheme", "chrome-extension-scheme"),
                defaults,
                hosts,
            ),
        )
        assertFalse(AllowList.sameSites(defaults, defaults, hosts))
        assertFalse(AllowList.sameSites(defaults + "example.com" + "left.over", defaults, hosts))
    }
}
