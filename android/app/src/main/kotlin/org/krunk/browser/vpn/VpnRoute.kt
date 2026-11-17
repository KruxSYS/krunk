package org.krunk.browser.vpn

import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL

/** Krunk's own request may not go out: the VPN is on but its proxy is not up (fail-closed). */
class VpnNotConnected : IOException("VPN not connected")

/** Where one of Krunk's own requests goes. */
enum class Route { DIRECT, PROXY, BLOCKED }

/**
 * Plan decisions 5, 6 and 8 (contract §5), for Krunk's own requests (sync, update, favicons,
 * search suggestions, the certificate probe). Pure.
 */
object VpnRouteRules {
    fun decide(
        vpnOn: Boolean,
        connected: Boolean,
        scope: VpnScope,
        forPrivateTab: Boolean,
        isLocalHost: Boolean,
        isLan: Boolean,
        lanDirect: Boolean,
    ): Route = when {
        !vpnOn -> Route.DIRECT

        // Never leaves the device.
        isLocalHost -> Route.DIRECT

        // "Private tabs only": normal tabs and Krunk's own requests go direct (Windows 0.22.0).
        scope == VpnScope.PRIVATE && !forPrivateTab -> Route.DIRECT

        isLan && lanDirect -> Route.DIRECT

        !connected -> Route.BLOCKED

        else -> Route.PROXY
    }
}

/**
 * Which hosts are "this device" and "the local network", by name or address literal (nothing is
 * resolved). The same list as Windows' `LOCAL_NETWORK_BYPASS` (`core/src/settings.rs`): names
 * without a dot, `*.local`, `*.lan`, `*.home.arpa`, `*.internal`, 10/8, 172.16/12, 192.168/16,
 * 169.254/16, fc00::/7, fe80::/10 (100.64/10 is left out: VPN providers use it in the tunnel).
 */
object VpnHosts {
    private fun bare(host: String): String = host.trim().lowercase().removePrefix("[").removeSuffix("]").trimEnd('.')

    fun isLocalHost(host: String): Boolean {
        val h = bare(host)
        if (h == "localhost" || h.endsWith(".localhost")) return true
        if (h == "::1" || h == "0:0:0:0:0:0:0:1") return true
        val v4 = ipv4(h) ?: return false
        return v4[0] == 127
    }

    fun isLan(host: String): Boolean {
        val h = bare(host)
        if (h.isEmpty() || isLocalHost(h)) return false
        ipv4(h)?.let { (a, b) ->
            return a == 10 || (a == 172 && b in 16..31) || (a == 192 && b == 168) || (a == 169 && b == 254)
        }
        if (':' in h) {
            val first = h.substringBefore(':').ifEmpty { "0" }.toIntOrNull(16) ?: return false
            return (first and 0xfe00) == 0xfc00 || (first and 0xffc0) == 0xfe80
        }
        if ('.' !in h) return true
        return listOf(".local", ".lan", ".home.arpa", ".internal").any { h.endsWith(it) }
    }

    private fun ipv4(h: String): List<Int>? {
        val parts = h.split('.')
        if (parts.size != 4) return null
        val n = parts.map { p ->
            p.toIntOrNull()?.takeIf { it in 0..255 && p.isNotEmpty() && p.all(Char::isDigit) }
                ?: return null
        }
        return n
    }
}

/**
 * The route of Krunk's own requests right now: set by [VpnController] on every change, read by
 * the call sites through [VpnRoute.open] and [VpnRoute.decide].
 */
data class VpnRouteState(
    val vpnOn: Boolean = false,
    /** The proxy is up and the tunnel connected. */
    val connected: Boolean = false,
    val scope: VpnScope = VpnScope.ALL,
    val lanDirect: Boolean = false,
    val port: Int? = null,
    /** The session's proxy token, 64 lowercase hex chars. */
    val tokenHex: String? = null,
) {
    override fun toString() = "VpnRouteState(on=$vpnOn, connected=$connected, scope=$scope, lan=$lanDirect)"
}

object VpnRoute {
    @Volatile
    var state: VpnRouteState = VpnRouteState()
        internal set

    fun decide(host: String, forPrivateTab: Boolean, s: VpnRouteState = state): Route = VpnRouteRules.decide(
        vpnOn = s.vpnOn,
        connected = s.connected && s.port != null && s.tokenHex != null,
        scope = s.scope,
        forPrivateTab = forPrivateTab,
        isLocalHost = VpnHosts.isLocalHost(host),
        isLan = VpnHosts.isLan(host),
        lanDirect = s.lanDirect,
    )

    /**
     * Opens [url] the way the VPN says: direct (VPN off, out of scope, localhost, LAN with the
     * option), through its SOCKS5 proxy with the session's credential ([VpnHttp] over
     * [VpnSocks]; the proxy resolves the name), or not at all ([VpnNotConnected]). Never the
     * platform's proxy support or `java.net.Authenticator` (process-wide). With the VPN off this
     * is exactly `url.openConnection()`.
     */
    @Throws(IOException::class)
    fun open(url: URL, forPrivateTab: Boolean): HttpURLConnection {
        val s = state
        return when (decide(url.host, forPrivateTab, s)) {
            Route.DIRECT -> url.openConnection() as HttpURLConnection

            Route.BLOCKED -> throw VpnNotConnected()

            Route.PROXY -> {
                val port = s.port ?: throw VpnNotConnected()
                val token = s.tokenHex ?: throw VpnNotConnected()
                VpnHttp(url, port, token)
            }
        }
    }

    /** Throws [VpnNotConnected] when a request to [host] must not go out now. */
    @Throws(VpnNotConnected::class)
    fun check(host: String, forPrivateTab: Boolean) {
        if (decide(host, forPrivateTab) == Route.BLOCKED) throw VpnNotConnected()
    }
}
