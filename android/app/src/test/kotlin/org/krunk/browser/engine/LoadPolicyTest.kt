package org.krunk.browser.engine

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class LoadPolicyTest {
    @Test
    fun webAddressesAreAllowed() {
        for (url in listOf(
            "https://example.com",
            "https://example.com/",
            "http://example.com/a?b=c#d",
            "HTTPS://Example.COM/Path",
            "https://[2001:db8::1]:8443/x",
            "http://192.168.1.1:8080",
            "https://example.com/a%20b?q=%E2%9C%93",
        )) {
            assertTrue(url, LoadPolicy.isWebUrl(url))
        }
    }

    @Test
    fun everythingElseIsRefused() {
        for (url in listOf(
            "",
            "example.com",
            "file:///data/data/org.krunk.browser/files/krunk/browser.db",
            "file:///sdcard/Download/page.html",
            "content://com.android.providers.downloads/1",
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "blob:https://example.com/0000",
            "about:config",
            "about:blank",
            "resource://android/assets/",
            "intent://scan/#Intent;scheme=zxing;end",
            "krunk://settings",
            "view-source:https://example.com",
            "ftp://example.com/",
            "https://",
            "https:///path",
            "https://user:pw@example.com/",
            "https://bank.example@evil.example/",
            "https://example.com/a b",
            "https://example.com/\nfile:///etc/hosts",
            " https://example.com",
            "https://example.com\\@evil.example/",
            "https://example.com/" + "a".repeat(9000),
        )) {
            assertFalse(url.take(60), LoadPolicy.isWebUrl(url))
        }
    }

    @Test
    fun viewIntentsOpenOnlyWebAddresses() {
        assertEquals("https://example.com/x", LoadPolicy.fromViewIntent("https://example.com/x"))
        assertEquals("https://example.com/x", LoadPolicy.fromViewIntent("  https://example.com/x\n"))
        assertNull(LoadPolicy.fromViewIntent(null))
        assertNull(LoadPolicy.fromViewIntent("file:///sdcard/x.html"))
        assertNull(LoadPolicy.fromViewIntent("javascript:alert(1)"))
        assertNull(LoadPolicy.fromViewIntent("about:support"))
    }

    @Test
    fun sharedTextYieldsAWebAddressOrNothing() {
        assertEquals("https://example.com/a", LoadPolicy.urlInSharedText("https://example.com/a"))
        assertEquals("https://example.com/a", LoadPolicy.urlInSharedText("Page title https://example.com/a"))
        assertEquals("https://example.com/a", LoadPolicy.urlInSharedText("Look: https://example.com/a."))
        assertEquals("http://example.com/x?y=1", LoadPolicy.urlInSharedText("a\nhttp://example.com/x?y=1\nb"))
        assertNull(LoadPolicy.urlInSharedText("just some words"))
        assertNull(LoadPolicy.urlInSharedText("file:///data/data/org.krunk.browser/files/krunk/browser.db"))
        assertNull(LoadPolicy.urlInSharedText("javascript:alert(1)"))
        assertNull(LoadPolicy.urlInSharedText("see file:///etc/hosts or about:config"))
    }

    @Test
    fun sharedTextBecomesABoundedOneLineSearch() {
        assertEquals("two words", LoadPolicy.searchText("  two\n\twords "))
        assertEquals(512, LoadPolicy.searchText("x".repeat(5000)).length)
    }

    @Test
    fun onlyTheUserMayOpenOtherSchemes() {
        assertTrue(LoadPolicy.allowed("file:///sdcard/x.html", LoadSource.USER))
        assertTrue(LoadPolicy.allowed("krunk://settings", LoadSource.USER))
        assertFalse(LoadPolicy.allowed("file:///sdcard/x.html", LoadSource.EXTERNAL_APP))
        assertFalse(LoadPolicy.allowed("file:///sdcard/x.html", LoadSource.PAGE))
        assertFalse(LoadPolicy.allowed("data:text/html,x", LoadSource.PAGE))
        assertTrue(LoadPolicy.allowed("https://example.com/", LoadSource.PAGE))
        assertTrue(LoadPolicy.allowed("https://example.com/", LoadSource.EXTERNAL_APP))
    }
}
