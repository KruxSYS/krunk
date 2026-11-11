package org.krunk.browser.tabs

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CaptureGateTest {
    @Test
    fun completesOnceWhenTheCaptureArrivesFirst() {
        var timeoutAction: (() -> Unit)? = null
        var done = 0
        val gate = CaptureGate(700, { _, action -> timeoutAction = action }) { done++ }
        gate.finish()
        gate.finish()
        timeoutAction?.invoke()
        assertEquals(1, done)
        assertFalse(gate.timedOut)
    }

    @Test
    fun theTimeoutOpensTheSwitcherWithoutAPicture() {
        var timeoutAction: (() -> Unit)? = null
        var delay = 0L
        var done = 0
        val gate = CaptureGate(700, { ms, action ->
            delay = ms
            timeoutAction = action
        }) { done++ }
        assertEquals(700L, delay)
        timeoutAction?.invoke()
        assertTrue(gate.timedOut)
        assertEquals(1, done)
        // A late capture result must not run the continuation again.
        gate.finish()
        assertEquals(1, done)
    }
}
