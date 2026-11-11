package org.krunk.browser.engine

/** Who asked for a page load. It decides what may be loaded and with which privileges. */
enum class LoadSource {
    /** The user, through Krunk's own UI: address bar, bookmarks, history, a restored tab. */
    USER,

    /** Another app (a VIEW or SEND intent). Loaded like a link on a web page, never privileged. */
    EXTERNAL_APP,

    /** An address a web page supplied (the link long-press menu). Loaded on behalf of that page. */
    PAGE,
}

/**
 * What Krunk loads for addresses it did not get from the user. The engine loads an address
 * given by the app with the app's own privileges unless told otherwise, so anything that comes
 * from another app or from a page is limited to web addresses here and then loaded
 * unprivileged (see `KrunkSession.load`). Pure logic, tested on the JVM.
 */
object LoadPolicy {
    private val webUrl = Regex("^https?://[^\\s/?#@\\\\]+([/?#]\\S*)?$", RegexOption.IGNORE_CASE)
    private val urlInText = Regex("https?://[^\\s<>\"']+", RegexOption.IGNORE_CASE)
    private const val MAX_URL = 8192
    private const val MAX_SEARCH = 512

    /**
     * `http://` or `https://` with a host and no user:password part, no whitespace or control
     * characters. Everything else (`file:`, `content:`, `javascript:`, `data:`, `blob:`,
     * `about:`, `resource:`, `intent:`, `krunk:`, ...) is refused.
     */
    fun isWebUrl(url: String): Boolean =
        url.length <= MAX_URL && url.none { it < ' ' || it == '\u007f' } && webUrl.matches(url)

    /** The address of a VIEW intent if Krunk opens it, else null. */
    fun fromViewIntent(data: String?): String? = data?.trim()?.takeIf(::isWebUrl)

    /**
     * Text another app shared: the web address when the text is one, or the first one inside it
     * ("Page title https://example.com/page"), else null (the caller searches for [searchText]).
     */
    fun urlInSharedText(text: String): String? {
        val t = text.trim()
        if (isWebUrl(t)) return t
        val found = urlInText.find(t)?.value ?: return null
        // Sentence punctuation after the address is not part of it (a ")" only without a "(").
        val url = found.trimEnd('.', ',', ';', ':', '!', '?').let { if ('(' in it) it else it.trimEnd(')') }
        return url.takeIf(::isWebUrl)
    }

    /** Shared text as a search query: one line, bounded. */
    fun searchText(text: String): String = text.replace(Regex("\\s+"), " ").trim().take(MAX_SEARCH)

    /** Whether an address may be opened for [source]. The user's own input is not limited here. */
    fun allowed(url: String, source: LoadSource): Boolean = source == LoadSource.USER || isWebUrl(url)
}
