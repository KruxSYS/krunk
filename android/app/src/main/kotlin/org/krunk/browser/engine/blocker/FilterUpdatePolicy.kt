package org.krunk.browser.engine.blocker

/** Who wants the blocker to fetch its filter lists. */
enum class FetchCause {
    /** The user pressed "Update filter lists". */
    USER,

    /** The blocker's own schedule (its auto-update, a list it has never fetched). */
    BLOCKER,

    /** The app starting, the app being installed or upgraded. */
    START,

    /** A timer of Krunk's. */
    TIMER,
}

/**
 * The one rule about filter-list traffic (plan 2.5): nothing is fetched unless the user caused
 * it. A press on "Update filter lists" opens the gate for one run of at most [WINDOW_MS]; the
 * "Update automatically" switch opens it for good. Neither the app's start nor any timer does.
 */
object FilterUpdatePolicy {
    /** How long one user-triggered update may take before the gate closes again. */
    const val WINDOW_MS = 5 * 60_000L

    /** Whether the gate for the blocker's requests is open at [now]. */
    fun gateOpen(automatic: Boolean, windowUntil: Long, now: Long): Boolean = automatic || now < windowUntil

    /** Whether a fetch wanted by [cause] may reach the network at [now]. */
    fun mayFetch(cause: FetchCause, automatic: Boolean, windowUntil: Long, now: Long): Boolean = when (cause) {
        FetchCause.USER -> true
        FetchCause.BLOCKER -> gateOpen(automatic, windowUntil, now)
        FetchCause.START, FetchCause.TIMER -> false
    }

    /** The end of the window a fetch wanted by [cause] opens at [now], or null: only the user opens one. */
    fun windowFor(cause: FetchCause, now: Long): Long? = if (cause == FetchCause.USER) now + WINDOW_MS else null

    /** Whether Krunk itself may start an update for [cause]: only when the user asked. */
    fun mayStart(cause: FetchCause): Boolean = cause == FetchCause.USER
}

/** One filter list as the blocker reports it. */
data class FilterList(val key: String, val written: Long, val filters: Int, val inPackage: Boolean)

/** Where a user-triggered update stands. */
sealed interface UpdateState {
    data object Idle : UpdateState
    data class Running(val since: Long) : UpdateState

    /** Finished: [updated] lists have a newer copy than before. */
    data class Done(val at: Long, val updated: Int) : UpdateState

    /** Nothing arrived (no network, or the blocker is not running). */
    data class Failed(val at: Long) : UpdateState
}

object FilterLists {
    /** How many lists of [after] were written since [before] (new or newer). */
    fun updated(before: List<FilterList>, after: List<FilterList>): Int {
        val old = before.associate { it.key to it.written }
        return after.count { it.written > (old[it.key] ?: 0L) }
    }
}
