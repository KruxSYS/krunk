package org.krunk.browser.ui.browser

import org.krunk.browser.engine.BackAction

/** How long a second Back leaves Krunk after the "Press back again to exit" hint. */
const val EXIT_WINDOW_MS = 2000L

/** What the browser screen knows when the user presses Back. */
data class BackInput(
    val findActive: Boolean,
    val editing: Boolean,
    val fullScreen: Boolean,
    /** What Back does inside the page (see `backAction`). */
    val backAction: BackAction,
    /** The tab was opened from another tab, and that tab is still open. */
    val backOpenerAlive: Boolean,
    /** The tab was opened for another app (an intent). */
    val fromApp: Boolean,
    /** When the last Back armed the exit (`SystemClock.elapsedRealtime`), or null. */
    val exitArmedAtMs: Long?,
    val nowMs: Long,
)

/** What Back does, first match wins (see [decideBack]). */
enum class BackDecision {
    CloseFind,
    StopEditing,
    LeaveFullScreen,

    /** Leave the "blocked" panel, or one step back in the page's history. */
    PageBack,

    /** Close the tab and show the tab it was opened from. */
    CloseToOpener,

    /** Close the tab and put Krunk in the background (back to the app that opened it). */
    CloseAndBackground,

    /** Second Back within [EXIT_WINDOW_MS]: Krunk goes to the background (tabs stay). */
    Exit,

    /** First Back with nothing else to do: show the hint and remember the time. */
    ArmExit,
}

fun decideBack(i: BackInput): BackDecision = when {
    i.findActive -> BackDecision.CloseFind

    i.editing -> BackDecision.StopEditing

    i.fullScreen -> BackDecision.LeaveFullScreen

    i.backAction != BackAction.NONE -> BackDecision.PageBack

    i.backOpenerAlive -> BackDecision.CloseToOpener

    i.fromApp -> BackDecision.CloseAndBackground

    i.exitArmedAtMs != null && i.nowMs >= i.exitArmedAtMs && i.nowMs - i.exitArmedAtMs <= EXIT_WINDOW_MS ->
        BackDecision.Exit

    else -> BackDecision.ArmExit
}
