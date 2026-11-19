package org.krunk.browser.ui.tv

import android.os.SystemClock
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.inputmethod.InputMethodManager
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.translate
import org.mozilla.geckoview.GeckoSession
import org.mozilla.geckoview.ScreenLength

/**
 * The browser screen's TV state (0.12.0): which part has the remote ([mode]), the cursor, and
 * the page's scroll position (Up on the top of the page goes to the toolbar).
 */
@Stable
class TvBrowserState {
    var mode by mutableStateOf(TvMode.CHROME)
    var cursorAt by mutableStateOf(Offset.Zero)
        private set
    val router = TvKeyRouter()
    private val cursor = TvCursor()
    private var cursorPlaced = false

    /** CSS pixels from the top of the page on screen (GeckoSession.ScrollDelegate). */
    var scrollY = 0

    fun showCursor(view: View) {
        if (!cursorPlaced || cursor.x >= view.width || cursor.y >= view.height) {
            cursor.center(view.width, view.height)
            cursorPlaced = true
        }
        cursorAt = Offset(cursor.x, cursor.y)
        mode = TvMode.CURSOR
    }

    /** Moves the pointer; at an edge the page scrolls by what is left of the step. */
    fun moveCursor(view: View, session: GeckoSession, dx: Int, dy: Int, repeat: Int) {
        val density = view.resources.displayMetrics.density
        val scroll = cursor.move(dx, dy, repeat, view.width, view.height, density)
        cursorAt = Offset(cursor.x, cursor.y)
        if (scroll.dx != 0f || scroll.dy != 0f) {
            session.panZoomController.scrollBy(
                ScreenLength.fromPixels((scroll.dx * EDGE_SCROLL).toDouble()),
                ScreenLength.fromPixels((scroll.dy * EDGE_SCROLL).toDouble()),
            )
        }
        hover(view, session)
    }

    /** A tap where the pointer is: a real touch for the page, so any element reacts. */
    fun click(view: View) {
        val now = SystemClock.uptimeMillis()
        val x = cursor.x
        val y = cursor.y
        fun touch(action: Int, time: Long) = MotionEvent.obtain(now, time, action, x, y, 0).apply {
            source = InputDevice.SOURCE_TOUCHSCREEN
        }
        val down = touch(MotionEvent.ACTION_DOWN, now)
        val up = touch(MotionEvent.ACTION_UP, now + TAP_MS)
        view.dispatchTouchEvent(down)
        view.dispatchTouchEvent(up)
        down.recycle()
        up.recycle()
    }

    /** Hover where the pointer is, as a mouse would: menus that open on hover open. */
    private fun hover(view: View, session: GeckoSession) {
        val now = SystemClock.uptimeMillis()
        val props = arrayOf(
            MotionEvent.PointerProperties().apply {
                id = 0
                toolType = MotionEvent.TOOL_TYPE_MOUSE
            },
        )
        val coords = arrayOf(
            MotionEvent.PointerCoords().apply {
                x = cursor.x
                y = cursor.y
            },
        )
        val ev = MotionEvent.obtain(
            now, now, MotionEvent.ACTION_HOVER_MOVE, 1, props, coords, 0, 0, 1f, 1f, 0, 0,
            InputDevice.SOURCE_MOUSE, 0,
        )
        session.panZoomController.onMouseEvent(ev)
        ev.recycle()
    }

    private companion object {
        const val EDGE_SCROLL = 3f
        const val TAP_MS = 40L
    }
}

/** Sends a key (down and up) to the page view as if a keyboard typed it. */
fun View.sendKey(keyCode: Int, meta: Int = 0) {
    val now = SystemClock.uptimeMillis()
    dispatchKeyEvent(KeyEvent(now, now, KeyEvent.ACTION_DOWN, keyCode, 0, meta))
    dispatchKeyEvent(KeyEvent(now, now, KeyEvent.ACTION_UP, keyCode, 0, meta))
}

/**
 * Wires the remote to the page while [active] (TV, the browser on top, a web page on screen,
 * not editing the address): the activity hands every key to [TvKeys.handler] first. [toChrome]
 * moves the focus to the toolbar.
 */
@Composable
fun TvPageKeys(state: TvBrowserState, view: View?, session: GeckoSession?, active: Boolean, toChrome: () -> Unit) {
    DisposableEffect(state, view, session, active) {
        if (view == null || session == null || !active) {
            if (state.mode == TvMode.CURSOR) state.mode = TvMode.PAGE
            return@DisposableEffect onDispose { }
        }
        state.scrollY = 0
        session.scrollDelegate = object : GeckoSession.ScrollDelegate {
            override fun onScrollChanged(session: GeckoSession, scrollX: Int, scrollY: Int) {
                state.scrollY = scrollY
            }
        }
        // The view gets or loses the focus some other way too (Compose focus search, a click).
        view.setOnFocusChangeListener { _, has ->
            if (has && state.mode == TvMode.CHROME) state.mode = TvMode.PAGE
            if (!has && state.mode != TvMode.CHROME) state.mode = TvMode.CHROME
        }
        if (view.hasFocus() && state.mode == TvMode.CHROME) state.mode = TvMode.PAGE
        val handler: (KeyEvent) -> Boolean = handler@{ ev ->
            val down = ev.action == KeyEvent.ACTION_DOWN
            if (ev.action != KeyEvent.ACTION_DOWN && ev.action != KeyEvent.ACTION_UP) return@handler false
            when (
                val action = state.router.route(
                    mode = state.mode,
                    keyCode = ev.keyCode,
                    down = down,
                    repeat = ev.repeatCount,
                    longPress = ev.isLongPress,
                    pageAtTop = state.scrollY <= 0,
                    hasPage = true,
                )
            ) {
                TvAction.Pass -> false

                TvAction.Consume -> true

                TvAction.ToChrome -> {
                    state.mode = TvMode.CHROME
                    toChrome()
                    true
                }

                TvAction.ToPage -> {
                    view.requestFocus()
                    state.mode = TvMode.PAGE
                    true
                }

                is TvAction.FocusStep -> {
                    view.sendKey(KeyEvent.KEYCODE_TAB, if (action.back) KeyEvent.META_SHIFT_ON else 0)
                    true
                }

                TvAction.Activate -> {
                    view.sendKey(KeyEvent.KEYCODE_ENTER)
                    // A focused text field: the TV keyboard. Gecko offers an input connection
                    // only for an editable element, so nothing opens after a link.
                    view.post {
                        view.context.getSystemService(InputMethodManager::class.java)?.showSoftInput(view, 0)
                    }
                    true
                }

                TvAction.ToggleCursor -> {
                    if (state.mode == TvMode.CURSOR) state.mode = TvMode.PAGE else state.showCursor(view)
                    true
                }

                TvAction.ExitCursor -> {
                    state.mode = TvMode.PAGE
                    true
                }

                is TvAction.CursorMove -> {
                    state.moveCursor(view, session, action.dx, action.dy, action.repeat)
                    true
                }

                TvAction.CursorClick -> {
                    state.click(view)
                    true
                }
            }
        }
        TvKeys.handler = handler
        onDispose {
            if (TvKeys.handler === handler) TvKeys.handler = null
            view.onFocusChangeListener = null
            session.scrollDelegate = null
        }
    }
}

/** The cursor mode's pointer: an arrow with a dark outline, readable on any page. */
@Composable
fun TvCursorOverlay(state: TvBrowserState, modifier: Modifier = Modifier) {
    if (state.mode != TvMode.CURSOR) return
    Canvas(modifier.fillMaxSize()) {
        val s = density * 1.6f
        val arrow = Path().apply {
            moveTo(0f, 0f)
            lineTo(0f, 17f * s)
            lineTo(4.5f * s, 13f * s)
            lineTo(8f * s, 20f * s)
            lineTo(11f * s, 18.6f * s)
            lineTo(7.6f * s, 11.8f * s)
            lineTo(13f * s, 11.8f * s)
            close()
        }
        translate(state.cursorAt.x, state.cursorAt.y) {
            drawPath(arrow, Color.White)
            drawPath(arrow, Color.Black, style = Stroke(width = 1.6f * density, join = StrokeJoin.Round))
        }
    }
}
