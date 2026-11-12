package org.krunk.browser.ui.theme

import kotlin.math.sin
import org.krunk.browser.ui.components.HexColor
import org.krunk.ffi.ThemeHighlight
import org.krunk.ffi.ThemeShape

/** How a tab card shows that it is the active tab (see [ThemeStyles.cardMark]). */
enum class CardMark {
    /** Not the active tab. */
    NONE,

    /** The card's title row is filled with the accent. */
    FILL,

    /** A line in the accent under the title row. */
    UNDERLINE,

    /** A border in the accent around the card. */
    OUTLINE,

    /** Only the title is drawn in the accent. */
    TITLE,
}

/**
 * The parts of a theme that are plain arithmetic: colours as numbers, the gradient's direction,
 * corner sizes and the active card's mark. The colours themselves come resolved (and checked for
 * contrast) from the shared core's `resolve_theme`.
 */
object ThemeStyles {
    private const val OPAQUE_BLACK = 0xFF000000.toInt()

    /** `#rrggbb` as an opaque ARGB number; black for anything unreadable (the core sends none). */
    fun argb(hex: String): Int = HexColor.argb(hex) ?: OPAQUE_BLACK

    /**
     * A gradient's colours from the left edge to the right edge. Android draws a gradient along
     * the toolbar's width, so that the strip behind the status bar and the toolbar under it are
     * one piece; of the stored angle (CSS degrees: 0 runs upwards, 90 to the right) only the
     * side it runs to is kept. A toolbar is so much wider than high that a slanted gradient
     * would look the same.
     */
    fun leftToRight(colors: List<String>, angle: Int): List<Int> {
        val list = colors.map(::argb)
        return if (sin(Math.toRadians(angle.toDouble())) < -EPSILON) list.asReversed() else list
    }

    /**
     * The address field's corner radius in dp. "Rounded" keeps the field as it has always been
     * (a full pill at its 40 dp height), so a theme without a style looks like Krunk did before.
     */
    fun pillCornerDp(shape: ThemeShape): Int = when (shape) {
        ThemeShape.ROUNDED, ThemeShape.PILL -> 20
        ThemeShape.SQUARE -> 6
    }

    /** A tab card's corner radius in dp. */
    fun cardCornerDp(shape: ThemeShape): Int = when (shape) {
        ThemeShape.ROUNDED -> 12
        ThemeShape.SQUARE -> 3
        ThemeShape.PILL -> 28
    }

    /**
     * How the card of the [active] tab is marked. Every style marks it somehow: "none" on
     * Windows (no mark on the tab itself) still colours the title here, because the switcher
     * has no other sign of where you are.
     */
    fun cardMark(highlight: ThemeHighlight, active: Boolean): CardMark = when {
        !active -> CardMark.NONE
        highlight == ThemeHighlight.FILL -> CardMark.FILL
        highlight == ThemeHighlight.UNDERLINE -> CardMark.UNDERLINE
        highlight == ThemeHighlight.OUTLINE -> CardMark.OUTLINE
        else -> CardMark.TITLE
    }

    private const val EPSILON = 1e-9
}
