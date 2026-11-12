package org.krunk.browser.tabs

/**
 * One tab's row of `kv["android.gecko_states"]`: the address it was saved for, the engine's
 * serialised state ("" when it had none) and whether the tab asked for the desktop site.
 */
data class SavedTabState(val url: String, val state: String, val desktop: Boolean = false) {
    /** A row without a state and without the mode has nothing to give back to a tab. */
    val worthRestoring: Boolean get() = state.isNotEmpty() || desktop
}

/**
 * "Request desktop site", per tab. The mode belongs to the tab, not to its engine session: a
 * session is thrown away and made again while the tab lives on (the page's process crashed or
 * was killed, the tab was discarded to save memory, the app was restarted), and every new
 * session of the tab starts in the tab's mode, before its first load.
 *
 * The mode is kept here for every open tab and goes when the tab is closed. Normal tabs also
 * carry it in their saved row ([SavedTabState.desktop]), so it comes back with the session after
 * a restart; private tabs never reach the disk. A tab reopened from "Recently closed" is a new
 * tab and starts with the mobile site (the core's closed list has no place for the mode).
 */
class DesktopModes {
    private val desktop = HashSet<UInt>()

    /** The mode a session of tab [id] is created with (and what the menu's check mark shows). */
    operator fun get(id: UInt): Boolean = id in desktop

    /** The user switched "Request desktop site" for the tab. */
    operator fun set(id: UInt, on: Boolean) {
        if (on) desktop += id else desktop -= id
    }

    /** The tab is closed. */
    fun forget(id: UInt) {
        desktop -= id
    }

    /** A duplicate asks for the same kind of site as its original. */
    fun copy(from: UInt, to: UInt) = set(to, get(from))
}

/**
 * Pairs the saved rows with the restored tabs (`id to address`, in tab order), by the rule of
 * [SessionPersistence.assignSaved]. One pairing for the state and the mode: a tab can never get
 * the state of one row and the desktop mode of another.
 */
fun assignSavedTabs(tabs: List<Pair<UInt, String>>, saved: List<SavedTabState>): Map<UInt, SavedTabState> =
    SessionPersistence.assignSaved(tabs, saved.map { it.url to it }) { it.worthRestoring }
