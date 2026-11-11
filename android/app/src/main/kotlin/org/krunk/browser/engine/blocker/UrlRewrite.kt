package org.krunk.browser.engine.blocker

import org.krunk.ffi.navigationRewrite

/**
 * The answer to the guard's question "may this top-level address be cleaned?": the address
 * without its tracking parameters, by the shared core's rules (the same list as on Windows), or
 * null to leave the request as it is. Only web addresses, only when the setting is on; HTTPS
 * upgrading is the engine's business and is not done here.
 */
object UrlRewrite {
    /** Longer addresses are left alone (and never cross into the core). */
    const val MAX_URL = 8192

    fun cleaned(url: String, stripTrackingParams: Boolean): String? {
        if (!stripTrackingParams || url.length > MAX_URL) return null
        if (!url.startsWith("http://", ignoreCase = true) && !url.startsWith("https://", ignoreCase = true)) return null
        if ('?' !in url) return null
        val out = runCatching { navigationRewrite(url, true, false, emptyList())?.url }.getOrNull() ?: return null
        return out.takeIf { it != url }
    }
}
