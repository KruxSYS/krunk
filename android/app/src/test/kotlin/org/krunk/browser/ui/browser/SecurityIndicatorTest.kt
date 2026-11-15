package org.krunk.browser.ui.browser

import org.junit.Assert.assertEquals
import org.junit.Test

class SecurityIndicatorTest {
    private fun ind(
        url: String,
        secure: Boolean?,
        loading: Boolean = false,
        pending: Boolean = false,
        internal: Boolean = false,
        loadError: Boolean = false,
    ) = securityIndicator(url, internal, secure, loading, loadError, pending)

    @Test
    fun lockNeedsHttpsAndTheEnginesWord() {
        assertEquals(SecurityIndicator.SECURE, ind("https://example.com/", true))
        assertEquals(SecurityIndicator.INSECURE, ind("https://example.com/", false))
        assertEquals(SecurityIndicator.NONE, ind("https://example.com/", null))
        // A stale "secure" can never put a lock on a plain http address.
        assertEquals(SecurityIndicator.INSECURE, ind("http://example.com/", true))
        assertEquals(SecurityIndicator.INSECURE, ind("http://example.com/", null))
        assertEquals(SecurityIndicator.INSECURE, ind("HTTP://EXAMPLE.COM/", false))
    }

    @Test
    fun anAddressThatIsNotLoadedYetShowsNothing() {
        assertEquals(SecurityIndicator.NONE, ind("https://bank.example/", true, pending = true))
        assertEquals(SecurityIndicator.NONE, ind("http://bank.example/", false, pending = true))
        assertEquals(SecurityIndicator.NONE, ind("http://example.com/", null, loading = true))
        assertEquals(SecurityIndicator.NONE, ind("https://example.com/", null, loading = true))
    }

    @Test
    fun internalErrorAndOtherPagesShowNothing() {
        assertEquals(SecurityIndicator.NONE, ind("https://krunk.invalid/settings.html", true, internal = true))
        assertEquals(SecurityIndicator.NONE, ind("https://example.com/", true, loadError = true))
        assertEquals(SecurityIndicator.NONE, ind("", true))
        assertEquals(SecurityIndicator.NONE, ind("about:blank", true))
        assertEquals(SecurityIndicator.NONE, ind("data:text/html,x", true))
        assertEquals(SecurityIndicator.NONE, ind("file:///sdcard/a.html", true))
    }

    @Test
    fun thePillShowsTheHostOfAWebPage() {
        assertEquals("example.com", pillText("https://example.com/"))
        assertEquals("example.com", pillText("https://EXAMPLE.com/Some/Path?q=1#x"))
        assertEquals("login.bank.example.evil.tld", pillText("https://login.bank.example.evil.tld/bank.example/"))
        assertEquals("example.com:8080", pillText("http://example.com:8080/x"))
        assertEquals("[2001:db8::1]", pillText("https://[2001:db8::1]/x"))
        assertEquals("xn--bcher-kva.example", pillText("https://xn--bcher-kva.example/"))
    }

    @Test
    fun thePillShowsTheWholeAddressAsHostAndRest() {
        assertEquals(PillParts("example.com", ""), pillParts("https://example.com/"))
        assertEquals(PillParts("example.com", "/Some/Path?q=1#x"), pillParts("https://EXAMPLE.com/Some/Path?q=1#x"))
        assertEquals(PillParts("example.com", "?q=1"), pillParts("https://example.com?q=1"))
        assertEquals(PillParts("example.com:8080", "/x"), pillParts("http://example.com:8080/x"))
        // The host is never taken from the path or the user part.
        assertEquals(
            PillParts("login.bank.example.evil.tld", "/bank.example/"),
            pillParts("https://login.bank.example.evil.tld/bank.example/"),
        )
        assertEquals(PillParts("evil.example", "/login"), pillParts("https://bank.example:pw@evil.example/login"))
        assertEquals(PillParts("example.com", "/ab"), pillParts("https://example.com/a‮b"))
        // Not a web page: all of it in the host part.
        assertEquals(PillParts("krunk://settings", ""), pillParts("krunk://settings"))
    }

    @Test
    fun thePillNeverShowsTheUserPart() {
        assertEquals("evil.example", pillText("https://bank.example@evil.example/"))
        assertEquals("evil.example", pillText("https://bank.example:pw@evil.example/login"))
        assertEquals("evil.example", pillText("https://a@b@evil.example/"))
    }

    @Test
    fun thePillDropsCharactersThatReorderText() {
        assertEquals("evil.elpmaxe.example", pillText("https://evil.‮elpmaxe.example/"))
        assertEquals("example.com", pillText("https://exa​mple.com/"))
    }

    @Test
    fun otherAddressesAreShownAsTheyAreButBounded() {
        assertEquals("krunk://settings", pillText("krunk://settings"))
        assertEquals("", pillText(""))
        assertEquals("file:///sdcard/a.html", pillText("file:///sdcard/a.html"))
        assertEquals(2000, pillText("data:text/html," + "a".repeat(100_000)).length)
    }

    @Test
    fun suggestionRowsKeepThePathAndMarkHttp() {
        assertEquals("example.com", addressText("https://example.com/"))
        assertEquals("example.com/a/b", addressText("https://example.com/a/b"))
        assertEquals("http://example.com/x", addressText("http://example.com/x"))
        // Bounded before the scheme is dropped: 2000 characters minus "https://".
        assertEquals(1992, addressText("https://example.com/" + "a".repeat(5000)).length)
    }
}
