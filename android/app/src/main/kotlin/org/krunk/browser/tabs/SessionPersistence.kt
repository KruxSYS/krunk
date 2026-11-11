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
     */
    fun plan(startup: StartupMode, cleanExit: Boolean, sessionJson: String?, countTabs: (String) -> Int): StartupPlan {
        if (sessionJson == null) return StartupPlan.Fresh
        val wantsRestore = startup == StartupMode.RESTORE_SESSION
        return when {
            wantsRestore -> StartupPlan.Restore(sessionJson)

            cleanExit -> StartupPlan.Fresh

            else -> {
                val n = countTabs(sessionJson)
                if (n > 0) StartupPlan.Prompt(sessionJson, n) else StartupPlan.Fresh
            }
        }
    }

    /**
     * Pairs saved Gecko states with the restored tabs. States are saved as `(url, state)` in tab
     * order; the core may skip tabs it cannot restore, so each tab takes the entry at its own
     * position when the URL matches, else the first unused entry with the same URL. When every
     * count matches but URLs differ (credentials stripped, legacy origins migrated) positions win.
     */
    fun assignStates(tabs: List<Pair<UInt, String>>, saved: List<Pair<String, String>>): Map<UInt, String> {
        val out = HashMap<UInt, String>()
        val used = BooleanArray(saved.size)
        tabs.forEachIndexed { i, (id, url) ->
            val pos = saved.getOrNull(i)
            val idx = when {
                pos != null && !used[i] && pos.first == url -> i
                else -> saved.indices.firstOrNull { !used[it] && saved[it].first == url } ?: -1
            }
            if (idx >= 0) {
                used[idx] = true
                if (saved[idx].second.isNotEmpty()) out[id] = saved[idx].second
            }
        }
        if (out.isEmpty() && tabs.size == saved.size) {
            tabs.forEachIndexed { i, (id, _) -> if (saved[i].second.isNotEmpty()) out[id] = saved[i].second }
        }
        return out
    }
}

/** The crash-restore offer shown by the browser screen. */
data class RestorePromptState(val tabCount: Int)
