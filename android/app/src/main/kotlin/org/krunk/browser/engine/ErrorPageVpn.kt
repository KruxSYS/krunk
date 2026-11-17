package org.krunk.browser.engine

import org.mozilla.geckoview.WebRequestError

/** Krunk's own error pages while its VPN is on (plan-android-vpn 6.3). */
enum class VpnErrorKind {
    /** The VPN is on but not connected: the page was held back, nothing left outside it. */
    NOT_CONNECTED,

    /** Connected, but the VPN's proxy refused: outside the profile's routes or no such name. */
    NOT_REACHABLE,
}

/**
 * Which VPN page a failed load gets, if any. Pure (the constants are compile-time ints).
 *
 * Gecko does not report the SOCKS proxy's own reply codes as a proxy error (spike S2: replies
 * 1, 2, 5 arrive as CONNECTION_REFUSED, 4 as UNKNOWN_HOST), so the page is chosen from Krunk's
 * VPN state ([NOT_CONNECTED]) and from the proxy's short record of hosts it refused
 * (`KrunkVpn.refusedRecently`, [NOT_REACHABLE]); the guard's own cancel after its hold time is a
 * proxy-category error.
 */
object ErrorPageVpn {
    fun kind(vpnOn: Boolean, connected: Boolean, errorCode: Int, refusedRecently: Boolean): VpnErrorKind? = when {
        !vpnOn -> null
        !connected -> if (networkish(errorCode)) VpnErrorKind.NOT_CONNECTED else null
        refusedRecently && refusalCode(errorCode) -> VpnErrorKind.NOT_REACHABLE
        else -> null
    }

    /** How Gecko shows the proxy's "outside the routes" (2) and "name not found" (4) replies. */
    private fun refusalCode(code: Int): Boolean =
        code == WebRequestError.ERROR_CONNECTION_REFUSED || code == WebRequestError.ERROR_UNKNOWN_HOST

    /** Errors a missing tunnel can cause: the connection, the name, the proxy, being offline. */
    fun networkish(code: Int): Boolean = code == WebRequestError.ERROR_CONNECTION_REFUSED ||
        code == WebRequestError.ERROR_UNKNOWN_HOST ||
        code == WebRequestError.ERROR_NET_TIMEOUT ||
        code == WebRequestError.ERROR_NET_RESET ||
        code == WebRequestError.ERROR_NET_INTERRUPT ||
        code == WebRequestError.ERROR_OFFLINE ||
        // The low four bits of a GeckoView error code are its category.
        (code and CATEGORY_MASK) == WebRequestError.ERROR_CATEGORY_PROXY

    /** The host of [url] as the VPN proxy records it (lower case, no port, no brackets). */
    fun host(url: String): String? {
        val authority = Regex("""^[A-Za-z][A-Za-z0-9+.-]*://([^/?#\\]*)""").find(url)?.groupValues?.get(1)
            ?: return null
        val hostPort = authority.substringAfterLast('@')
        val host = if (hostPort.startsWith("[")) {
            hostPort.substringAfter('[').substringBefore(']')
        } else {
            hostPort.substringBefore(':')
        }
        return host.lowercase().ifEmpty { null }
    }

    fun title(kind: VpnErrorKind): String = when (kind) {
        VpnErrorKind.NOT_CONNECTED -> "VPN not connected"
        VpnErrorKind.NOT_REACHABLE -> "Not reachable through this VPN"
    }

    fun message(kind: VpnErrorKind): String = when (kind) {
        VpnErrorKind.NOT_CONNECTED ->
            "Krunk’s VPN is on but not connected, so this page was not loaded. Nothing was sent outside the VPN."

        VpnErrorKind.NOT_REACHABLE ->
            "This address is outside what your VPN profile routes, or its name was not found through the VPN."
    }

    /** The page's buttons besides Retry. */
    fun actions(kind: VpnErrorKind): List<ErrorPageAction> = when (kind) {
        VpnErrorKind.NOT_CONNECTED -> listOf(ErrorPageAction.OpenVpnSettings)
        VpnErrorKind.NOT_REACHABLE -> emptyList()
    }

    private const val CATEGORY_MASK = 0xF
}
