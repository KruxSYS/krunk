package org.krunk.browser.ui.browser

/**
 * The rules of full screen, free of Android classes so they can be tested on the JVM.
 *
 * A page is *shown* full screen only while its tab is the active one and the browser screen is
 * on top. Every tab that believes it is full screen without being shown is told to leave it
 * ([tabsToLeave]): switching the tab, opening the tab switcher or any other screen ends full
 * screen instead of leaving a hidden tab in it. While a page is shown full screen the window
 * hides the system bars, keeps the screen on and uses the cutout area; all of that, and an
 * orientation the page locked, is taken back when it stops being shown, whatever the reason
 * (Back, the page itself, a tab switch or close, another screen, a crash).
 */
object FullScreenPolicy {
    /** Whether the active tab's page is to be shown full screen. */
    fun shown(activeTab: UInt?, fullScreenTabs: Set<UInt>, browserOnTop: Boolean): Boolean =
        browserOnTop && activeTab != null && activeTab in fullScreenTabs

    /** The tabs whose page must be told to leave full screen: all that are in it without being shown. */
    fun tabsToLeave(activeTab: UInt?, fullScreenTabs: Set<UInt>, browserOnTop: Boolean): Set<UInt> =
        if (shown(activeTab, fullScreenTabs, browserOnTop)) fullScreenTabs - setOfNotNull(activeTab) else fullScreenTabs
}

/**
 * The orientation lock of pages (`screen.orientation.lock()`), as a state machine without
 * Android classes. Orientations are `ActivityInfo.SCREEN_ORIENTATION_*` values, which is what
 * the engine hands over; this class only stores and returns them.
 *
 * - A lock is allowed only while a page is shown full screen (the engine asks for the same of
 *   the page; it does not say which tab asks, and only the active tab can be shown full screen).
 * - The engine announces full screen and the lock on two different paths, so a lock that arrives
 *   just before full screen is known waits ([Answer.Wait]) until [fullScreenChanged] or
 *   [waitExpired] settles it.
 * - The activity's orientation from before the first lock is restored when the page unlocks or
 *   when full screen ends for any reason.
 */
class OrientationLock {
    /** What to answer the engine, and what to do with the activity. */
    sealed class Answer {
        /** Allow, and set the activity's requested orientation to [orientation]. */
        data class Allow(val orientation: Int) : Answer()

        /**
         * Not answered yet: no page is shown full screen, but one may be about to be. Denied
         * when that does not happen within [WAIT_MS].
         */
        data object Wait : Answer()
    }

    /** What happens to the activity and to a waiting request after an event. */
    data class Outcome(
        /** The orientation to set on the activity now, if any. */
        val apply: Int? = null,
        /** The waiting request is settled: allowed (`true`) or denied (`false`). */
        val settle: Boolean? = null,
    )

    private var shown = false
    private var waiting: Int? = null
    private var saved: Int? = null

    /** Whether a page holds a lock (the activity's own orientation is saved). */
    val locked: Boolean get() = saved != null

    /** Whether a request waits for full screen. */
    val isWaiting: Boolean get() = waiting != null

    /**
     * A page asks to lock to [requested]; [current] is the activity's requested orientation now.
     * A request that arrives while another waits replaces it (the caller denies the older one).
     */
    fun lock(requested: Int, current: Int): Answer {
        if (!shown) {
            waiting = requested
            return Answer.Wait
        }
        if (saved == null) saved = current
        return Answer.Allow(requested)
    }

    /** The page unlocks: the orientation to restore, or null when nothing was locked. */
    fun unlock(): Int? {
        waiting = null
        return saved.also { saved = null }
    }

    /**
     * A page is shown full screen ([nowShown]) or stopped being shown. [current] is the
     * activity's requested orientation now.
     */
    fun fullScreenChanged(nowShown: Boolean, current: Int): Outcome {
        if (nowShown == shown) return Outcome()
        shown = nowShown
        if (nowShown) {
            val requested = waiting ?: return Outcome()
            waiting = null
            if (saved == null) saved = current
            return Outcome(apply = requested, settle = true)
        }
        val hadWaiting = waiting != null
        waiting = null
        val restore = saved
        saved = null
        return Outcome(apply = restore, settle = if (hadWaiting) false else null)
    }

    /** Full screen did not start in time: a waiting request is denied. Returns whether one was. */
    fun waitExpired(): Boolean = (waiting != null).also { waiting = null }

    companion object {
        /** How long a lock request waits for full screen to be reported. */
        const val WAIT_MS = 1_000L
    }
}
