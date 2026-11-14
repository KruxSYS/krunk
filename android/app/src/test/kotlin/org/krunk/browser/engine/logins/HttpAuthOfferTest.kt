package org.krunk.browser.engine.logins

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.krunk.browser.engine.logins.HttpAuthOffer.Challenge
import org.krunk.browser.engine.logins.HttpAuthOffer.Saved
import org.krunk.browser.engine.logins.LoginVault.State

class HttpAuthOfferTest {
    private fun challenge(
        uri: String? = "https://intranet.example/private",
        proxy: Boolean = false,
        cross: Boolean = false,
        failed: Boolean = false,
    ) = Challenge(uri, proxy, cross, failed)

    @Test
    fun anOpenVaultListsALockedOneOffersUnlockOnlyWithAHint() {
        val o = HttpAuthOffer()
        assertEquals(Saved.LIST, o.saved(challenge(), private = false, State.UNLOCKED, hint = false))
        assertEquals(Saved.UNLOCK, o.saved(challenge(), private = false, State.LOCKED, hint = true))
        assertEquals(Saved.NONE, o.saved(challenge(), private = false, State.LOCKED, hint = false))
        assertEquals(Saved.NONE, o.saved(challenge(), private = false, State.NONE, hint = true))
        assertEquals(Saved.NONE, o.saved(challenge(), private = false, State.KEY_LOST, hint = true))
    }

    @Test
    fun aPrivateTabOnlyGetsTheExplicitButton() {
        val o = HttpAuthOffer()
        assertEquals(Saved.EXPLICIT, o.saved(challenge(), private = true, State.UNLOCKED, hint = true))
        assertEquals(Saved.EXPLICIT, o.saved(challenge(), private = true, State.LOCKED, hint = false))
        assertEquals(Saved.NONE, o.saved(challenge(), private = true, State.NONE, hint = false))
    }

    @Test
    fun aProxyAResourceOfAnotherSiteAndAnUnknownServerGetNothing() {
        val o = HttpAuthOffer()
        assertEquals(Saved.NONE, o.saved(challenge(proxy = true), false, State.UNLOCKED, true))
        assertEquals(Saved.NONE, o.saved(challenge(cross = true), false, State.UNLOCKED, true))
        assertEquals(Saved.NONE, o.saved(challenge(uri = null), false, State.UNLOCKED, true))
        assertEquals(Saved.NONE, o.saved(challenge(uri = "ftp://files.example/"), false, State.UNLOCKED, true))
    }

    @Test
    fun aSignInThatWorkedIsOfferedOnce() {
        val o = HttpAuthOffer()
        val c = challenge()
        o.onChallenge(c)
        o.onAnswered(c, private = false, "alice", "pw")
        val sent = o.onPageStop(success = true)
        assertEquals("https://intranet.example", sent?.origin)
        assertEquals("alice", sent?.username)
        assertEquals("pw", sent?.password)
        assertNull(o.onPageStop(success = true))
    }

    @Test
    fun aRepeatedChallengeMeansTheAnswerWasWrong() {
        val o = HttpAuthOffer()
        o.onAnswered(challenge(), private = false, "alice", "wrong")
        o.onChallenge(challenge(failed = true))
        assertNull(o.onPageStop(success = true))
        // Also without the flag: the same server asking again.
        o.onAnswered(challenge(), private = false, "alice", "wrong")
        o.onChallenge(challenge())
        assertNull(o.onPageStop(success = true))
    }

    @Test
    fun aFailedLoadOrACancelledDialogOffersNothing() {
        val o = HttpAuthOffer()
        o.onAnswered(challenge(), private = false, "alice", "pw")
        assertNull(o.onPageStop(success = false))
        o.onAnswered(challenge(), private = false, "alice", "pw")
        o.clear()
        assertNull(o.onPageStop(success = true))
    }

    @Test
    fun aSignInIsNotOfferedOnALoadThatWentToAnotherServer() {
        val o = HttpAuthOffer()
        o.onAnswered(challenge(), private = false, "alice", "pw")
        o.onPageStart("https://intranet.example/next")
        assertEquals("alice", o.onPageStop(success = true)?.username)
        o.onAnswered(challenge(), private = false, "alice", "pw")
        o.onPageStart("https://elsewhere.example/")
        assertNull(o.onPageStop(success = true))
    }

    @Test
    fun aLockTakesTheListOutOfTheOpenDialog() {
        assertEquals(Saved.UNLOCK, HttpAuthOffer.afterLock(Saved.LIST, private = false))
        assertEquals(Saved.UNLOCK, HttpAuthOffer.afterLock(Saved.UNLOCK, private = false))
        assertEquals(Saved.EXPLICIT, HttpAuthOffer.afterLock(Saved.LIST, private = true))
        assertEquals(Saved.EXPLICIT, HttpAuthOffer.afterLock(Saved.EXPLICIT, private = true))
        assertEquals(Saved.NONE, HttpAuthOffer.afterLock(Saved.NONE, private = false))
    }

    @Test
    fun nothingIsKeptForAPrivateTabAProxyOrAnEmptyPassword() {
        val o = HttpAuthOffer()
        o.onAnswered(challenge(), private = true, "alice", "pw")
        assertNull(o.onPageStop(success = true))
        o.onAnswered(challenge(proxy = true), private = false, "alice", "pw")
        assertNull(o.onPageStop(success = true))
        o.onAnswered(challenge(cross = true), private = false, "alice", "pw")
        assertNull(o.onPageStop(success = true))
        o.onAnswered(challenge(), private = false, "alice", "")
        assertNull(o.onPageStop(success = true))
    }
}
