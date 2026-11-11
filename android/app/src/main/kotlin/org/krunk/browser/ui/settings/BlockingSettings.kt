package org.krunk.browser.ui.settings

import org.krunk.browser.engine.blocker.UpdateState

/** What the line under "Update filter lists" says. */
sealed interface UpdateLine {
    /** Never updated: the lists are the ones shipped in the app. */
    data object FromPackage : UpdateLine

    /** Last updated at [at] (Unix ms). */
    data class UpdatedAt(val at: Long) : UpdateLine

    data object Updating : UpdateLine

    /** The last try brought nothing (no connection, or blocking is not running). */
    data class Failed(val lastUpdated: Long?) : UpdateLine
}

/**
 * [state] is the running or just finished update, [stored] the time of the last update that
 * brought newer lists (kept across restarts).
 */
fun updateLine(state: UpdateState, stored: Long?): UpdateLine = when (state) {
    is UpdateState.Running -> UpdateLine.Updating
    is UpdateState.Failed -> UpdateLine.Failed(stored)
    is UpdateState.Done -> UpdateLine.UpdatedAt(maxOf(state.at, stored ?: 0L))
    UpdateState.Idle -> if (stored != null && stored > 0) UpdateLine.UpdatedAt(stored) else UpdateLine.FromPackage
}

/** Whether the "Update filter lists" row can be pressed. */
fun updateEnabled(state: UpdateState, blockingOn: Boolean): Boolean = blockingOn && state !is UpdateState.Running
