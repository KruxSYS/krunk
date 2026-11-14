package org.krunk.browser.ui.vault

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VaultClipboardTest {
    @Test
    fun ourClipIsClearedWhenItsTimeIsUpAndItIsStillThere() {
        assertEquals(
            ClipAction.CLEAR,
            clipExpiryAction(pending = true, due = true, foreground = true, holdsOurClip = true),
        )
    }

    @Test
    fun somethingElseOnTheClipboardIsLeftAlone() {
        assertEquals(
            ClipAction.FORGET,
            clipExpiryAction(pending = true, due = true, foreground = true, holdsOurClip = false),
        )
    }

    @Test
    fun inTheBackgroundThePasswordIsClearedWithoutLooking() {
        // The clipboard cannot be read there, and a password must not stay on it: cleared when
        // the time is up, whatever a read would have said.
        for (holds in listOf(true, false, null)) {
            assertEquals(
                ClipAction.CLEAR,
                clipExpiryAction(pending = true, due = true, foreground = false, holdsOurClip = holds),
            )
        }
    }

    @Test
    fun anUnreadableClipboardIsAskedAgainAndNeverClearedBlind() {
        assertEquals(
            ClipAction.RETRY,
            clipExpiryAction(pending = true, due = true, foreground = true, holdsOurClip = null),
        )
    }

    @Test
    fun nothingHappensBeforeTheTimeIsUpOrWithoutACopy() {
        for (foreground in listOf(true, false)) {
            for (holds in listOf(true, false, null)) {
                assertEquals(ClipAction.NOTHING, clipExpiryAction(pending = true, due = false, foreground, holds))
                assertEquals(ClipAction.NOTHING, clipExpiryAction(pending = false, due = true, foreground, holds))
                assertEquals(ClipAction.NOTHING, clipExpiryAction(pending = false, due = false, foreground, holds))
            }
        }
    }

    @Test
    fun krunkConfirmsACopyOnlyWhereTheSystemDoesNot() {
        assertTrue(showsOwnCopyConfirmation(26))
        assertTrue(showsOwnCopyConfirmation(32))
        assertFalse(showsOwnCopyConfirmation(33))
        assertFalse(showsOwnCopyConfirmation(36))
    }

    @Test
    fun theWindowIsSecureWhileAnyVaultSurfaceShows() {
        val counter = SecureSurfaceCounter()
        assertFalse(counter.secure.value)
        counter.acquire() // the screen
        assertTrue(counter.secure.value)
        counter.acquire() // a dialog over it
        counter.release() // the screen goes first
        assertTrue(counter.secure.value)
        assertEquals(1, counter.surfaces)
        counter.release()
        assertFalse(counter.secure.value)
        // One release too many does not make the next surface invisible to the count.
        counter.release()
        assertEquals(0, counter.surfaces)
        counter.acquire()
        assertTrue(counter.secure.value)
    }
}
