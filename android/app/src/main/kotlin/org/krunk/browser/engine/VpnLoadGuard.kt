package org.krunk.browser.engine

/**
 * The app's own stop for page loads while Krunk's VPN is on but the engine is not routed through
 * it (0.10.0). The engine follows the VPN only through `krunk-guard`'s channel filter; if that
 * extension is missing (it failed to load, it was restarted, it has not answered yet) a page
 * would connect directly. While [blocking] says so, [KrunkSession] refuses the navigation and
 * shows "VPN not connected" instead: fail-closed even when the guard is gone.
 *
 * Covers navigations (pages, frames); requests of a page that was already open when the guard
 * went away are not seen here. The engine config's backstop covers a start with the VPN on.
 */
object VpnLoadGuard {
    /** Set by the app: true while the VPN is on, the tab is in its scope and the filter is not installed. */
    @Volatile
    var blocking: (privateTab: Boolean) -> Boolean = { false }

    /** Whether a navigation to [uri] must be refused now. Only network schemes are stopped. */
    fun refuses(uri: String, privateTab: Boolean): Boolean = isNetwork(uri) && blocking(privateTab)

    /** http, https, ws, wss (any case); everything else (about:, data:, moz-extension:, krunk:) is not. */
    fun isNetwork(uri: String): Boolean {
        val scheme = uri.substringBefore(':', "").lowercase()
        return scheme == "http" || scheme == "https" || scheme == "ws" || scheme == "wss"
    }
}
