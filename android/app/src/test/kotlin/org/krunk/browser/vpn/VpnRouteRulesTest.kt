package org.krunk.browser.vpn

import java.net.URL
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class VpnRouteRulesTest {
    @After
    fun reset() {
        VpnRoute.state = VpnRouteState()
    }

    /** Every combination, against the rules written out once more in plain words. */
    @Test
    fun theFullMatrix() {
        val bools = listOf(false, true)
        var cases = 0
        val combos = bools.flatMap { a -> bools.map { b -> a to b } }
        for ((on, connected) in combos) {
            for (scope in VpnScope.values()) {
                for ((priv, local) in combos) {
                    for ((lan, lanDirect) in combos) {
                        check(on, connected, scope, priv, local, lan, lanDirect)
                        cases++
                    }
                }
            }
        }
        assertEquals(128, cases)
    }

    private fun check(
        on: Boolean,
        connected: Boolean,
        scope: VpnScope,
        priv: Boolean,
        local: Boolean,
        lan: Boolean,
        lanDirect: Boolean,
    ) {
        val expected = when {
            !on -> Route.DIRECT
            local -> Route.DIRECT
            scope == VpnScope.PRIVATE && !priv -> Route.DIRECT
            lan && lanDirect -> Route.DIRECT
            connected -> Route.PROXY
            else -> Route.BLOCKED
        }
        val got = VpnRouteRules.decide(on, connected, scope, priv, local, lan, lanDirect)
        assertEquals("on=$on c=$connected $scope priv=$priv local=$local lan=$lan lanDirect=$lanDirect", expected, got)
    }

    @Test
    fun theDecisionsOfThePlan() {
        // Off: nothing changes.
        assertEquals(Route.DIRECT, VpnRouteRules.decide(false, false, VpnScope.ALL, false, false, false, false))
        // Everywhere: sync, update, favicons, suggestions, the probe through the VPN...
        assertEquals(Route.PROXY, VpnRouteRules.decide(true, true, VpnScope.ALL, false, false, false, false))
        // ...and refused while it is down (fail-closed).
        assertEquals(Route.BLOCKED, VpnRouteRules.decide(true, false, VpnScope.ALL, false, false, false, false))
        // Private tabs only: Krunk's own requests and normal tabs direct, private tabs through it.
        assertEquals(Route.DIRECT, VpnRouteRules.decide(true, false, VpnScope.PRIVATE, false, false, false, false))
        assertEquals(Route.PROXY, VpnRouteRules.decide(true, true, VpnScope.PRIVATE, true, false, false, false))
        assertEquals(Route.BLOCKED, VpnRouteRules.decide(true, false, VpnScope.PRIVATE, true, false, false, false))
        // Localhost always direct; the LAN only with the option.
        assertEquals(Route.DIRECT, VpnRouteRules.decide(true, false, VpnScope.ALL, true, true, false, false))
        assertEquals(Route.PROXY, VpnRouteRules.decide(true, true, VpnScope.ALL, false, false, true, false))
        assertEquals(Route.DIRECT, VpnRouteRules.decide(true, false, VpnScope.ALL, false, false, true, true))
    }

    @Test
    fun localHostsAndTheLocalNetwork() {
        for (h in listOf("localhost", "LOCALHOST", "app.localhost", "127.0.0.1", "127.1.2.3", "::1", "[::1]")) {
            assertTrue(h, VpnHosts.isLocalHost(h))
            assertFalse(h, VpnHosts.isLan(h))
        }
        for (h in listOf(
            "10.0.0.1", "172.16.0.1", "172.31.255.255", "192.168.1.1", "169.254.1.1", "printer",
            "nas.local", "router.lan", "x.home.arpa", "svc.internal", "fd00::1", "[fe80::1]", "FC00::2",
        )) {
            assertTrue(h, VpnHosts.isLan(h))
            assertFalse(h, VpnHosts.isLocalHost(h))
        }
        for (h in listOf(
            "example.com", "8.8.8.8", "172.32.0.1", "100.64.0.1", "192.169.0.1", "2001:db8::1",
            "localhost.example.com", "local.com", "128.0.0.1", "1.2.3.4.5",
        )) {
            assertFalse(h, VpnHosts.isLan(h))
            assertFalse(h, VpnHosts.isLocalHost(h))
        }
    }

    @Test
    fun theStateDecidesForTheCallSites() {
        val on = VpnRouteState(vpnOn = true, connected = true, port = 1234, tokenHex = "ab".repeat(32))
        assertEquals(Route.PROXY, VpnRoute.decide("example.com", false, on))
        // "connected" without a port or token is not connected.
        assertEquals(Route.BLOCKED, VpnRoute.decide("example.com", false, on.copy(port = null)))
        assertEquals(Route.BLOCKED, VpnRoute.decide("example.com", false, on.copy(tokenHex = null)))
        assertEquals(Route.DIRECT, VpnRoute.decide("127.0.0.1", false, on.copy(connected = false)))
        assertEquals(Route.DIRECT, VpnRoute.decide("example.com", false, on.copy(scope = VpnScope.PRIVATE)))
        // The token never shows in the state's text.
        assertFalse(on.toString().contains("abab"))
    }

    @Test
    fun openRefusesWhileDownAndIsPlainWhileOff() {
        VpnRoute.state = VpnRouteState(vpnOn = true, connected = false)
        try {
            VpnRoute.open(URL("https://example.com/"), forPrivateTab = false)
            fail("opened while the VPN is down")
        } catch (e: VpnNotConnected) {
            assertEquals("VPN not connected", e.message)
        }
        VpnRoute.state = VpnRouteState()
        val conn = VpnRoute.open(URL("http://example.com/"), forPrivateTab = false)
        assertFalse(conn.usingProxy())
        assertNull(conn.getRequestProperty("Proxy-Authorization"))
    }

    @Test
    fun theCredentialHeader() {
        // base64("krunk:" + 64 x "0")
        val hex = "0".repeat(64)
        val expected = "Basic " + java.util.Base64.getEncoder().encodeToString("krunk:$hex".toByteArray())
        assertEquals(expected, VpnRoute.proxyAuthorization(hex))
        assertTrue(expected.startsWith("Basic a3J1bms6"))
    }
}
