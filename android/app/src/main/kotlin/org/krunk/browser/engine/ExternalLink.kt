package org.krunk.browser.engine

/**
 * Links a page can ask for that are not pages: `mailto:` (the mail app) and `tel:` (the dialer).
 * Krunk hands them to Android instead of loading them. Everything else stays with the engine as
 * before; `intent:`, `market:` and app-specific schemes are deliberately not handed over.
 */
enum class ExternalKind { MAIL, PHONE }

/** A link to hand over: the cleaned [uri] for the intent and [shown], what the question names. */
data class ExternalLink(val kind: ExternalKind, val uri: String, val shown: String)

/** What to do with an address a page wants to load. */
sealed interface HandOff {
    /** Not a hand-off link: the engine deals with it. */
    data object NotExternal : HandOff

    /** A hand-off scheme, but malformed or oversized: dropped (the load is refused). */
    data object Refuse : HandOff

    /** The user tapped the link: hand it over. */
    data class Open(val link: ExternalLink) : HandOff

    /** The page asked by itself (a script, a redirect): ask the user first. */
    data class Ask(val link: ExternalLink) : HandOff
}

object ExternalLinks {
    const val MAX_URI = 2048
    private const val MAX_SHOWN = 120
    private val PHONE = Regex("""^[+]?[0-9()\-.*# ]{1,40}$""")

    /**
     * How long after a touch on the page a link counts as tapped. The engine's own "user
     * gesture" flag stays set long after a tap (a script got the dialer opened 8 s after one
     * on the device), so Krunk also asks how long ago the page was really touched.
     */
    const val TAP_WINDOW_MS = 2_000L

    /**
     * [uri] is what the page wants to load; [userGesture] whether the engine saw a user gesture
     * for it; [redirect] whether a server redirect led here (never treated as a tap);
     * [msSinceTouch] how long ago the user last touched the page (null: never).
     */
    fun classify(uri: String, userGesture: Boolean, redirect: Boolean = false, msSinceTouch: Long? = 0): HandOff {
        val scheme = uri.substringBefore(':', "").lowercase()
        if (scheme != "mailto" && scheme != "tel") return HandOff.NotExternal
        if (uri.length > MAX_URI || uri.any { it.isISOControl() }) return HandOff.Refuse
        val rest = uri.substring(scheme.length + 1)
        val link = when (scheme) {
            "mailto" -> mail(rest)
            else -> phone(rest)
        } ?: return HandOff.Refuse
        val tapped = userGesture && !redirect && msSinceTouch != null && msSinceTouch in 0..TAP_WINDOW_MS
        return if (tapped) HandOff.Open(link) else HandOff.Ask(link)
    }

    /** `mailto:` with an address, or with only fields (`mailto:?subject=...`). */
    private fun mail(rest: String): ExternalLink? {
        if (rest.isEmpty()) return null
        val address = decode(rest.substringBefore('?')).trim()
        if (address.isNotEmpty() && '@' !in address) return null
        val shown = address.ifEmpty { "mailto:" }.take(MAX_SHOWN)
        return ExternalLink(ExternalKind.MAIL, "mailto:$rest", shown)
    }

    /** `tel:` with digits and the usual separators; anything else (letters, parameters) is refused. */
    private fun phone(rest: String): ExternalLink? {
        val number = decode(rest).trim()
        if (!PHONE.matches(number) || number.none { it.isDigit() }) return null
        // The dialer gets the number without spaces; `#` must stay encoded in a tel: address.
        val compact = number.filterNot { it == ' ' }.replace("#", "%23")
        return ExternalLink(ExternalKind.PHONE, "tel:$compact", number)
    }

    private fun decode(s: String): String = runCatching { java.net.URLDecoder.decode(s.replace("+", "%2B"), "UTF-8") }
        .getOrDefault(s)
}
