package org.krunk.browser.tabs

import android.graphics.Bitmap
import org.krunk.browser.engine.PageState

/** One tab as the UI sees it: the core model's row merged with the engine's page state. */
data class TabUi(
    val id: UInt,
    /** The stored URL (internal pages as `https://krunk.invalid/...`). */
    val url: String,
    /** What the address bar shows (`krunk://...` for internal pages, "" for the new tab page). */
    val displayUrl: String,
    val title: String,
    val private: Boolean,
    /** Has a Gecko session (false = restored lazily or discarded). */
    val live: Boolean,
    /** Live but frozen by the memory policy (`setActive(false)`); wakes on activation. */
    val asleep: Boolean = false,
    /** The `krunk://` page name when the tab shows an internal page, else null. */
    val internalPage: String?,
    val page: PageState,
    val favicon: Bitmap? = null,
    val thumbnail: Bitmap? = null,
) {
    val isNewTab: Boolean get() = internalPage == "newtab"
}

/** One-off events the UI reacts to (menus, downloads, prompts). */
sealed class TabEvent {
    data class ContextMenu(val tabId: UInt, val target: org.krunk.browser.engine.ContextTarget) : TabEvent()

    data class Download(val tabId: UInt, val response: org.mozilla.geckoview.WebResponse) : TabEvent()

    /** Typed `krunk://history` etc.: show the native screen. */
    data class OpenScreen(val page: String) : TabEvent()

    /**
     * The previous run did not end cleanly and a session is stored: offer to restore. Also
     * exposed as state in `TabManager.restorePrompt` (the event may fire before any collector).
     */
    data class RestorePrompt(val tabCount: Int) : TabEvent()
}
