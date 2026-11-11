package org.krunk.browser.ui.browser

import org.krunk.browser.engine.BackAction
import org.krunk.browser.engine.PageState
import org.krunk.browser.engine.backAction

/**
 * The first row of the menu: Back, Forward and (on phones) Home, as icons. What each does and
 * whether it is enabled follows the page's history state, the same state the system back
 * gesture uses.
 */
data class NavRow(
    val back: BackAction,
    val forwardEnabled: Boolean,
    /** Home is in this row on phones; a tablet's toolbar has its own Home button. */
    val showHome: Boolean,
    /** Long press on Back / Forward opens the history list when there is one to show. */
    val backHasHistory: Boolean,
    val forwardHasHistory: Boolean,
) {
    val backEnabled: Boolean get() = back != BackAction.NONE
}

fun navRow(page: PageState?, hasSession: Boolean, toolbarShowsHome: Boolean): NavRow {
    val live = hasSession && page != null
    val back = if (live) backAction(page) else BackAction.NONE
    val forward = live && page?.canGoForward == true && page.blocked == null
    return NavRow(
        back = back,
        forwardEnabled = forward,
        showHome = !toolbarShowsHome,
        backHasHistory = back == BackAction.HISTORY,
        forwardHasHistory = forward,
    )
}
