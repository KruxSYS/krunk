package org.krunk.browser.engine.blocker

import org.krunk.browser.engine.PageState

/**
 * Which tab a "the blocker stopped the load to [url]" report belongs to: the one whose last
 * top-level load went to exactly that address and has not committed or been replaced since.
 * When several tabs match (the same link opened twice) the active one wins, else the newest.
 * No match means the report is stale (the tab is gone or went elsewhere): nothing is shown.
 */
fun blockedTab(url: String, pages: Map<UInt, PageState>, active: UInt?): UInt? {
    val matching = pages.filter { (_, page) -> page.loadingUrl == url && page.blocked == null && page.url != url }.keys
    return when {
        matching.isEmpty() -> null
        active != null && active in matching -> active
        else -> matching.max()
    }
}
