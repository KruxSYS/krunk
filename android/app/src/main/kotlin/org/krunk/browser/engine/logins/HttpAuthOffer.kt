package org.krunk.browser.engine.logins

import org.krunk.browser.engine.originOf

/**
 * Saved logins in the HTTP sign-in dialog, and the offer to save after a sign-in that worked.
 * One per tab. The server that asks is matched by its exact origin, as on Windows; a proxy and a
 * resource of another site get no offer at all.
 */
class HttpAuthOffer {
    /** What the sign-in dialog shows about saved logins. */
    enum class Saved {
        /** Nothing: no vault, no logins, a proxy, a resource of another site. */
        NONE,

        /** The vault is open: list the logins of this server. */
        LIST,

        /** Locked and the hint list knows this server: an "Unlock vault" button. */
        UNLOCK,

        /** A private tab: only the explicit "Fill from vault" button. */
        EXPLICIT,
    }

    companion object {
        /** What the dialog shows of the vault once the vault has locked while it was open. */
        fun afterLock(shown: Saved, private: Boolean): Saved = when (shown) {
            Saved.NONE -> Saved.NONE
            Saved.EXPLICIT -> Saved.EXPLICIT
            Saved.LIST, Saved.UNLOCK -> if (private) Saved.EXPLICIT else Saved.UNLOCK
        }
    }

    data class Challenge(
        /** The address of the server that asks. */
        val uri: String?,
        val proxy: Boolean,
        val crossOriginSubResource: Boolean,
        val previousFailed: Boolean,
    )

    /** What the user sent and where, kept until the load shows whether it worked. */
    class Sent(val origin: String, val username: String, val password: String)

    private var sent: Sent? = null

    fun saved(c: Challenge, private: Boolean, vault: LoginVault.State, hint: Boolean): Saved {
        val origin = c.uri?.let { originOf(it) }
        if (c.proxy || c.crossOriginSubResource || !LoginPolicy.isWeb(origin)) return Saved.NONE
        return when (vault) {
            LoginVault.State.NONE, LoginVault.State.KEY_LOST -> Saved.NONE

            LoginVault.State.LOCKED -> if (private) {
                Saved.EXPLICIT
            } else if (hint) {
                Saved.UNLOCK
            } else {
                Saved.NONE
            }

            LoginVault.State.UNLOCKED -> if (private) Saved.EXPLICIT else Saved.LIST
        }
    }

    /** A challenge arrived. One that says the last answer failed drops what was sent. */
    fun onChallenge(c: Challenge) {
        val s = sent ?: return
        if (c.previousFailed || c.uri?.let { originOf(it) } == s.origin) sent = null
    }

    /** The user answered the dialog. */
    fun onAnswered(c: Challenge, private: Boolean, username: String, password: String) {
        val origin = c.uri?.let { originOf(it) }
        sent = if (private || c.proxy || c.crossOriginSubResource || origin == null ||
            !LoginPolicy.isWeb(origin) || password.isEmpty()
        ) {
            null
        } else {
            Sent(origin, username, password)
        }
    }

    /** A new top-level load began: a sign-in sent to another server is not offered on it. */
    fun onPageStart(url: String) {
        val s = sent ?: return
        if (originOf(url) != s.origin) sent = null
    }

    /** The dialog was closed without an answer, or the tab went elsewhere. */
    fun clear() {
        sent = null
    }

    /**
     * The top-level load ended. With [success] and no new challenge since the answer, the sign-in
     * worked: the login to offer for saving (once).
     */
    fun onPageStop(success: Boolean): Sent? {
        val s = sent
        sent = null
        return if (success) s else null
    }
}
