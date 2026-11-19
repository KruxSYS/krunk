package org.krunk.browser.ui.tv

import android.view.KeyEvent

/**
 * Where the remote's keys go on TV (0.12.0).
 *
 * - [CHROME]: Krunk's own UI (toolbar, new tab page, menus) has the focus; Compose moves it.
 * - [PAGE]: the web page has the focus. Up/Down scroll it (Gecko itself), Left/Right move the
 *   page's focus to the previous/next link or field (Shift+Tab/Tab), Center activates it. Up on
 *   the top of the page, or Menu, goes back to the toolbar.
 * - [CURSOR]: a pointer over the page. Arrows move it, Center taps where it is, the page scrolls
 *   when it pushes against an edge. Long-press Center (or the menu entry) turns it on and off,
 *   Back turns it off.
 */
enum class TvMode { CHROME, PAGE, CURSOR }

/** What one key event does ([TvKeyRouter.route]). */
sealed interface TvAction {
    /** Not ours: the normal dispatch (Compose focus, Gecko, Back handling). */
    data object Pass : TvAction

    /** Swallowed (the other half of a key handled on its down or up). */
    data object Consume : TvAction

    data object ToChrome : TvAction

    data object ToPage : TvAction

    /** Move the page's focus: Shift+Tab when [back], else Tab. */
    data class FocusStep(val back: Boolean) : TvAction

    /** Activate the page's focused element (Enter). */
    data object Activate : TvAction

    data object ToggleCursor : TvAction

    data object ExitCursor : TvAction

    /** Move the pointer one step in a direction (-1, 0, 1), [repeat] = the key's repeat count. */
    data class CursorMove(val dx: Int, val dy: Int, val repeat: Int) : TvAction

    data object CursorClick : TvAction
}

/**
 * The key decisions, kept apart from Android views so they are unit-tested. One instance per
 * browser screen; it remembers only whether the Center key being held has become a long press.
 */
class TvKeyRouter {
    private var centerLong = false
    private var centerDown = false

    /**
     * [down] for ACTION_DOWN (else ACTION_UP), [repeat] the repeat count, [longPress] the
     * event's long-press flag. [pageAtTop]: the page is scrolled to its very top. [hasPage]: a web
     * page is on screen (not the new tab page or a Krunk screen).
     */
    fun route(
        mode: TvMode,
        keyCode: Int,
        down: Boolean,
        repeat: Int,
        longPress: Boolean,
        pageAtTop: Boolean,
        hasPage: Boolean,
    ): TvAction {
        if (!hasPage) {
            centerDown = false
            centerLong = false
            return TvAction.Pass
        }
        if (keyCode == KeyEvent.KEYCODE_MENU) {
            if (!down) return TvAction.Consume
            if (repeat > 0) return TvAction.Consume
            return if (mode == TvMode.CHROME) TvAction.ToPage else TvAction.ToChrome
        }
        return when (mode) {
            TvMode.CHROME -> TvAction.Pass
            TvMode.PAGE -> page(keyCode, down, repeat, longPress, pageAtTop)
            TvMode.CURSOR -> cursor(keyCode, down, repeat, longPress)
        }
    }

    private fun page(keyCode: Int, down: Boolean, repeat: Int, longPress: Boolean, pageAtTop: Boolean): TvAction =
        when (keyCode) {
            KeyEvent.KEYCODE_DPAD_UP ->
                if (down && repeat == 0 && pageAtTop) TvAction.ToChrome else TvAction.Pass

            KeyEvent.KEYCODE_DPAD_LEFT -> if (down) TvAction.FocusStep(back = true) else TvAction.Consume

            KeyEvent.KEYCODE_DPAD_RIGHT -> if (down) TvAction.FocusStep(back = false) else TvAction.Consume

            KeyEvent.KEYCODE_DPAD_CENTER -> center(down, repeat, longPress, TvAction.Activate)

            else -> TvAction.Pass
        }

    private fun cursor(keyCode: Int, down: Boolean, repeat: Int, longPress: Boolean): TvAction {
        fun move(dx: Int, dy: Int) = if (down) TvAction.CursorMove(dx, dy, repeat) else TvAction.Consume
        return when (keyCode) {
            KeyEvent.KEYCODE_DPAD_UP -> move(0, -1)

            KeyEvent.KEYCODE_DPAD_DOWN -> move(0, 1)

            KeyEvent.KEYCODE_DPAD_LEFT -> move(-1, 0)

            KeyEvent.KEYCODE_DPAD_RIGHT -> move(1, 0)

            KeyEvent.KEYCODE_DPAD_CENTER, KeyEvent.KEYCODE_ENTER -> center(
                down,
                repeat,
                longPress,
                TvAction.CursorClick,
            )

            KeyEvent.KEYCODE_BACK -> if (down) TvAction.Consume else TvAction.ExitCursor

            else -> TvAction.Pass
        }
    }

    /**
     * Center acts when it is let go ([short]), or as a long press (cursor on/off) once it is
     * held: Android flags the first repeat as the long press. The up after a long press is
     * swallowed.
     */
    private fun center(down: Boolean, repeat: Int, longPress: Boolean, short: TvAction): TvAction {
        if (down) {
            if (repeat == 0) {
                centerDown = true
                centerLong = false
                return TvAction.Consume
            }
            if ((longPress || repeat > 0) && centerDown && !centerLong) {
                centerLong = true
                return TvAction.ToggleCursor
            }
            return TvAction.Consume
        }
        val wasLong = centerLong
        val wasDown = centerDown
        centerDown = false
        centerLong = false
        return if (wasLong || !wasDown) TvAction.Consume else short
    }
}

/**
 * The pointer of the cursor mode, in the page view's pixels. Steps grow while an arrow is held
 * ([step]); a step past an edge moves the pointer to the edge and returns the rest as the amount
 * the page has to scroll.
 */
class TvCursor(var x: Float = 0f, var y: Float = 0f) {
    /** The page scroll a [move] asks for, in pixels (0 when the pointer stayed inside). */
    data class Scroll(val dx: Float, val dy: Float)

    fun center(width: Int, height: Int) {
        x = width / 2f
        y = height / 2f
    }

    fun move(dx: Int, dy: Int, repeat: Int, width: Int, height: Int, density: Float): Scroll {
        val s = step(repeat) * density
        val nx = x + dx * s
        val ny = y + dy * s
        val maxX = (width - 1).coerceAtLeast(0).toFloat()
        val maxY = (height - 1).coerceAtLeast(0).toFloat()
        x = nx.coerceIn(0f, maxX)
        y = ny.coerceIn(0f, maxY)
        return Scroll(nx - x, ny - y)
    }

    companion object {
        /** dp per key event: slow for aiming, faster the longer an arrow is held. */
        fun step(repeat: Int): Float = when {
            repeat < 3 -> 12f
            repeat < 10 -> 24f
            else -> 40f
        }
    }
}

/**
 * The browser screen's key handler on TV, asked first by the activity for every key
 * (`MainActivity.dispatchKeyEvent`). Set while the browser is on top; returns true for a
 * handled key.
 */
object TvKeys {
    @Volatile var handler: ((KeyEvent) -> Boolean)? = null
}
