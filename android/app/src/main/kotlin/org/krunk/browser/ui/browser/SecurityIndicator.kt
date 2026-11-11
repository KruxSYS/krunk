package org.krunk.browser.ui.browser

/** What the address bar shows in front of the URL. */
enum class SecurityIndicator {
    /** Internal pages, blank tabs, error pages, pages still loading for the first time. */
    NONE,

    /** The engine reports a secure connection (lock). */
    SECURE,

    /** Plain `http://` once loaded, or `https://` the engine does not consider secure. */
    INSECURE,
}

/**
 * The indicator for a page: [internal] pages and error pages show nothing, a page the engine
 * flagged secure shows the lock, an `http://` page shows "Not secure" once it has loaded (not
 * while it may still be upgraded), and an `https://` page the engine does not call secure
 * (broken certificate chain, mixed active content) shows "Not secure" as well.
 */
fun securityIndicator(
    url: String,
    internal: Boolean,
    secure: Boolean?,
    loading: Boolean,
    loadError: Boolean,
): SecurityIndicator {
    if (internal || url.isEmpty() || loadError) return SecurityIndicator.NONE
    val lower = url.lowercase()
    val http = lower.startsWith("http://")
    val https = lower.startsWith("https://")
    if (!http && !https) return SecurityIndicator.NONE
    return when {
        secure == true -> SecurityIndicator.SECURE
        loading && secure == null -> SecurityIndicator.NONE
        http -> SecurityIndicator.INSECURE
        secure == false -> SecurityIndicator.INSECURE
        else -> SecurityIndicator.NONE
    }
}

/**
 * The address as the (non-editing) pill shows it: `https://` dropped (the lock says it),
 * `http://` kept so an insecure page is obvious, a lone trailing slash dropped. Editing always
 * starts from the full URL.
 */
fun addressText(displayUrl: String): String {
    var s = displayUrl
    if (s.startsWith("https://", ignoreCase = true)) s = s.substring("https://".length)
    if (s.count { it == '/' } == 1 && s.endsWith('/')) s = s.dropLast(1)
    return s
}
