package org.krunk.browser.engine

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.mozilla.geckoview.WebRequestError

class ErrorPageVpnTest {
    private val networkish = listOf(
        WebRequestError.ERROR_CONNECTION_REFUSED,
        WebRequestError.ERROR_UNKNOWN_HOST,
        WebRequestError.ERROR_NET_TIMEOUT,
        WebRequestError.ERROR_NET_RESET,
        WebRequestError.ERROR_NET_INTERRUPT,
        WebRequestError.ERROR_OFFLINE,
        WebRequestError.ERROR_PROXY_CONNECTION_REFUSED,
        WebRequestError.ERROR_UNKNOWN_PROXY_HOST,
    )

    @Test
    fun withTheVpnOffNothingChanges() {
        for (code in networkish) {
            assertNull(ErrorPageVpn.kind(vpnOn = false, connected = false, errorCode = code, refusedRecently = true))
        }
    }

    @Test
    fun onButNotConnectedNetworkErrorsSayVpnNotConnected() {
        for (code in networkish) {
            assertEquals(VpnErrorKind.NOT_CONNECTED, ErrorPageVpn.kind(true, false, code, false))
        }
        // A certificate error or a crashed page is not the VPN's doing.
        assertNull(ErrorPageVpn.kind(true, false, WebRequestError.ERROR_SECURITY_BAD_CERT, false))
        assertNull(ErrorPageVpn.kind(true, false, WebRequestError.ERROR_CONTENT_CRASHED, false))
        assertNull(ErrorPageVpn.kind(true, false, WebRequestError.ERROR_MALFORMED_URI, false))
    }

    @Test
    fun connectedRefusalsOfTheProxySayNotReachable() {
        assertEquals(
            VpnErrorKind.NOT_REACHABLE,
            ErrorPageVpn.kind(true, true, WebRequestError.ERROR_CONNECTION_REFUSED, true),
        )
        assertEquals(
            VpnErrorKind.NOT_REACHABLE,
            ErrorPageVpn.kind(true, true, WebRequestError.ERROR_UNKNOWN_HOST, true),
        )
        // Not refused by the proxy: the site's own refusal, the normal page.
        assertNull(ErrorPageVpn.kind(true, true, WebRequestError.ERROR_CONNECTION_REFUSED, false))
        assertNull(ErrorPageVpn.kind(true, true, WebRequestError.ERROR_NET_TIMEOUT, true))
    }

    @Test
    fun proxyCategoryIsReadFromTheCode() {
        assertTrue(ErrorPageVpn.networkish(WebRequestError.ERROR_PROXY_CONNECTION_REFUSED))
        assertFalse(ErrorPageVpn.networkish(WebRequestError.ERROR_HTTPS_ONLY))
    }

    @Test
    fun hostsAreReadAsTheProxyRecordsThem() {
        assertEquals("example.com", ErrorPageVpn.host("https://User:pw@Example.COM:8443/a?b#c"))
        assertEquals("::1", ErrorPageVpn.host("http://[::1]:80/"))
        assertEquals("inside.test", ErrorPageVpn.host("http://inside.test"))
        assertNull(ErrorPageVpn.host("about:blank"))
    }

    @Test
    fun textsAndButtons() {
        assertEquals("VPN not connected", ErrorPageVpn.title(VpnErrorKind.NOT_CONNECTED))
        assertTrue(ErrorPageVpn.message(VpnErrorKind.NOT_CONNECTED).contains("Nothing was sent outside the VPN."))
        assertEquals("Not reachable through this VPN", ErrorPageVpn.title(VpnErrorKind.NOT_REACHABLE))
        assertEquals(listOf(ErrorPageAction.OpenVpnSettings), ErrorPageVpn.actions(VpnErrorKind.NOT_CONNECTED))
        assertEquals(emptyList<ErrorPageAction>(), ErrorPageVpn.actions(VpnErrorKind.NOT_REACHABLE))
    }

    @Test
    fun onlyATappedButtonOnAnErrorPageReachesTheApp() {
        val got = ArrayList<ErrorPageAction>()
        ErrorPage.onAction = { got += it }
        try {
            val url = ErrorPageAction.OpenVpnSettings.url
            assertEquals("krunk://error-action/open-vpn-settings", url)
            assertTrue(ErrorPage.dispatch(url, showsError = true, userGesture = true))
            // A page navigating there by itself, or without an error shown: refused, not passed on.
            assertTrue(ErrorPage.dispatch(url, showsError = true, userGesture = false))
            assertTrue(ErrorPage.dispatch(url, showsError = false, userGesture = true))
            assertFalse(ErrorPage.dispatch("https://example.com/", showsError = true, userGesture = true))
            assertEquals(listOf<ErrorPageAction>(ErrorPageAction.OpenVpnSettings), got)
        } finally {
            ErrorPage.onAction = null
        }
    }
}
