package org.krunk.browser.engine

import android.content.Context
import android.util.Base64
import org.mozilla.geckoview.WebRequestError

/**
 * A button on Krunk's error page that the app carries out (the page is a `data:` document and
 * asks by navigating to [url]; the tab refuses that load and passes it to [ErrorPage.onAction]).
 */
sealed class ErrorPageAction(val path: String, val label: String) {
    val url: String get() = ACTION_BASE + path

    /** "Open VPN settings" on the "VPN not connected" page. */
    data object OpenVpnSettings : ErrorPageAction("open-vpn-settings", "Open VPN settings")

    companion object {
        const val ACTION_BASE = "krunk://error-action/"

        fun fromUrl(url: String): ErrorPageAction? = when (url) {
            OpenVpnSettings.url -> OpenVpnSettings
            else -> null
        }
    }
}

/**
 * Krunk's own error page (like the desktop one): assets/error.html with placeholders, returned
 * from `onLoadError` as a `data:` URI. Dark/light follows the page's `prefers-color-scheme`.
 * While Krunk's VPN is on, network errors get the VPN's own pages ([ErrorPageVpn]).
 */
class ErrorPage(context: Context) {
    private val template: String = context.assets.open("error.html").bufferedReader().use { it.readText() }

    fun dataUri(uri: String, category: Int, code: Int): String {
        val vpnKind = vpnSource?.let { vpn ->
            val on = vpn.vpnOn
            val connected = on && vpn.connected
            // Asked only when it can matter (a refusal while connected).
            val refused = on && connected && ErrorPageVpn.host(uri)?.let(vpn::refusedRecently) == true
            ErrorPageVpn.kind(on, connected, code, refused)
        }
        val (title, message) = if (vpnKind != null) {
            ErrorPageVpn.title(vpnKind) to ErrorPageVpn.message(vpnKind)
        } else {
            describe(category, code)
        }
        val buttons = buildString {
            append("<button onclick=\"location.reload()\">")
            append(if (vpnKind != null) "Retry" else "Try again")
            append("</button>")
            for (action in vpnKind?.let(ErrorPageVpn::actions).orEmpty()) {
                // The action's address is a constant of ours: nothing from the page goes in here.
                // A link, not a script: the engine marks a tapped link as a user gesture (a
                // location change from onclick was not delivered as one on the device).
                append("<a class=\"button secondary\" href=\"").append(escape(action.url)).append("\">")
                append(escape(action.label)).append("</a>")
            }
        }
        val html = template
            .replace("{{title}}", escape(title))
            .replace("{{message}}", escape(message))
            .replace("{{url}}", escape(uri))
            .replace("{{code}}", escape(codeName(code)))
            .replace("{{buttons}}", buttons)
        return "data:text/html;base64," + Base64.encodeToString(html.toByteArray(Charsets.UTF_8), Base64.NO_WRAP)
    }

    /** What the error page needs to know about Krunk's VPN (set by the app; main thread). */
    interface VpnSource {
        val vpnOn: Boolean
        val connected: Boolean

        /** `KrunkVpn.refusedRecently`: the proxy refused [host] within the last 30 s. */
        fun refusedRecently(host: String): Boolean
    }

    companion object {
        /** Krunk's VPN as the error page sees it; null: no VPN (normal pages only). */
        @Volatile
        var vpnSource: VpnSource? = null

        /** Gets the error page's buttons (besides Retry), on the main thread. */
        @Volatile
        var onAction: ((ErrorPageAction) -> Unit)? = null

        /**
         * A tab's load request to [url]: if it is one of the error page's buttons and the tab shows
         * an error page ([showsError]) after a tap ([userGesture]), [onAction] gets it. True when the
         * request was such a button (the load is then refused, whether or not it was passed on).
         */
        fun dispatch(url: String, showsError: Boolean, userGesture: Boolean): Boolean {
            val action = ErrorPageAction.fromUrl(url) ?: return false
            if (showsError && userGesture) onAction?.invoke(action)
            return true
        }
    }

    private fun describe(category: Int, code: Int): Pair<String, String> = when (code) {
        WebRequestError.ERROR_UNKNOWN_HOST ->
            "Can’t find this site" to
                "There is no site at this address. Check the spelling, or search for it instead."

        WebRequestError.ERROR_CONNECTION_REFUSED ->
            "Connection refused" to
                "The site is there but it does not accept connections right now."

        WebRequestError.ERROR_NET_TIMEOUT ->
            "The site took too long" to
                "The server did not answer in time. Try again in a moment."

        WebRequestError.ERROR_NET_INTERRUPT, WebRequestError.ERROR_NET_RESET ->
            "Connection interrupted" to
                "The connection was cut while the page was loading."

        WebRequestError.ERROR_OFFLINE -> "You’re offline" to "Connect to a network and try again."

        WebRequestError.ERROR_HTTPS_ONLY ->
            "HTTPS-only is on" to
                "This site only offers an insecure (HTTP) connection. Turn HTTPS-only off in Settings to continue."

        WebRequestError.ERROR_SECURITY_SSL, WebRequestError.ERROR_SECURITY_BAD_CERT ->
            "This connection is not secure" to
                "The site’s certificate could not be verified, so Krunk did not continue."

        WebRequestError.ERROR_SAFEBROWSING_MALWARE_URI, WebRequestError.ERROR_SAFEBROWSING_PHISHING_URI,
        WebRequestError.ERROR_SAFEBROWSING_UNWANTED_URI, WebRequestError.ERROR_SAFEBROWSING_HARMFUL_URI,
        -> "Blocked" to "This page was blocked."

        WebRequestError.ERROR_MALFORMED_URI ->
            "That is not a valid address" to
                "Krunk could not understand this address."

        WebRequestError.ERROR_PORT_BLOCKED ->
            "Port blocked" to
                "This address uses a port that browsers refuse for safety."

        WebRequestError.ERROR_CONTENT_CRASHED ->
            "The page crashed" to
                "Something went wrong while showing this page. Reload to try again."

        else -> when (category) {
            WebRequestError.ERROR_CATEGORY_NETWORK ->
                "Can’t reach this site" to
                    "A network problem stopped the page from loading."

            WebRequestError.ERROR_CATEGORY_SECURITY ->
                "Not secure" to
                    "A security check failed, so the page was not loaded."

            WebRequestError.ERROR_CATEGORY_CONTENT ->
                "The page could not be shown" to
                    "The content could not be displayed."

            else -> "Something went wrong" to "The page could not be loaded."
        }
    }

    private fun codeName(code: Int): String = when (code) {
        WebRequestError.ERROR_UNKNOWN_HOST -> "UNKNOWN_HOST"
        WebRequestError.ERROR_CONNECTION_REFUSED -> "CONNECTION_REFUSED"
        WebRequestError.ERROR_NET_TIMEOUT -> "NET_TIMEOUT"
        WebRequestError.ERROR_NET_INTERRUPT -> "NET_INTERRUPT"
        WebRequestError.ERROR_NET_RESET -> "NET_RESET"
        WebRequestError.ERROR_OFFLINE -> "OFFLINE"
        WebRequestError.ERROR_HTTPS_ONLY -> "HTTPS_ONLY"
        WebRequestError.ERROR_SECURITY_SSL -> "SECURITY_SSL"
        WebRequestError.ERROR_SECURITY_BAD_CERT -> "SECURITY_BAD_CERT"
        WebRequestError.ERROR_MALFORMED_URI -> "MALFORMED_URI"
        WebRequestError.ERROR_PORT_BLOCKED -> "PORT_BLOCKED"
        WebRequestError.ERROR_CONTENT_CRASHED -> "CONTENT_CRASHED"
        else -> "ERROR_$code"
    }

    private fun escape(s: String): String = buildString(s.length) {
        for (c in s) {
            when (c) {
                '&' -> append("&amp;")
                '<' -> append("&lt;")
                '>' -> append("&gt;")
                '"' -> append("&quot;")
                '\'' -> append("&#39;")
                else -> append(c)
            }
        }
    }
}
