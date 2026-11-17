package org.krunk.browser.engine.blocker

/**
 * Holds page loads until the blocker is filtering (plan 2.2) and, when Krunk's VPN is on at
 * start, until `krunk-guard`'s VPN filter is in place (plan-android-vpn 3.2). A page that loads
 * before the bundled blocker has read its lists is not filtered, and at start-up the restored
 * tab would be exactly that page; one that loads before the VPN filter would be refused by the
 * backstop and show an error. Everything a tab asks the engine to load goes through [run]: at
 * once when the gate is open, otherwise when it opens. Only loads wait; the app's own screens
 * do not.
 *
 * Two conditions, see [mayOpen]: the blocker's ([open], which also comes from the start's time
 * limit) and the VPN's ([vpnFilterInstalled]). The time limit never stands in for the VPN filter:
 * without it a page would go nowhere anyway (fail-closed), and Krunk must not look as if it
 * loaded it. Once open, the gate stays open.
 *
 * Main thread only.
 */
class PageGate(vpnRequired: Boolean = false) {
    var isOpen: Boolean = false
        private set
    private val waiting = ArrayList<() -> Unit>()
    private var blockerDone = false
    private var vpnRequired = vpnRequired
    private var vpnInstalled = false

    /** Runs [load] now, or keeps it (in order) until the gate opens. */
    fun run(load: () -> Unit) {
        if (isOpen) load() else waiting += load
    }

    /** The blocker's condition is met (it filters, is off or unavailable, or the time passed). */
    fun open() {
        blockerDone = true
        settle()
    }

    /** Whether loads also wait for the VPN filter (the VPN is on). Has no effect once open. */
    fun vpnRequired(required: Boolean) {
        vpnRequired = required
        settle()
    }

    /** What the guard last reported about its VPN filter. */
    fun vpnFilterInstalled(installed: Boolean) {
        vpnInstalled = installed
        settle()
    }

    private fun settle() {
        if (isOpen || !mayOpen(blockerDone, vpnRequired, vpnInstalled)) return
        isOpen = true
        val held = ArrayList(waiting)
        waiting.clear()
        for (load in held) load()
    }

    val held: Int get() = waiting.size

    companion object {
        /** The gate's rule: the blocker's condition, and the VPN filter when the VPN is on. */
        fun mayOpen(blockerDone: Boolean, vpnRequired: Boolean, vpnFilterInstalled: Boolean): Boolean =
            blockerDone && (!vpnRequired || vpnFilterInstalled)
    }
}
