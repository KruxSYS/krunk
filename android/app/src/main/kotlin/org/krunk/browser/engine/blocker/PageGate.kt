package org.krunk.browser.engine.blocker

/**
 * Holds page loads until the blocker is filtering (plan 2.2). A page that loads before the
 * bundled blocker has read its lists is not filtered, and at start-up the restored tab would be
 * exactly that page. Everything a tab asks the engine to load goes through [run]: at once when
 * the gate is open, otherwise when it opens. Only loads wait; the app's own screens do not.
 *
 * Main thread only.
 */
class PageGate {
    var isOpen: Boolean = false
        private set
    private val waiting = ArrayList<() -> Unit>()

    /** Runs [load] now, or keeps it (in order) until [open]. */
    fun run(load: () -> Unit) {
        if (isOpen) load() else waiting += load
    }

    /** Opens the gate for good and runs what waited. Calling it again does nothing. */
    fun open() {
        if (isOpen) return
        isOpen = true
        val held = ArrayList(waiting)
        waiting.clear()
        for (load in held) load()
    }

    val held: Int get() = waiting.size
}
