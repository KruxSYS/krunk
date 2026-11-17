package org.krunk.browser.engine

import org.junit.After
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VpnLoadGuardTest {
    @After
    fun reset() {
        VpnLoadGuard.blocking = { false }
    }

    @Test
    fun onlyNetworkSchemesAreStopped() {
        VpnLoadGuard.blocking = { true }
        for (u in listOf("http://a.example/", "HTTPS://a.example/", "ws://a.example/s", "wss://a.example/s")) {
            assertTrue(u, VpnLoadGuard.refuses(u, privateTab = false))
        }
        for (u in listOf("about:blank", "data:text/html,x", "moz-extension://x/y", "krunk://vpn", "", "javascript:1")) {
            assertFalse(u, VpnLoadGuard.refuses(u, privateTab = false))
        }
    }

    @Test
    fun followsTheAppsAnswerPerTab() {
        VpnLoadGuard.blocking = { private -> private }
        assertTrue(VpnLoadGuard.refuses("https://a.example/", privateTab = true))
        assertFalse(VpnLoadGuard.refuses("https://a.example/", privateTab = false))
        VpnLoadGuard.blocking = { false }
        assertFalse(VpnLoadGuard.refuses("https://a.example/", privateTab = true))
    }
}
