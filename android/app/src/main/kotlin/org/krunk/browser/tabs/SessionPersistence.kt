package org.krunk.browser.tabs

import org.krunk.ffi.StartupMode

/** What to do with the stored session at start-up. */
sealed class StartupPlan {
    /** Nothing stored, or the user wants a fresh start: open the start page. */
    data object Fresh : StartupPlan()

    /** Restore silently (startup setting, or a crash with the restore setting on). */
    data class Restore(val sessionJson: String) : StartupPlan()

    /** The previous run did not exit cleanly: open the start page and offer the session. */
    data class Prompt(val sessionJson: String, val tabCount: Int) : StartupPlan()
}

/** The pure part of session persistence (tested on the JVM). */
object SessionPersistence {
    /**
     * `cleanExit` is the stored clean-exit flag (absent = clean, first run); `countTabs` tells how
     * many tabs a session JSON would restore (0 = nothing worth offering).
     *
     * `uncleanStarts` counts the starts in a row whose previous run did not exit cleanly
     * ([uncleanStarts]): from the second one the session is offered instead of restored, so a
     * page that takes the app down on load cannot do it on every start.
     *
     * `restoredBySystem` says Android is bringing the task back after it killed the process in
     * the background (saved instance state): the user never left, so the tabs come back whatever
     * the start-up setting says.
     */
    fun plan(
        startup: StartupMode,
        cleanExit: Boolean,
        sessionJson: String?,
        uncleanStarts: Int = if (cleanExit) 0 else 1,
        restoredBySystem: Boolean = false,
        countTabs: (String) -> Int,
    ): StartupPlan {
        if (sessionJson == null) return StartupPlan.Fresh
        val wantsRestore = startup == StartupMode.RESTORE_SESSION || restoredBySystem
        val crashLoop = !cleanExit && uncleanStarts >= 2
        return when {
            wantsRestore && !crashLoop -> StartupPlan.Restore(sessionJson)

            cleanExit && !wantsRestore -> StartupPlan.Fresh

            else -> {
                val n = countTabs(sessionJson)
                if (n > 0) StartupPlan.Prompt(sessionJson, n) else StartupPlan.Fresh
            }
        }
    }

    /**
     * After a tab's saved state was restored: whether its address still has to be loaded.
     * [stateUrl] is where the state's history stands, [tabUrl] what the tab model saved. They
     * differ when the state was saved before the engine had reported the last navigation; then
     * the restored history is kept and the tab's address is loaded on top of it. When they agree
     * nothing more is loaded, so the restored page keeps its scroll position and form data.
     */
    fun restoreNeedsLoad(stateUrl: String, tabUrl: String): Boolean = stateUrl.isEmpty() || stateUrl != tabUrl

    /** The new value of the unclean-start counter at start-up: reset by a clean exit. */
    fun uncleanStarts(previous: Int, cleanExit: Boolean): Int = if (cleanExit) 0 else previous.coerceAtLeast(0) + 1

    /**
     * The order in which "close all" closes [ids]: background tabs first, the active tab last.
     * Closing the active tab activates a neighbour, and an unloaded neighbour would get a
     * session and start loading its page only to be closed a moment later.
     */
    fun closeOrder(ids: List<UInt>, active: UInt?): List<UInt> =
        ids.filter { it != active } + ids.filter { it == active }

    /**
     * Pairs saved Gecko states with the restored tabs. States are saved as `(url, state)` in tab
     * order; the core may skip tabs it cannot restore, so each tab takes the entry at its own
     * position when the URL matches, else the first unused entry with the same URL. When every
     * count matches but URLs differ (credentials stripped, legacy origins migrated) positions win.
     */
    fun assignStates(tabs: List<Pair<UInt, String>>, saved: List<Pair<String, String>>): Map<UInt, String> =
        assignSaved(tabs, saved) { it.isNotEmpty() }

    /**
     * The pairing of [assignStates] for any saved value; [worth] says whether a value has
     * anything to give back to a tab (an empty state has not).
     */
    fun <T> assignSaved(
        tabs: List<Pair<UInt, String>>,
        saved: List<Pair<String, T>>,
        worth: (T) -> Boolean,
    ): Map<UInt, T> {
        val out = HashMap<UInt, T>()
        val used = BooleanArray(saved.size)
        tabs.forEachIndexed { i, (id, url) ->
            val pos = saved.getOrNull(i)
            val idx = when {
                pos != null && !used[i] && pos.first == url -> i
                else -> saved.indices.firstOrNull { !used[it] && saved[it].first == url } ?: -1
            }
            if (idx >= 0) {
                used[idx] = true
                if (worth(saved[idx].second)) out[id] = saved[idx].second
            }
        }
        if (out.isEmpty() && tabs.size == saved.size) {
            tabs.forEachIndexed { i, (id, _) -> if (worth(saved[i].second)) out[id] = saved[i].second }
        }
        return out
    }
}

/** The crash-restore offer shown by the browser screen. */
data class RestorePromptState(val tabCount: Int)
