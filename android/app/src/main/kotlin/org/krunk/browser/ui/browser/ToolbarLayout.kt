package org.krunk.browser.ui.browser

/**
 * What the toolbar row holds at a given width, and how much of it the address pill gets. The
 * composable lays the row out from these numbers, so the rule can be tested without a device.
 *
 * Phone: `[pill: shield · lock · site · reload/stop] [tabs] [menu]`. Back and Forward live in
 * the menu's first row (and on the system back gesture); the shield sits inside the pill, so
 * the address keeps the room. From tablet width on, Home is a button of its own.
 */
data class ToolbarLayout(
    /** A Home button in the toolbar (tablets). On phones Home is in the menu's first row. */
    val showHome: Boolean,
    /** Room for the words "Not secure" next to the sign. */
    val showNotSecureWords: Boolean,
    /** Width of everything but the pill, in dp. */
    val fixedDp: Int,
    /** Width left for the address pill, in dp. */
    val pillDp: Int,
) {
    companion object {
        /** From this width (dp) the toolbar is laid out as on a tablet. */
        const val WIDE_DP = 600

        /** Touch target of every toolbar button, and of the shield and reload inside the pill. */
        const val BUTTON_DP = 48

        /** Padding at both ends of the row. */
        const val ROW_PADDING_DP = 4

        /** Space on both sides of the pill. */
        const val PILL_PADDING_DP = 4
    }
}

fun toolbarLayout(widthDp: Int): ToolbarLayout {
    val wide = widthDp >= ToolbarLayout.WIDE_DP
    // The tab counter and the menu button; Home on a tablet.
    val buttons = if (wide) 3 else 2
    val fixed = 2 * ToolbarLayout.ROW_PADDING_DP + 2 * ToolbarLayout.PILL_PADDING_DP + buttons * ToolbarLayout.BUTTON_DP
    return ToolbarLayout(
        showHome = wide,
        showNotSecureWords = wide,
        fixedDp = fixed,
        pillDp = (widthDp - fixed).coerceAtLeast(0),
    )
}

/** The share of the toolbar's width the address pill takes (0..1). */
fun pillShare(widthDp: Int): Float = if (widthDp <= 0) 0f else toolbarLayout(widthDp).pillDp.toFloat() / widthDp
