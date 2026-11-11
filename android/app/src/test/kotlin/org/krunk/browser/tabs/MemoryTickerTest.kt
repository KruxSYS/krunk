package org.krunk.browser.tabs

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.ffi.AndroidSettings
import org.krunk.ffi.KrunkStore
import org.krunk.ffi.MemoryAction
import org.krunk.ffi.MemoryStage
import org.krunk.ffi.memoryPolicy

/** The ticker against a fake host; decisions come from the real core (`libkrunk_ffi.so`). */
class MemoryTickerTest {
    private class FakeHost : MemoryHost {
        val tabs = ArrayList<MemoryTabSnapshot>()
        val slept = ArrayList<UInt>()
        val discarded = ArrayList<UInt>()

        override fun snapshot() = tabs.toList()

        override fun sleep(id: UInt) {
            slept += id
        }

        override fun discard(id: UInt) {
            discarded += id
            val i = tabs.indexOfFirst { it.id == id }
            if (i >= 0) tabs[i] = tabs[i].copy(live = false)
        }
    }

    private fun tab(
        id: Int,
        lastActive: Long,
        active: Boolean = false,
        private: Boolean = false,
        live: Boolean = true,
    ) = MemoryTabSnapshot(id.toUInt(), live, active, private, lastActive)

    @Test
    fun defaultPolicySleepsBackgroundTabsAfterFiveMinutesThenSuspends() {
        val host = FakeHost()
        host.tabs += tab(1, lastActive = 0, active = true)
        host.tabs += tab(2, lastActive = 0)
        val t = MemoryTicker(host)
        assertEquals(memoryPolicy(5u, false, 30u), t.policy)
        assertTrue(t.tickMs in 1000L..15000L)

        assertTrue(t.tick(now = 4 * 60_000).isEmpty())
        assertEquals(listOf(2u to MemoryAction.SET_LOW_MEMORY), t.tick(now = 5 * 60_000))
        assertEquals(MemoryStage.LOW_MEMORY, t.stage(2u))
        assertEquals(listOf(2u), host.slept)
        // The active tab is exempt and the sleeping one waits for the suspend delay.
        assertTrue(t.tick(now = 5 * 60_000 + 30_000).isEmpty())
        assertEquals(listOf(2u to MemoryAction.SUSPEND), t.tick(now = 6 * 60_000))
        assertEquals(MemoryStage.SUSPENDED, t.stage(2u))
        // Discard is off by default: nothing more happens.
        assertTrue(t.tick(now = 60 * 60_000).isEmpty())
        assertTrue(host.discarded.isEmpty())
    }

    @Test
    fun wakeRestartsTheSequenceAndDiscardFreesTheSession() {
        val host = FakeHost()
        host.tabs += tab(1, lastActive = 0, active = true)
        host.tabs += tab(2, lastActive = 0)
        val t = MemoryTicker(host)
        t.policy = MemoryTicker.policyFor(settings(sleepMinutes = 1u, discard = true, discardMinutes = 10u))
        // A quarter of the shortest timer (one minute), clamped to 1-15 s.
        assertEquals(15_000L, t.tickMs)

        assertEquals(listOf(2u to MemoryAction.SET_LOW_MEMORY), t.tick(60_000))
        t.wake(2u)
        assertEquals(MemoryStage.AWAKE, t.stage(2u))
        // Woken (activated) tabs come back with a fresh last_active from the model.
        host.tabs[1] = tab(2, lastActive = 60_000)
        assertTrue(t.tick(90_000).isEmpty())
        assertEquals(listOf(2u to MemoryAction.SET_LOW_MEMORY), t.tick(120_000))
        assertEquals(listOf(2u to MemoryAction.SUSPEND), t.tick(180_000))
        assertEquals(listOf(2u to MemoryAction.DISCARD), t.tick(60_000 + 10 * 60_000))
        assertEquals(listOf(2u), host.discarded)
        assertEquals(MemoryStage.AWAKE, t.stage(2u))
        // Not live any more: nothing further.
        assertTrue(t.tick(100 * 60_000).isEmpty())
    }

    @Test
    fun theLastLivePrivateTabIsSuspendedInsteadOfDiscarded() {
        val host = FakeHost()
        host.tabs += tab(1, lastActive = 0, active = true)
        host.tabs += tab(2, lastActive = 0, private = true)
        host.tabs += tab(3, lastActive = 0, private = true)
        val t = MemoryTicker(host)
        t.policy = MemoryTicker.policyFor(settings(1u, true, 2u), testSeconds = 10)
        // Test timers: sleep 10 s, suspend 20 s, discard 40 s.
        assertEquals(10_000L, t.policy.sleepAfter)
        assertEquals(40_000L, t.policy.discardAfter)
        val actions = t.tick(40_000)
        // Two live private tabs: the first is discarded, the remaining one is only suspended.
        assertEquals(listOf(2u to MemoryAction.DISCARD, 3u to MemoryAction.SUSPEND), actions)
        assertEquals(listOf(2u), host.discarded)
        assertEquals(listOf(3u), host.slept)
        assertTrue(t.tick(80_000).isEmpty())
    }

    @Test
    fun policyForFollowsTheSettingsAndTheDebugOverride() {
        val off = MemoryTicker.policyFor(settings(0u, false, 30u))
        assertNull(off.sleepAfter)
        assertNull(off.discardAfter)
        val p = MemoryTicker.policyFor(settings(7u, true, 20u))
        assertEquals(7 * 60_000L, p.sleepAfter)
        assertEquals(20 * 60_000L, p.discardAfter)
        val test = MemoryTicker.policyFor(settings(5u, false, 30u), testSeconds = 3, forceDiscard = true)
        assertEquals(3_000L, test.sleepAfter)
        assertEquals(3_000L, test.suspendDelay)
        assertEquals(12_000L, test.discardAfter)
        // Sleep off stays off under the override; a zero override is ignored.
        assertNull(MemoryTicker.policyFor(settings(0u, false, 30u), testSeconds = 3).sleepAfter)
        assertEquals(p, MemoryTicker.policyFor(settings(7u, true, 20u), testSeconds = 0))
    }

    companion object {
        /** Real settings from a scratch store, with the three memory fields changed. */
        fun settings(sleepMinutes: UInt, discard: Boolean, discardMinutes: UInt): AndroidSettings {
            val db = File.createTempFile("krunk-test", ".db").also { it.delete() }
            val store = KrunkStore.open(db.path)
            try {
                return store.loadSettings().copy(
                    tabSleepMinutes = sleepMinutes,
                    tabDiscardEnabled = discard,
                    tabDiscardMinutes = discardMinutes,
                )
            } finally {
                store.destroy()
                db.delete()
            }
        }
    }
}
