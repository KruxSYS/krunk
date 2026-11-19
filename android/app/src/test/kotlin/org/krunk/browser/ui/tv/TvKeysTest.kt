package org.krunk.browser.ui.tv

import android.view.KeyEvent
import org.junit.Assert.assertEquals
import org.junit.Test

class TvKeysTest {
    private val r = TvKeyRouter()

    private fun route(
        mode: TvMode,
        key: Int,
        down: Boolean = true,
        repeat: Int = 0,
        longPress: Boolean = false,
        atTop: Boolean = false,
        hasPage: Boolean = true,
    ) = r.route(mode, key, down, repeat, longPress, atTop, hasPage)

    @Test
    fun chromeKeepsItsKeysButMenuGoesToThePage() {
        for (key in listOf(
            KeyEvent.KEYCODE_DPAD_UP,
            KeyEvent.KEYCODE_DPAD_DOWN,
            KeyEvent.KEYCODE_DPAD_CENTER,
            KeyEvent.KEYCODE_BACK,
        )) {
            assertEquals(TvAction.Pass, route(TvMode.CHROME, key))
        }
        assertEquals(TvAction.ToPage, route(TvMode.CHROME, KeyEvent.KEYCODE_MENU))
        assertEquals(TvAction.Consume, route(TvMode.CHROME, KeyEvent.KEYCODE_MENU, down = false))
        assertEquals(TvAction.ToChrome, route(TvMode.PAGE, KeyEvent.KEYCODE_MENU))
        assertEquals(TvAction.ToChrome, route(TvMode.CURSOR, KeyEvent.KEYCODE_MENU))
    }

    @Test
    fun withoutAPageNothingIsTaken() {
        assertEquals(TvAction.Pass, route(TvMode.PAGE, KeyEvent.KEYCODE_DPAD_LEFT, hasPage = false))
        assertEquals(TvAction.Pass, route(TvMode.CURSOR, KeyEvent.KEYCODE_MENU, hasPage = false))
    }

    @Test
    fun onThePageUpScrollsUntilTheTopThenGoesToTheToolbar() {
        assertEquals(TvAction.Pass, route(TvMode.PAGE, KeyEvent.KEYCODE_DPAD_UP))
        assertEquals(TvAction.Pass, route(TvMode.PAGE, KeyEvent.KEYCODE_DPAD_DOWN))
        assertEquals(TvAction.ToChrome, route(TvMode.PAGE, KeyEvent.KEYCODE_DPAD_UP, atTop = true))
        // A held Up that reaches the top keeps scrolling (no jump mid-repeat).
        assertEquals(TvAction.Pass, route(TvMode.PAGE, KeyEvent.KEYCODE_DPAD_UP, repeat = 4, atTop = true))
        // Back is the browser's (history).
        assertEquals(TvAction.Pass, route(TvMode.PAGE, KeyEvent.KEYCODE_BACK))
    }

    @Test
    fun leftAndRightStepThroughThePagesLinks() {
        assertEquals(TvAction.FocusStep(back = true), route(TvMode.PAGE, KeyEvent.KEYCODE_DPAD_LEFT))
        assertEquals(TvAction.FocusStep(back = false), route(TvMode.PAGE, KeyEvent.KEYCODE_DPAD_RIGHT))
        assertEquals(TvAction.Consume, route(TvMode.PAGE, KeyEvent.KEYCODE_DPAD_RIGHT, down = false))
    }

    @Test
    fun centerActivatesOnReleaseAndALongPressTogglesTheCursor() {
        assertEquals(TvAction.Consume, route(TvMode.PAGE, KeyEvent.KEYCODE_DPAD_CENTER))
        assertEquals(TvAction.Activate, route(TvMode.PAGE, KeyEvent.KEYCODE_DPAD_CENTER, down = false))

        assertEquals(TvAction.Consume, route(TvMode.PAGE, KeyEvent.KEYCODE_DPAD_CENTER))
        assertEquals(
            TvAction.ToggleCursor,
            route(TvMode.PAGE, KeyEvent.KEYCODE_DPAD_CENTER, repeat = 1, longPress = true),
        )
        assertEquals(TvAction.Consume, route(TvMode.PAGE, KeyEvent.KEYCODE_DPAD_CENTER, repeat = 2))
        assertEquals(TvAction.Consume, route(TvMode.PAGE, KeyEvent.KEYCODE_DPAD_CENTER, down = false))

        // An up without its down (the down went elsewhere, e.g. to a closing dialog) does nothing.
        assertEquals(TvAction.Consume, route(TvMode.PAGE, KeyEvent.KEYCODE_DPAD_CENTER, down = false))
    }

    @Test
    fun cursorModeMovesClicksAndLeavesWithBack() {
        assertEquals(TvAction.CursorMove(0, -1, 0), route(TvMode.CURSOR, KeyEvent.KEYCODE_DPAD_UP))
        assertEquals(TvAction.CursorMove(1, 0, 5), route(TvMode.CURSOR, KeyEvent.KEYCODE_DPAD_RIGHT, repeat = 5))
        assertEquals(TvAction.Consume, route(TvMode.CURSOR, KeyEvent.KEYCODE_DPAD_RIGHT, down = false))
        assertEquals(TvAction.Consume, route(TvMode.CURSOR, KeyEvent.KEYCODE_DPAD_CENTER))
        assertEquals(TvAction.CursorClick, route(TvMode.CURSOR, KeyEvent.KEYCODE_DPAD_CENTER, down = false))
        assertEquals(TvAction.Consume, route(TvMode.CURSOR, KeyEvent.KEYCODE_BACK))
        assertEquals(TvAction.ExitCursor, route(TvMode.CURSOR, KeyEvent.KEYCODE_BACK, down = false))
        // Long press in cursor mode turns it off again.
        route(TvMode.CURSOR, KeyEvent.KEYCODE_DPAD_CENTER)
        assertEquals(
            TvAction.ToggleCursor,
            route(TvMode.CURSOR, KeyEvent.KEYCODE_DPAD_CENTER, repeat = 1, longPress = true),
        )
    }

    @Test
    fun theCursorStaysOnScreenAndPushesThePageAtTheEdges() {
        val c = TvCursor()
        c.center(1000, 500)
        assertEquals(500f, c.x)
        assertEquals(250f, c.y)
        // 12 dp at density 2 = 24 px; inside: no scroll.
        assertEquals(TvCursor.Scroll(0f, 0f), c.move(1, 0, 0, 1000, 500, 2f))
        assertEquals(524f, c.x)
        // Held longer, bigger steps.
        assertEquals(12f, TvCursor.step(0))
        assertEquals(40f, TvCursor.step(20))
        // Against the bottom edge: the pointer stops at the last row, the rest scrolls.
        c.y = 490f
        val s = c.move(0, 1, 0, 1000, 500, 2f)
        assertEquals(499f, c.y)
        assertEquals(15f, s.dy)
        assertEquals(0f, s.dx)
        // And the top edge scrolls up.
        c.y = 5f
        assertEquals(-19f, c.move(0, -1, 0, 1000, 500, 2f).dy)
        assertEquals(0f, c.y)
    }
}
