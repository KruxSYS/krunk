package org.krunk.browser.tabs

import org.krunk.ffi.LinkTabsMode

/** How a link came to open a tab. */
enum class LinkOpen {
    /** "Open in new tab" in the link menu (or an image's). */
    LINK_MENU,

    /** The page opened a window itself (`target=_blank`, `window.open`). */
    PAGE_WINDOW,
}

/**
 * Whether a tab opened by a link comes to the front, by the shared "link tabs" setting: as asked
 * (the page's own window in front, the menu's tab behind), always in front, or always behind.
 */
fun opensInFront(mode: LinkTabsMode, how: LinkOpen): Boolean = when (mode) {
    LinkTabsMode.AUTO -> how == LinkOpen.PAGE_WINDOW
    LinkTabsMode.SWITCH -> true
    LinkTabsMode.BACKGROUND -> false
}
