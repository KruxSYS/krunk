package org.krunk.browser.engine

/**
 * Website sign-in (0.11.0): Krunk, signed in to the Krunk account, signs its own website in. The
 * decision is the shared core's (`AutoSignIn`: only the site's sign-in page, never a private tab,
 * not after a sign-out on the site until the app restarts, at most once per 30 s).
 */
interface WebSignIn {
    /**
     * For a top-level navigation to [url]: `null` = load it as it is; otherwise cancel it and
     * open [url] for the returned `next`. Main thread, no network.
     */
    fun check(url: String, private: Boolean): Target?

    /**
     * The sign-in page with a fresh single-use code, or `null` on any failure. Suspends for the
     * network call. The address is a secret: never log it.
     */
    suspend fun url(next: String?): String?

    /** [next]: the sign-in page's own `next` parameter (where the site goes once signed in). */
    data class Target(val next: String?)
}
