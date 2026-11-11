package org.krunk.browser.tabs

import org.krunk.ffi.AndroidSettings
import org.krunk.ffi.MemoryAction
import org.krunk.ffi.MemoryPolicy
import org.krunk.ffi.MemoryStage
import org.krunk.ffi.MemoryTabState
import org.krunk.ffi.memoryDecide
import org.krunk.ffi.memoryPolicy
import org.krunk.ffi.memoryTickMs

/** A tab as the memory policy needs to see it. */
data class MemoryTabSnapshot(
    val id: UInt,
    val live: Boolean,
    val active: Boolean,
    val private: Boolean,
    val lastActive: Long,
    val playingAudio: Boolean = false,
)

/** The sessions the ticker acts on (the tab manager; a fake in tests). */
interface MemoryHost {
    fun snapshot(): List<MemoryTabSnapshot>

    /** Freeze a background tab (`GeckoSession.setActive(false)`, mark asleep). Idempotent. */
    fun sleep(id: UInt)

    /** Close the tab's session, keep URL and state; it reloads on activation. */
    fun discard(id: UInt)
}

/**
 * Runs the core memory policy (`core::memory`, the same rules as Windows: low memory after the
 * sleep timeout, suspend a minute later, optional discard) over the live background tabs and
 * tracks each tab's stage. Pure apart from the FFI decision function; the tab manager drives
 * [tick] from a coroutine every [tickMs].
 */
class MemoryTicker(private val host: MemoryHost, private val log: (String) -> Unit = {}) {
    private data class StageEntry(val stage: MemoryStage, val since: Long)

    var policy: MemoryPolicy = memoryPolicy(DEFAULT_SLEEP_MINUTES, false, DEFAULT_DISCARD_MINUTES)

    private val stages = HashMap<UInt, StageEntry>()

    val tickMs: Long get() = memoryTickMs(policy).toLong()

    /** The stage a tab is in (Awake when it is not tracked). */
    fun stage(id: UInt): MemoryStage = stages[id]?.stage ?: MemoryStage.AWAKE

    /** The tab became active (or was reloaded): it starts over from Awake. */
    fun wake(id: UInt) {
        stages.remove(id)
    }

    /** Evaluates every tab once; returns the actions applied (for logging and tests). */
    fun tick(now: Long): List<Pair<UInt, MemoryAction>> {
        val snap = host.snapshot()
        val ids = snap.map { it.id }.toSet()
        stages.keys.retainAll(ids)
        var livePrivate = snap.count { it.live && it.private }
        val applied = ArrayList<Pair<UInt, MemoryAction>>()
        for (t in snap) {
            if (!t.live || t.active) {
                stages.remove(t.id)
                continue
            }
            val entry = stages[t.id] ?: StageEntry(MemoryStage.AWAKE, t.lastActive)
            val state = MemoryTabState(
                live = true,
                active = false,
                pinned = false,
                playingAudio = t.playingAudio,
                downloading = false,
                private = t.private,
                stage = entry.stage,
                lastActive = t.lastActive,
                stageSince = entry.since,
            )
            var action = memoryDecide(policy, state, now) ?: continue
            // Like Windows: the only live private tab is never discarded, since closing the last
            // private session ends the private browsing data; it is suspended instead.
            if (action == MemoryAction.DISCARD && t.private && livePrivate <= 1) {
                if (entry.stage == MemoryStage.SUSPENDED) continue
                action = MemoryAction.SUSPEND
            }
            when (action) {
                MemoryAction.SET_LOW_MEMORY -> {
                    stages[t.id] = StageEntry(MemoryStage.LOW_MEMORY, now)
                    host.sleep(t.id)
                }

                MemoryAction.SUSPEND -> {
                    stages[t.id] = StageEntry(MemoryStage.SUSPENDED, now)
                    host.sleep(t.id)
                }

                MemoryAction.DISCARD -> {
                    stages.remove(t.id)
                    if (t.private) livePrivate--
                    host.discard(t.id)
                }
            }
            log("memory: tab ${t.id} -> $action (idle ${(now - t.lastActive) / 1000} s)")
            applied += t.id to action
        }
        return applied
    }

    companion object {
        const val DEFAULT_SLEEP_MINUTES = 5u
        const val DEFAULT_DISCARD_MINUTES = 30u

        /**
         * The policy for the user's settings. `testSeconds` (debug builds:
         * `adb shell setprop debug.krunk.sleep_seconds N`) shrinks the timers like Windows'
         * `--sleep-test-seconds`: sleep after N s, suspend N s later, discard after 4N s.
         * `forceDiscard` turns discard on for such a test run.
         */
        fun policyFor(s: AndroidSettings, testSeconds: Long? = null, forceDiscard: Boolean = false): MemoryPolicy {
            val n = testSeconds?.takeIf { it > 0 }
            if (n != null) {
                val ms = n.coerceAtMost(3600) * 1000
                return MemoryPolicy(
                    sleepAfter = if (s.tabSleepMinutes > 0u) ms else null,
                    suspendDelay = ms,
                    discardAfter = if (s.tabDiscardEnabled || forceDiscard) 4 * ms else null,
                )
            }
            return memoryPolicy(s.tabSleepMinutes, s.tabDiscardEnabled, s.tabDiscardMinutes)
        }
    }
}
