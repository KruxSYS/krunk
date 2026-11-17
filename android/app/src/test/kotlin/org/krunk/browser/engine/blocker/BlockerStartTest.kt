package org.krunk.browser.engine.blocker

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class BlockerStartTest {
    /** Feeds events in order; returns the final state and every action issued, in order. */
    private fun run(vararg events: StartEvent, from: StartState = StartState()): Pair<StartState, List<StartAction>> {
        var state = from
        val actions = ArrayList<StartAction>()
        for (event in events) {
            val (next, out) = step(state, event)
            state = next
            actions += out
        }
        return state to actions
    }

    private val firstStart = arrayOf(
        StartEvent.Start(blockingWanted = true),
        StartEvent.GuardInstalled(privateAllowed = false),
        // The guard's first script says "ready" before private access restarts it.
        StartEvent.GuardReady,
        StartEvent.GuardLost,
        StartEvent.GuardPrivateAllowed,
        StartEvent.GuardReady,
        StartEvent.BlockerInstalled(privateAllowed = false, enabled = true),
        StartEvent.BlockerPrivateAllowed,
        StartEvent.BlockerEnabled,
        StartEvent.FiltersReady,
        StartEvent.Configured,
    )

    @Test
    fun onAFirstStartTheBlockerIsInstalledOnlyAfterTheGuardIsReadyWithPrivateAccess() {
        val (state, actions) = run(*firstStart)
        assertEquals(StartPhase.READY, state.phase)
        assertEquals(BlockerStatus.READY, state.status)
        assertEquals(
            listOf(
                StartAction.InstallGuard,
                StartAction.AllowGuardPrivate,
                StartAction.ConfigureGuard,
                StartAction.InstallBlocker,
                StartAction.AllowBlockerPrivate,
                StartAction.EnableBlocker,
                StartAction.AwaitFilters,
                StartAction.Configure,
                StartAction.ReleasePages(unprotected = false),
            ),
            actions,
        )
        assertTrue(state.pagesReleased)
        assertFalse(state.startedUnprotected)
    }

    @Test
    fun aReadyFromBeforeTheGuardsRestartDoesNotInstallTheBlocker() {
        val (state, actions) = run(
            StartEvent.Start(true),
            StartEvent.GuardInstalled(privateAllowed = false),
            StartEvent.GuardReady,
            StartEvent.GuardReady,
        )
        assertEquals(StartPhase.GUARD_ALLOWING_PRIVATE, state.phase)
        assertFalse(StartAction.InstallBlocker in actions)
        assertFalse(state.guardUp)
        // The port that closes while private access is granted is the expected restart.
        val (after, more) = run(StartEvent.GuardLost, from = state)
        assertEquals(state, after)
        assertTrue(more.isEmpty())
    }

    @Test
    fun onALaterStartNothingRestartsAndTheOrderIsTheSame() {
        val (state, actions) = run(
            StartEvent.Start(true),
            StartEvent.GuardInstalled(privateAllowed = true),
            StartEvent.GuardReady,
            StartEvent.BlockerInstalled(privateAllowed = true, enabled = true),
            StartEvent.FiltersReady,
            StartEvent.Configured,
        )
        assertEquals(StartPhase.READY, state.phase)
        assertEquals(
            listOf(
                StartAction.InstallGuard,
                StartAction.ConfigureGuard,
                StartAction.InstallBlocker,
                StartAction.AwaitFilters,
                StartAction.Configure,
                StartAction.ReleasePages(false),
            ),
            actions,
        )
    }

    @Test
    fun pagesWaitUntilTheBlockerFiltersAndNoLonger() {
        // Everything up to "configured" keeps pages waiting.
        for (n in 1 until firstStart.size) {
            val (state, actions) = run(*firstStart.take(n).toTypedArray())
            assertFalse("after $n events", state.pagesReleased)
            assertFalse(actions.any { it is StartAction.ReleasePages })
            assertEquals(BlockerStatus.STARTING, state.status)
        }
        // Ten seconds without it: pages go, and the state remembers they went unprotected.
        val (waiting, _) = run(*firstStart.take(7).toTypedArray())
        val (late, actions) = run(StartEvent.HoldTimeout, from = waiting)
        assertEquals(listOf(StartAction.ReleasePages(unprotected = true)), actions)
        assertTrue(late.startedUnprotected)
        assertEquals(BlockerStatus.STARTING, late.status)
        // The blocker still finishes coming up, and pages are not released twice.
        val (ready, rest) = run(
            StartEvent.BlockerPrivateAllowed,
            StartEvent.BlockerEnabled,
            StartEvent.FiltersReady,
            StartEvent.Configured,
            from = late,
        )
        assertEquals(StartPhase.READY, ready.phase)
        assertFalse(rest.any { it is StartAction.ReleasePages })
        // A timeout after a normal start does nothing.
        val (done, _) = run(*firstStart)
        assertEquals(done to emptyList<StartAction>(), step(done, StartEvent.HoldTimeout))
    }

    @Test
    fun withoutAGuardThereIsNoEnabledBlocker() {
        // The guard cannot be installed: the blocker is never installed, one left by an earlier
        // run is disabled, and pages load.
        val (failed, actions) = run(StartEvent.Start(true), StartEvent.GuardFailed)
        assertEquals(StartPhase.UNAVAILABLE, failed.phase)
        assertEquals(
            listOf(StartAction.InstallGuard, StartAction.DisableBlocker, StartAction.ReleasePages(false)),
            actions,
        )
        assertFalse(StartAction.InstallBlocker in actions)

        // The guard goes away while everything runs: the blocker is disabled.
        val (ready, _) = run(*firstStart)
        val (lost, onLoss) = run(StartEvent.GuardLost, from = ready)
        assertEquals(StartPhase.UNAVAILABLE, lost.phase)
        assertEquals(listOf<StartAction>(StartAction.DisableBlocker), onLoss)
        assertFalse(lost.guardUp)

        // A blocker whose install answers after that is disabled again.
        val (_, late) = run(StartEvent.BlockerInstalled(privateAllowed = true, enabled = true), from = lost)
        assertEquals(listOf<StartAction>(StartAction.DisableBlocker), late)

        // The guard comes back (its process restarted): the blocker is brought up again behind it.
        val (back, again) = run(StartEvent.GuardReady, from = lost)
        assertEquals(StartPhase.BLOCKER_INSTALLING, back.phase)
        assertEquals(listOf(StartAction.ConfigureGuard, StartAction.InstallBlocker), again)
    }

    @Test
    fun withBlockingSwitchedOffTheGuardStillRunsAndPagesDoNotWait() {
        val (state, actions) = run(
            StartEvent.Start(blockingWanted = false),
            StartEvent.GuardInstalled(privateAllowed = true),
            StartEvent.GuardReady,
        )
        assertEquals(StartPhase.OFF, state.phase)
        assertEquals(BlockerStatus.OFF, state.status)
        assertTrue(state.guardUp)
        // The guard gets its settings (rewriter, gate); the blocker is not installed, and one
        // from an earlier run is disabled.
        assertEquals(
            listOf(
                StartAction.InstallGuard,
                StartAction.ConfigureGuard,
                StartAction.DisableBlocker,
                StartAction.ReleasePages(false),
            ),
            actions,
        )
        // The guard restarting later is configured again; still no blocker.
        val (_, more) = run(StartEvent.GuardReady, from = state)
        assertEquals(listOf<StartAction>(StartAction.ConfigureGuard), more)
    }

    @Test
    fun theBlockingSwitchTurnsTheBlockerOffAndOnAtRunTime() {
        val (ready, _) = run(*firstStart)
        val (off, turnOff) = run(StartEvent.BlockingWanted(false), from = ready)
        assertEquals(StartPhase.OFF, off.phase)
        assertEquals(listOf<StartAction>(StartAction.DisableBlocker), turnOff)
        assertTrue(off.guardUp)
        // Saying the same again changes nothing.
        assertEquals(off to emptyList<StartAction>(), step(off, StartEvent.BlockingWanted(false)))

        val (on, turnOn) = run(
            StartEvent.BlockingWanted(true),
            StartEvent.BlockerInstalled(privateAllowed = true, enabled = false),
            StartEvent.BlockerEnabled,
            StartEvent.FiltersReady,
            StartEvent.Configured,
            from = off,
        )
        assertEquals(StartPhase.READY, on.phase)
        assertEquals(
            listOf(
                StartAction.InstallBlocker,
                StartAction.EnableBlocker,
                StartAction.AwaitFilters,
                StartAction.Configure,
            ),
            turnOn,
        )
    }

    @Test
    fun aBlockerThatFailsLeavesPagesLoadingAndIsDisabled() {
        val (state, actions) = run(
            StartEvent.Start(true),
            StartEvent.GuardInstalled(true),
            StartEvent.GuardReady,
            StartEvent.BlockerFailed,
        )
        assertEquals(BlockerStatus.UNAVAILABLE, state.status)
        assertEquals(listOf(StartAction.DisableBlocker, StartAction.ReleasePages(false)), actions.takeLast(2))
    }

    @Test
    fun theGuardsScriptRestartingWhileReadyReconnectsTheBlocker() {
        val (ready, _) = run(*firstStart)
        val (state, actions) = run(StartEvent.GuardReady, from = ready)
        assertEquals(StartPhase.WAITING_FILTERS, state.phase)
        assertEquals(listOf(StartAction.ConfigureGuard, StartAction.AwaitFilters), actions)
    }

    @Test
    fun aDisabledBlockerFromAnEarlierOffIsEnabledAgainAtStart() {
        val (_, actions) = run(
            StartEvent.Start(true),
            StartEvent.GuardInstalled(true),
            StartEvent.GuardReady,
            StartEvent.BlockerInstalled(privateAllowed = true, enabled = false),
            StartEvent.BlockerEnabled,
        )
        assertEquals(listOf(StartAction.EnableBlocker, StartAction.AwaitFilters), actions.takeLast(2))
    }

    @Test
    fun theHoldTimeoutReleasesPagesForTheBlockerButNeverForTheVpn() {
        // The VPN is on at start; the blocker is slow and its time limit passes.
        val gate = PageGate(vpnRequired = true)
        var loaded = false
        gate.run { loaded = true }
        val (waiting, _) = run(*firstStart.take(7).toTypedArray())
        val (_, actions) = run(StartEvent.HoldTimeout, from = waiting)
        for (action in actions) if (action is StartAction.ReleasePages) gate.open()
        assertFalse(gate.isOpen)
        assertFalse(loaded)
        // The guard reports its VPN filter: now they go.
        gate.vpnFilterInstalled(true)
        assertTrue(loaded)
    }

    @Test
    fun vpnConfigsNeverPrintTheirSecrets() {
        val pass = "0123456789abcdef".repeat(4)
        val on = VpnGuardConfig.connected(40123, pass, VpnGuardScope.PRIVATE, lan = true)
        assertFalse(on.toString().contains(pass))
        assertFalse(on.toString().contains("40123"))
        assertEquals(on, VpnGuardConfig.connected(40123, pass, VpnGuardScope.PRIVATE, lan = true))
        assertTrue(on.on && !on.pending)
        assertEquals("private", on.scope.wire)
        val pending = VpnGuardConfig.pending(VpnGuardScope.ALL, lan = false)
        assertTrue(pending.pending)
        assertEquals(null, pending.port)
        assertEquals(null, pending.pass)
        for (bad in listOf("short", pass.uppercase(), pass + "0")) {
            assertTrue(runCatching { VpnGuardConfig.connected(1, bad, VpnGuardScope.ALL, false) }.isFailure)
        }
        assertTrue(runCatching { VpnGuardConfig.connected(0, pass, VpnGuardScope.ALL, false) }.isFailure)
    }
}
