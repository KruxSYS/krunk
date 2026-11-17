package org.krunk.browser.engine.blocker

/**
 * The order in which the blocker's two extensions are brought up, as a pure state machine
 * ([step]); `BlockerController` feeds it the engine's answers and carries out the actions.
 *
 * The rules it keeps (plan 2.2):
 *  - `krunk-guard` (whose gate keeps the blocker off the network) is installed, allowed in
 *    private tabs and has reported ready *after* that before the blocker is installed. Allowing
 *    private tabs restarts an extension, so a "ready" from before the restart does not count.
 *  - Without a working guard there is no enabled blocker.
 *  - Pages wait until the blocker filters, the blocker is off or unavailable, or [HOLD_MS] passed.
 *    With Krunk's VPN on at start they also wait for the guard's VPN filter, and [HOLD_MS] does
 *    not stand in for that ([PageGate.mayOpen]).
 *  - With blocking switched off the guard still runs (its rewriter and the engine's gate apply).
 */
enum class BlockerStatus {
    /** Coming up; pages wait. */
    STARTING,

    /** Filtering. */
    READY,

    /** Switched off by the user in Settings. */
    OFF,

    /** The guard or the blocker failed: nothing is blocked, and the blocker is not enabled. */
    UNAVAILABLE,
}

enum class StartPhase {
    IDLE,
    GUARD_INSTALLING,
    GUARD_ALLOWING_PRIVATE,
    GUARD_WAITING_READY,
    BLOCKER_INSTALLING,
    BLOCKER_ALLOWING_PRIVATE,
    BLOCKER_ENABLING,
    WAITING_FILTERS,
    CONFIGURING,
    READY,
    OFF,
    UNAVAILABLE,
}

data class StartState(
    val phase: StartPhase = StartPhase.IDLE,
    /** The user's "block ads and trackers" switch. */
    val blockingWanted: Boolean = true,
    /** The guard has reported ready with private access in place (its gate is up). */
    val guardUp: Boolean = false,
    /** Page loads were let go ([StartAction.ReleasePages] was issued). */
    val pagesReleased: Boolean = false,
    /** Pages were let go by the time limit, before the blocker was filtering. */
    val startedUnprotected: Boolean = false,
) {
    val status: BlockerStatus
        get() = when (phase) {
            StartPhase.READY -> BlockerStatus.READY
            StartPhase.OFF -> BlockerStatus.OFF
            StartPhase.UNAVAILABLE -> BlockerStatus.UNAVAILABLE
            else -> BlockerStatus.STARTING
        }
}

sealed interface StartEvent {
    data class Start(val blockingWanted: Boolean) : StartEvent
    data class GuardInstalled(val privateAllowed: Boolean) : StartEvent
    data object GuardPrivateAllowed : StartEvent

    /** The guard's background script said "ready". */
    data object GuardReady : StartEvent

    /** Installing the guard or allowing it in private tabs failed. */
    data object GuardFailed : StartEvent

    /** The guard's port closed, or the guard was found disabled. */
    data object GuardLost : StartEvent
    data class BlockerInstalled(val privateAllowed: Boolean, val enabled: Boolean) : StartEvent
    data object BlockerPrivateAllowed : StartEvent
    data object BlockerEnabled : StartEvent
    data object BlockerFailed : StartEvent

    /** The blocker answered "ready to filter" through the bridge. */
    data object FiltersReady : StartEvent

    /** Krunk's settings and allow list are in the blocker. */
    data object Configured : StartEvent
    data class BlockingWanted(val wanted: Boolean) : StartEvent

    /** [HOLD_MS] passed since the start. */
    data object HoldTimeout : StartEvent
}

sealed interface StartAction {
    data object InstallGuard : StartAction
    data object AllowGuardPrivate : StartAction

    /** Send the guard its settings (rewriter on or off, gate state). */
    data object ConfigureGuard : StartAction
    data object InstallBlocker : StartAction
    data object AllowBlockerPrivate : StartAction
    data object EnableBlocker : StartAction

    /** Disable the blocker if the profile has one (it may never have been installed). */
    data object DisableBlocker : StartAction

    /** Open the blocker's page for the bridge and ask it until it is ready to filter. */
    data object AwaitFilters : StartAction

    /** Write Krunk's settings and allow list into the blocker. */
    data object Configure : StartAction
    data class ReleasePages(val unprotected: Boolean) : StartAction
}

/** How long page loads wait for the blocker at start before they go ahead without it. */
const val HOLD_MS = 10_000L

fun step(state: StartState, event: StartEvent): Pair<StartState, List<StartAction>> {
    fun release(s: StartState, unprotected: Boolean = false): Pair<StartState, List<StartAction>> {
        if (s.pagesReleased) return s to emptyList()
        val released = s.copy(pagesReleased = true, startedUnprotected = unprotected)
        return released to listOf(StartAction.ReleasePages(unprotected))
    }

    fun unavailable(s: StartState): Pair<StartState, List<StartAction>> {
        val (next, actions) = release(s.copy(phase = StartPhase.UNAVAILABLE))
        return next to (listOf(StartAction.DisableBlocker) + actions)
    }

    return when (event) {
        is StartEvent.Start ->
            if (state.phase != StartPhase.IDLE) {
                state to emptyList()
            } else {
                state.copy(phase = StartPhase.GUARD_INSTALLING, blockingWanted = event.blockingWanted) to
                    listOf(StartAction.InstallGuard)
            }

        is StartEvent.GuardInstalled ->
            if (state.phase != StartPhase.GUARD_INSTALLING) {
                state to emptyList()
            } else if (event.privateAllowed) {
                state.copy(phase = StartPhase.GUARD_WAITING_READY) to emptyList()
            } else {
                state.copy(phase = StartPhase.GUARD_ALLOWING_PRIVATE) to listOf(StartAction.AllowGuardPrivate)
            }

        StartEvent.GuardPrivateAllowed ->
            if (state.phase == StartPhase.GUARD_ALLOWING_PRIVATE) {
                state.copy(phase = StartPhase.GUARD_WAITING_READY) to emptyList()
            } else {
                state to emptyList()
            }

        StartEvent.GuardReady -> when {
            // Before private access is in place the guard is about to restart: not its final start.
            state.phase == StartPhase.GUARD_INSTALLING || state.phase == StartPhase.GUARD_ALLOWING_PRIVATE ->
                state to emptyList()

            state.phase == StartPhase.GUARD_WAITING_READY -> {
                val up = state.copy(guardUp = true)
                if (up.blockingWanted) {
                    up.copy(phase = StartPhase.BLOCKER_INSTALLING) to
                        listOf(StartAction.ConfigureGuard, StartAction.InstallBlocker)
                } else {
                    val (next, actions) = release(up.copy(phase = StartPhase.OFF))
                    next to (listOf(StartAction.ConfigureGuard, StartAction.DisableBlocker) + actions)
                }
            }

            // The guard's script started again (its process was restarted): it has lost what the
            // app told it, and the blocker's page went with the process.
            state.phase == StartPhase.READY || state.phase == StartPhase.WAITING_FILTERS ||
                state.phase == StartPhase.CONFIGURING ->
                state.copy(phase = StartPhase.WAITING_FILTERS) to
                    listOf(StartAction.ConfigureGuard, StartAction.AwaitFilters)

            // The guard came back after it was lost (its process was restarted).
            state.phase == StartPhase.UNAVAILABLE && !state.guardUp -> {
                val up = state.copy(guardUp = true)
                if (up.blockingWanted) {
                    up.copy(phase = StartPhase.BLOCKER_INSTALLING) to
                        listOf(StartAction.ConfigureGuard, StartAction.InstallBlocker)
                } else {
                    up.copy(phase = StartPhase.OFF) to listOf(StartAction.ConfigureGuard, StartAction.DisableBlocker)
                }
            }

            state.guardUp -> state to listOf(StartAction.ConfigureGuard)

            else -> state to emptyList()
        }

        StartEvent.GuardFailed, StartEvent.GuardLost -> when {
            // Its port closes when private access is granted: that restart is expected.
            event == StartEvent.GuardLost &&
                (state.phase == StartPhase.GUARD_ALLOWING_PRIVATE || state.phase == StartPhase.GUARD_WAITING_READY) &&
                !state.guardUp -> state to emptyList()

            state.phase == StartPhase.UNAVAILABLE || state.phase == StartPhase.IDLE -> state to emptyList()

            else -> unavailable(state.copy(guardUp = false))
        }

        is StartEvent.BlockerInstalled ->
            if (state.phase == StartPhase.OFF || state.phase == StartPhase.UNAVAILABLE) {
                // It arrived after blocking was switched off or the guard was lost.
                state to listOf(StartAction.DisableBlocker)
            } else if (state.phase != StartPhase.BLOCKER_INSTALLING) {
                state to emptyList()
            } else if (!event.privateAllowed) {
                state.copy(phase = StartPhase.BLOCKER_ALLOWING_PRIVATE) to listOf(StartAction.AllowBlockerPrivate)
            } else if (!event.enabled) {
                state.copy(phase = StartPhase.BLOCKER_ENABLING) to listOf(StartAction.EnableBlocker)
            } else {
                state.copy(phase = StartPhase.WAITING_FILTERS) to listOf(StartAction.AwaitFilters)
            }

        StartEvent.BlockerPrivateAllowed ->
            if (state.phase == StartPhase.BLOCKER_ALLOWING_PRIVATE) {
                // Ask again what state it is in: it may also be disabled from an earlier "off".
                state.copy(phase = StartPhase.BLOCKER_ENABLING) to listOf(StartAction.EnableBlocker)
            } else {
                state to emptyList()
            }

        StartEvent.BlockerEnabled ->
            if (state.phase == StartPhase.BLOCKER_ENABLING) {
                state.copy(phase = StartPhase.WAITING_FILTERS) to listOf(StartAction.AwaitFilters)
            } else {
                state to emptyList()
            }

        StartEvent.BlockerFailed -> when (state.phase) {
            StartPhase.BLOCKER_INSTALLING, StartPhase.BLOCKER_ALLOWING_PRIVATE, StartPhase.BLOCKER_ENABLING,
            StartPhase.WAITING_FILTERS, StartPhase.CONFIGURING, StartPhase.READY,
            -> unavailable(state)

            else -> state to emptyList()
        }

        StartEvent.FiltersReady ->
            if (state.phase == StartPhase.WAITING_FILTERS) {
                state.copy(phase = StartPhase.CONFIGURING) to listOf(StartAction.Configure)
            } else {
                state to emptyList()
            }

        StartEvent.Configured ->
            if (state.phase == StartPhase.CONFIGURING) {
                release(state.copy(phase = StartPhase.READY))
            } else {
                state to emptyList()
            }

        is StartEvent.BlockingWanted -> {
            val s = state.copy(blockingWanted = event.wanted)
            when {
                event.wanted == state.blockingWanted -> state to emptyList()

                // Not there yet: the guard's "ready" reads the new value.
                !s.guardUp -> s to emptyList()

                !event.wanted && s.phase != StartPhase.UNAVAILABLE -> {
                    val (next, actions) = release(s.copy(phase = StartPhase.OFF))
                    next to (listOf(StartAction.DisableBlocker) + actions)
                }

                event.wanted && s.phase == StartPhase.OFF ->
                    s.copy(phase = StartPhase.BLOCKER_INSTALLING) to listOf(StartAction.InstallBlocker)

                else -> s to emptyList()
            }
        }

        StartEvent.HoldTimeout ->
            if (state.pagesReleased) state to emptyList() else release(state, unprotected = true)
    }
}

/** Which channels Krunk's VPN carries (the guard's filter sends the others direct). */
enum class VpnGuardScope(val wire: String) {
    ALL("all"),
    PRIVATE("private"),
}

/**
 * What `krunk-guard`'s VPN filter is told (contract 3). [pending]: the VPN is on but its proxy is
 * not up yet, so matching connections are held (and cancelled after 20 s). Otherwise [port] and
 * [pass] (the session token as 64 lowercase hex characters) are the proxy's. Never logged:
 * [toString] leaves the port and the password out.
 */
class VpnGuardConfig private constructor(
    val pending: Boolean,
    val port: Int?,
    val pass: String?,
    val scope: VpnGuardScope,
    val lan: Boolean,
) {
    /** Always true: "off" is a null config. Part of the message all the same (contract 3). */
    val on: Boolean get() = true

    override fun equals(other: Any?): Boolean = other is VpnGuardConfig && pending == other.pending &&
        port == other.port && pass == other.pass && scope == other.scope && lan == other.lan

    override fun hashCode(): Int = listOf(pending, port, pass, scope, lan).hashCode()

    override fun toString(): String = "VpnGuardConfig(pending=$pending, scope=$scope, lan=$lan)"

    companion object {
        /** The proxy is up on 127.0.0.1:[port]; [pass] is its token as hex. */
        fun connected(port: Int, pass: String, scope: VpnGuardScope, lan: Boolean): VpnGuardConfig {
            require(port in 1..65535) { "port out of range" }
            require(pass.length == 64 && pass.all { it in '0'..'9' || it in 'a'..'f' }) { "pass must be 64 hex chars" }
            return VpnGuardConfig(false, port, pass, scope, lan)
        }

        /** The VPN is on, its proxy not up yet. */
        fun pending(scope: VpnGuardScope, lan: Boolean): VpnGuardConfig =
            VpnGuardConfig(true, null, null, scope, lan)
    }
}
