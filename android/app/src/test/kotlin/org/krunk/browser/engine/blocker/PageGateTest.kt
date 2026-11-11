package org.krunk.browser.engine.blocker

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PageGateTest {
    @Test
    fun loadsWaitInOrderUntilTheGateOpensAndRunAtOnceAfterwards() {
        val gate = PageGate()
        val ran = ArrayList<String>()
        gate.run { ran += "restore tab 1" }
        gate.run { ran += "load tab 2" }
        assertFalse(gate.isOpen)
        assertEquals(2, gate.held)
        assertTrue(ran.isEmpty())

        gate.open()
        assertEquals(listOf("restore tab 1", "load tab 2"), ran)
        assertEquals(0, gate.held)

        gate.run { ran += "load tab 3" }
        assertEquals("load tab 3", ran.last())
        // Opening twice runs nothing twice.
        gate.open()
        assertEquals(3, ran.size)
    }

    @Test
    fun aLoadQueuedByAnotherHeldLoadStillRuns() {
        val gate = PageGate()
        val ran = ArrayList<Int>()
        gate.run {
            ran += 1
            gate.run { ran += 2 }
        }
        gate.open()
        assertEquals(listOf(1, 2), ran)
    }
}
