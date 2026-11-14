package org.krunk.browser.ui.browser

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class FullScreenPolicyTest {
    private val a = 1u
    private val b = 2u

    @Test
    fun onlyTheActiveTabUnderAnUncoveredBrowserIsShownFullScreen() {
        assertTrue(FullScreenPolicy.shown(a, setOf(a), browserOnTop = true))
        assertFalse(FullScreenPolicy.shown(a, emptySet(), browserOnTop = true))
        assertFalse(FullScreenPolicy.shown(b, setOf(a), browserOnTop = true))
        assertFalse(FullScreenPolicy.shown(a, setOf(a), browserOnTop = false))
        assertFalse(FullScreenPolicy.shown(null, setOf(a), browserOnTop = true))
    }

    @Test
    fun aTabThatIsFullScreenWithoutBeingShownIsToldToLeave() {
        // Shown: nobody leaves.
        assertEquals(emptySet<UInt>(), FullScreenPolicy.tabsToLeave(a, setOf(a), browserOnTop = true))
        // Another tab became active (a link opened a tab, the tab was switched).
        assertEquals(setOf(a), FullScreenPolicy.tabsToLeave(b, setOf(a), browserOnTop = true))
        // Another screen covers the browser: the tab switcher, settings...
        assertEquals(setOf(a), FullScreenPolicy.tabsToLeave(a, setOf(a), browserOnTop = false))
        // A background tab that is still in it leaves, the one shown stays.
        assertEquals(setOf(b), FullScreenPolicy.tabsToLeave(a, setOf(a, b), browserOnTop = true))
        // The last tab was closed.
        assertEquals(setOf(a), FullScreenPolicy.tabsToLeave(null, setOf(a), browserOnTop = true))
        assertEquals(emptySet<UInt>(), FullScreenPolicy.tabsToLeave(a, emptySet(), browserOnTop = false))
    }

    // ActivityInfo values, as the engine passes them.
    private val unspecified = -1
    private val landscape = 6
    private val portrait = 7

    @Test
    fun aLockWhileFullScreenIsAllowedAndUndoneWhenFullScreenEnds() {
        val lock = OrientationLock()
        assertEquals(OrientationLock.Outcome(), lock.fullScreenChanged(true, unspecified))
        assertEquals(OrientationLock.Answer.Allow(landscape), lock.lock(landscape, current = unspecified))
        assertTrue(lock.locked)
        // A second lock keeps the orientation from before the first one.
        assertEquals(OrientationLock.Answer.Allow(portrait), lock.lock(portrait, current = landscape))
        // Full screen ends (Back, the page, a tab switch, another screen, a crash: all the same).
        assertEquals(OrientationLock.Outcome(apply = unspecified), lock.fullScreenChanged(false, portrait))
        assertFalse(lock.locked)
        // Nothing more to undo.
        assertNull(lock.unlock())
        assertEquals(OrientationLock.Outcome(), lock.fullScreenChanged(false, unspecified))
    }

    @Test
    fun thePageUnlockingRestoresOnce() {
        val lock = OrientationLock()
        lock.fullScreenChanged(true, portrait)
        lock.lock(landscape, current = portrait)
        assertEquals(portrait, lock.unlock())
        assertNull(lock.unlock())
        // Full screen ending afterwards changes nothing.
        assertEquals(OrientationLock.Outcome(), lock.fullScreenChanged(false, portrait))
        // An unlock with nothing locked (the engine sends one when full screen ends) is harmless.
        assertNull(OrientationLock().unlock())
    }

    @Test
    fun aLockOutsideFullScreenWaitsAndIsDeniedWhenFullScreenDoesNotStart() {
        val lock = OrientationLock()
        assertEquals(OrientationLock.Answer.Wait, lock.lock(landscape, current = unspecified))
        assertTrue(lock.isWaiting)
        assertFalse(lock.locked)
        assertTrue(lock.waitExpired())
        assertFalse(lock.isWaiting)
        assertFalse(lock.waitExpired())
        // Full screen starting later does not revive it.
        assertEquals(OrientationLock.Outcome(), lock.fullScreenChanged(true, unspecified))
        assertFalse(lock.locked)
    }

    @Test
    fun aLockThatArrivesJustBeforeFullScreenIsAllowedWhenItStarts() {
        val lock = OrientationLock()
        assertEquals(OrientationLock.Answer.Wait, lock.lock(landscape, current = unspecified))
        assertEquals(
            OrientationLock.Outcome(apply = landscape, settle = true),
            lock.fullScreenChanged(true, current = unspecified),
        )
        assertTrue(lock.locked)
        assertFalse(lock.waitExpired())
        assertEquals(OrientationLock.Outcome(apply = unspecified), lock.fullScreenChanged(false, landscape))
    }

    @Test
    fun aWaitingLockIsDroppedByAnUnlockAndReplacedByANewerOne() {
        val lock = OrientationLock()
        lock.lock(landscape, current = unspecified)
        assertNull(lock.unlock())
        assertFalse(lock.isWaiting)
        lock.lock(landscape, current = unspecified)
        lock.lock(portrait, current = unspecified)
        assertEquals(
            OrientationLock.Outcome(apply = portrait, settle = true),
            lock.fullScreenChanged(true, unspecified),
        )
    }

    @Test
    fun fullScreenEndingWhileALockIsHeldAndTheSameStateTwiceDoNothingExtra() {
        val lock = OrientationLock()
        lock.fullScreenChanged(true, unspecified)
        assertEquals(OrientationLock.Outcome(), lock.fullScreenChanged(true, unspecified))
        lock.lock(landscape, current = unspecified)
        assertEquals(OrientationLock.Outcome(), lock.fullScreenChanged(true, landscape))
        assertEquals(OrientationLock.Outcome(apply = unspecified), lock.fullScreenChanged(false, landscape))
        assertEquals(OrientationLock.Outcome(), lock.fullScreenChanged(false, unspecified))
    }
}
