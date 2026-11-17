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
    fun withTheVpnOnAtStartPagesAlsoWaitForItsFilter() {
        val gate = PageGate(vpnRequired = true)
        val ran = ArrayList<String>()
        gate.run { ran += "restore" }
        // The blocker's condition (here: its time limit) alone does not open it.
        gate.open()
        assertFalse(gate.isOpen)
        gate.vpnFilterInstalled(false)
        assertFalse(gate.isOpen)
        gate.vpnFilterInstalled(true)
        assertTrue(gate.isOpen)
        assertEquals(listOf("restore"), ran)
        // Once open it stays open, even when the filter goes (a guard restart).
        gate.vpnFilterInstalled(false)
        assertTrue(gate.isOpen)
    }

    @Test
    fun theVpnFilterAloneDoesNotOpenIt() {
        val gate = PageGate(vpnRequired = true)
        gate.vpnFilterInstalled(true)
        assertFalse(gate.isOpen)
        gate.open()
        assertTrue(gate.isOpen)
    }

    @Test
    fun theVpnSwitchedOffDuringTheStartNoLongerHoldsPages() {
        val gate = PageGate(vpnRequired = true)
        gate.open()
        assertFalse(gate.isOpen)
        gate.vpnRequired(false)
        assertTrue(gate.isOpen)
        // And switched on before the gate opened: pages wait for the filter.
        val late = PageGate()
        late.vpnRequired(true)
        late.open()
        assertFalse(late.isOpen)
    }

    @Test
    fun theRule() {
        assertTrue(PageGate.mayOpen(blockerDone = true, vpnRequired = false, vpnFilterInstalled = false))
        assertTrue(PageGate.mayOpen(blockerDone = true, vpnRequired = true, vpnFilterInstalled = true))
        assertFalse(PageGate.mayOpen(blockerDone = true, vpnRequired = true, vpnFilterInstalled = false))
        assertFalse(PageGate.mayOpen(blockerDone = false, vpnRequired = true, vpnFilterInstalled = true))
        assertFalse(PageGate.mayOpen(blockerDone = false, vpnRequired = false, vpnFilterInstalled = false))
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
