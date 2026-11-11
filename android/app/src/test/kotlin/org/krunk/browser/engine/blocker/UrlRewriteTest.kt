package org.krunk.browser.engine.blocker

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class UrlRewriteTest {
    @Test
    fun trackingParametersGoAndEverythingElseStays() {
        val table = listOf(
            "https://example.com/landing.html?utm_source=news&id=5&fbclid=abc123" to
                "https://example.com/landing.html?id=5",
            "http://10.250.136.3:18080/landing.html?utm_campaign=y&keep=1&gclid=zz" to
                "http://10.250.136.3:18080/landing.html?keep=1",
            "https://example.com/?utm_medium=mail" to "https://example.com/",
            "https://example.com/a?b=1#utm_source=x" to null,
            "https://example.com/a?id=9" to null,
            "https://example.com/plain" to null,
            // HTTPS upgrading is the engine's business: http stays http.
            "http://example.com/a?id=9" to null,
            // Not web addresses.
            "about:blank?utm_source=x" to null,
            "file:///sdcard/a.html?utm_source=x" to null,
            "data:text/html,<a href='?utm_source=x'>" to null,
        )
        for ((url, want) in table) assertEquals(url, want, UrlRewrite.cleaned(url, stripTrackingParams = true))
    }

    @Test
    fun aCleanedAddressIsNotCleanedAgain() {
        // What the rewriter redirects to passes the rewriter again: it must come back as "leave it".
        val first = UrlRewrite.cleaned("https://example.com/p?utm_source=a&x=1&mc_eid=9&y=2", true)
        assertEquals("https://example.com/p?x=1&y=2", first)
        assertNull(UrlRewrite.cleaned(first!!, true))
    }

    @Test
    fun withTheSettingOffNothingIsRewritten() {
        assertNull(UrlRewrite.cleaned("https://example.com/?utm_source=news", stripTrackingParams = false))
    }

    @Test
    fun veryLongAddressesAreLeftAlone() {
        val long = "https://example.com/?utm_source=x&q=" + "a".repeat(UrlRewrite.MAX_URL)
        assertNull(UrlRewrite.cleaned(long, true))
    }
}
