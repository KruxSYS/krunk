package org.krunk.browser.ui.settings

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.browser.engine.blocker.UpdateState

class BlockingSettingsTest {
    private val t1 = 1_800_000_000_000L
    private val t2 = t1 + 3_600_000L

    @Test
    fun theLineUnderUpdateSaysWhereTheListsAreFrom() {
        // Never updated: the lists shipped in the app.
        assertEquals(UpdateLine.FromPackage, updateLine(UpdateState.Idle, stored = null))
        assertEquals(UpdateLine.FromPackage, updateLine(UpdateState.Idle, stored = 0))
        // Updated in an earlier session.
        assertEquals(UpdateLine.UpdatedAt(t1), updateLine(UpdateState.Idle, stored = t1))
        // Running, then done: the new time, also before the stored one was read again.
        assertEquals(UpdateLine.Updating, updateLine(UpdateState.Running(t2), stored = t1))
        assertEquals(UpdateLine.UpdatedAt(t2), updateLine(UpdateState.Done(t2, updated = 9), stored = t1))
        assertEquals(UpdateLine.UpdatedAt(t2), updateLine(UpdateState.Done(t2, updated = 9), stored = null))
        // Nothing arrived: say so, and keep the last good time.
        assertEquals(UpdateLine.Failed(t1), updateLine(UpdateState.Failed(t2), stored = t1))
        assertEquals(UpdateLine.Failed(null), updateLine(UpdateState.Failed(t2), stored = null))
    }

    @Test
    fun updateCanBePressedOnlyWhileBlockingIsOnAndNoUpdateRuns() {
        assertTrue(updateEnabled(UpdateState.Idle, blockingOn = true))
        assertTrue(updateEnabled(UpdateState.Done(t1, 3), blockingOn = true))
        assertTrue(updateEnabled(UpdateState.Failed(t1), blockingOn = true))
        assertFalse(updateEnabled(UpdateState.Running(t1), blockingOn = true))
        assertFalse(updateEnabled(UpdateState.Idle, blockingOn = false))
    }
}
