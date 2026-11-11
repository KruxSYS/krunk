package org.krunk.browser.ui.browser

import org.krunk.browser.engine.originOf

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
 * The indicator for the address the bar shows: [internal] pages and error pages show nothing;
 * an address that is still [pending] (requested, not loaded) shows nothing, because the page
 * under it is another one; an `https://` page the engine flagged secure shows the lock; an
 * `http://` page shows "Not secure" once it has loaded (not while it may still be upgraded),
 * whatever the engine says; and an `https://` page the engine does not call secure (broken
 * certificate chain, mixed active content) shows "Not secure" as well.
 */
fun securityIndicator(
    url: String,
    internal: Boolean,
    secure: Boolean?,
    loading: Boolean,
    loadError: Boolean,
    pending: Boolean = false,
): SecurityIndicator {
    if (internal || url.isEmpty() || loadError || pending) return SecurityIndicator.NONE
    val lower = url.lowercase()
    val http = lower.startsWith("http://")
    val https = lower.startsWith("https://")
    if (!http && !https) return SecurityIndicator.NONE
    return when {
        https && secure == true -> SecurityIndicator.SECURE
        loading && secure == null -> SecurityIndicator.NONE
        http -> SecurityIndicator.INSECURE
        secure == false -> SecurityIndicator.INSECURE
        else -> SecurityIndicator.NONE
    }
}

private const val MAX_SHOWN = 2000

/** Characters that reorder or hide text (bidi controls, zero-width, other controls). */
private fun isDeceptive(c: Char): Boolean =
    c < ' ' || c == '\u007f' || c in '\u0080'..'\u009f' || c in '\u200b'..'\u200f' ||
        c in '\u202a'..'\u202e' || c in '\u2066'..'\u2069' || c == '\ufeff'

/**
 * An address as a suggestion row shows it: `https://` dropped (the lock says it), `http://`
 * kept so an insecure page is obvious, a lone trailing slash dropped, bounded in length and
 * without characters that reorder text.
 */
fun addressText(displayUrl: String): String {
    var s = displayUrl.take(MAX_SHOWN).filterNot(::isDeceptive)
    if (s.startsWith("https://", ignoreCase = true)) s = s.substring("https://".length)
    if (s.count { it == '/' } == 1 && s.endsWith('/')) s = s.dropLast(1)
    return s
}

/**
 * The address as the (non-editing) pill shows it. For a web page that is the **host only**
 * (with a port when it is not the default), lower case, never the `user:password@` part: the
 * pill is narrow, and what must always be readable is which site this is. The text is laid out
 * left to right and cut at the start, so the end of the host (the registrable domain) stays
 * visible. Other addresses (`krunk://settings`, `file:`, `data:`) are shown as they are,
 * bounded. Editing always starts from the full address.
 */
fun pillText(displayUrl: String): String {
    val origin = originOf(displayUrl)
    val scheme = origin?.substringBefore("://")
    if (origin == null || (scheme != "http" && scheme != "https")) return addressText(displayUrl)
    return origin.substringAfter("://").filterNot(::isDeceptive)
}
