package org.krunk.browser.engine

/**
 * Tells when a tab's top-level **document** really changed, which is when what was granted or
 * refused for "this page" ends and its open permission prompts are dismissed.
 *
 * `onPageStart` alone is not that signal: it also fires for loads that never replace the
 * document (a download, an HTTP 204 answer, a link handed to another app), and a page could
 * start such loads to end its own "denied for this page" state or to dismiss a prompt the user
 * is reading. GeckoView 160 has no "same document" flag on `onLocationChange` either
 * (`GeckoViewNavigation.sys.mjs` sends every top-level location change, `pushState` and
 * fragment changes included). The signal used is therefore:
 *
 * - a location change while a top-level load is under way (between `onPageStart` and
 *   `onPageStop`): the load committed its document (a download or a 204 never changes the
 *   location, and a same-document change does not start a load); or
 * - a location change to another origin, which a same-document change cannot be.
 *
 * Main thread only (where the engine calls the delegates).
 */
class DocumentChange(private val originOf: (String) -> String?) {
    private var loading = false
    private var origin: String? = null
    private var any = false

    fun pageStart() {
        loading = true
    }

    fun pageStop() {
        loading = false
    }

    /** The committed address changed to [url]: true when that is another document. */
    fun locationChanged(url: String): Boolean {
        val o = originOf(url)
        val changed = loading || !any || o != origin
        any = true
        origin = o
        return changed
    }
}
