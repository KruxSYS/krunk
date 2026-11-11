package org.krunk.browser.ui.browser

import org.krunk.browser.engine.blocker.AllowList
import org.krunk.browser.engine.blocker.BlockerStatus
import org.krunk.browser.engine.blocker.Shield

/** How the shield in the address pill looks. Every mode has its own icon and description. */
enum class ShieldMode {
    /** No shield: the new-tab page, Krunk's own pages, an empty tab. */
    HIDDEN,

    /** The blocker is still coming up (pages wait for it). */
    STARTING,

    /** Blocking; [ShieldUi.count] requests were blocked on this page. */
    ACTIVE,

    /** Blocking is switched off for this page's site (the allow list). */
    ALLOWED,

    /** Blocking is switched off in Settings. */
    OFF,

    /** This page loaded before the blocker was ready and was not checked. */
    UNFILTERED,

    /** The blocker failed; nothing is blocked. */
    UNAVAILABLE,
}

data class ShieldUi(
    val mode: ShieldMode,
    /** Blocked requests as the blocker formats them ("0", "12", "1.2k"); "" when not counting. */
    val count: String = "",
) {
    /** The count is drawn next to the icon only when something was blocked. */
    val showCount: Boolean get() = mode == ShieldMode.ACTIVE && count.isNotEmpty() && count != "0"
}

/**
 * The shield for one tab. [shield] is the blocker's reading for *this* tab (null before it has
 * reported), [allowedEntry] the allow-list entry that covers the page, [startedUnprotected]
 * whether pages were let go at start before the blocker filtered.
 */
fun shieldUi(
    url: String,
    isPage: Boolean,
    status: BlockerStatus,
    shield: Shield?,
    allowedEntry: String?,
    startedUnprotected: Boolean = false,
): ShieldUi = when {
    !isPage || AllowList.siteOf(url) == null -> ShieldUi(ShieldMode.HIDDEN)

    status == BlockerStatus.OFF -> ShieldUi(ShieldMode.OFF)

    status == BlockerStatus.UNAVAILABLE -> ShieldUi(ShieldMode.UNAVAILABLE)

    // Pages that went ahead without the blocker at start stay marked until it reports on them.
    status == BlockerStatus.STARTING ->
        ShieldUi(if (startedUnprotected) ShieldMode.UNFILTERED else ShieldMode.STARTING)

    shield?.unfiltered == true -> ShieldUi(ShieldMode.UNFILTERED)

    // Krunk's own list says so at once; the blocker's "off" confirms it after the reload.
    allowedEntry != null || shield?.off == true -> ShieldUi(ShieldMode.ALLOWED)

    else -> ShieldUi(ShieldMode.ACTIVE, shield?.count?.ifEmpty { "0" } ?: "0")
}

/** What the sheet offers for the per-site switch. */
enum class AllowSwitch {
    /** "Block on this site" is on. */
    BLOCKING,

    /** Blocking is off for this site. */
    ALLOWED,

    /** A private tab: no switch, a hint instead (an allow made here would outlast the tab). */
    PRIVATE_TAB,

    /** Nothing to switch: blocking is off altogether, unavailable, or this is not a web page. */
    NONE,
}

fun allowSwitch(ui: ShieldUi, privateTab: Boolean): AllowSwitch = when (ui.mode) {
    ShieldMode.HIDDEN, ShieldMode.OFF, ShieldMode.UNAVAILABLE -> AllowSwitch.NONE

    ShieldMode.ALLOWED -> if (privateTab) AllowSwitch.PRIVATE_TAB else AllowSwitch.ALLOWED

    ShieldMode.ACTIVE, ShieldMode.STARTING, ShieldMode.UNFILTERED ->
        if (privateTab) AllowSwitch.PRIVATE_TAB else AllowSwitch.BLOCKING
}
