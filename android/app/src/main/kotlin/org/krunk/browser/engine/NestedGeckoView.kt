package org.krunk.browser.engine

import android.content.Context
import android.view.MotionEvent
import androidx.core.view.NestedScrollingChild3
import androidx.core.view.NestedScrollingChildHelper
import androidx.core.view.ViewCompat
import org.mozilla.geckoview.GeckoView
import org.mozilla.geckoview.PanZoomController

/**
 * A [GeckoView] that takes part in Android nested scrolling, like Mozilla's `NestedGeckoView`:
 * vertical touch moves are reported to the parent (Compose's `AndroidView` holder is a
 * `NestedScrollingParent3`, so they reach `Modifier.nestedScroll` connections) before Gecko
 * sees them. That is what moves the toolbar away on scroll and drives pull-to-refresh.
 *
 * Gecko answers the first touch of a gesture with a [PanZoomController.InputResultDetail]:
 * whether the browser (not a scrollable element or a `touchmove` handler inside the page)
 * handles the gesture and which directions the content can scroll or overscroll. The parent
 * only hears about gestures the browser handles on a vertically scrollable page, and sees
 * unconsumed downward distance only when the page is already at its top, so pull-to-refresh
 * never fights a scroll inside the page.
 */
open class NestedGeckoView(context: Context) :
    GeckoView(context),
    NestedScrollingChild3 {
    private val helper = NestedScrollingChildHelper(this)
    private val consumed = IntArray(2)
    private val offsetInWindow = IntArray(2)
    private var lastY = 0
    private var nestedOffsetY = 0

    /** From the last gesture's [PanZoomController.InputResultDetail]. */
    private var handledByBrowser = false
    private var scrollableVertically = false
    private var canOverscrollTop = false

    /** Off while the page is in full screen: the toolbar is gone, nothing to move. */
    var nestedScrollAllowed = true

    /** Called when the user lifts a finger from the page (a tap or the end of a drag). */
    var onTouchUp: (() -> Unit)? = null

    init {
        isNestedScrollingEnabled = true
    }

    override fun onTouchEvent(ev: MotionEvent): Boolean {
        val event = MotionEvent.obtain(ev)
        // Screen coordinates: with the toolbar at the top the view itself moves while the toolbar
        // slides, and view-local positions would count that movement as finger movement.
        val eventY = event.rawY.toInt()
        val handled: Boolean
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                nestedOffsetY = 0
                lastY = eventY
                handledByBrowser = false
                scrollableVertically = false
                canOverscrollTop = false
                // Gecko receives the event through this call; the answer arrives asynchronously
                // and starts the nested scroll (moves before it are plain page scrolls).
                super.onTouchEventForDetailResult(event).accept { detail -> onInputResult(detail) }
                handled = true
            }

            MotionEvent.ACTION_MOVE -> {
                var deltaY = lastY - eventY
                val allow = nestedScrollAllowed && handledByBrowser && scrollableVertically
                if (allow && dispatchNestedPreScroll(0, deltaY, consumed, offsetInWindow)) {
                    deltaY -= consumed[1]
                    event.offsetLocation(0f, -offsetInWindow[1].toFloat())
                    nestedOffsetY += offsetInWindow[1]
                }
                lastY = eventY - offsetInWindow[1]
                if (allow) {
                    // Downward drag at the top of the page is the only distance the page cannot
                    // use; the parent gets it as unconsumed (pull-to-refresh).
                    val unconsumed = if (canOverscrollTop && deltaY < 0) deltaY else 0
                    if (dispatchNestedScroll(0, deltaY - unconsumed, 0, unconsumed, offsetInWindow)) {
                        lastY -= offsetInWindow[1]
                        event.offsetLocation(0f, offsetInWindow[1].toFloat())
                        nestedOffsetY += offsetInWindow[1]
                    }
                }
                handled = super.onTouchEvent(event)
            }

            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                if (event.actionMasked == MotionEvent.ACTION_UP) onTouchUp?.invoke()
                if (hasNestedScrollingParent()) {
                    // The holder forwards flings as Compose pre/post-fling, which is what settles
                    // the toolbar and releases the pull-to-refresh indicator.
                    if (!dispatchNestedPreFling(0f, 0f)) dispatchNestedFling(0f, 0f, false)
                }
                stopNestedScroll()
                handled = super.onTouchEvent(event)
            }

            else -> handled = super.onTouchEvent(event)
        }
        event.recycle()
        return handled
    }

    private fun onInputResult(detail: PanZoomController.InputResultDetail?) {
        detail ?: return
        handledByBrowser = detail.handledResult() == PanZoomController.INPUT_RESULT_HANDLED
        val scrollable = detail.scrollableDirections()
        val vertical = PanZoomController.SCROLLABLE_FLAG_TOP or PanZoomController.SCROLLABLE_FLAG_BOTTOM
        scrollableVertically = scrollable and vertical != 0
        canOverscrollTop = scrollable and PanZoomController.SCROLLABLE_FLAG_TOP == 0 &&
            detail.overscrollDirections() and PanZoomController.OVERSCROLL_FLAG_VERTICAL != 0
        if (handledByBrowser && nestedScrollAllowed) startNestedScroll(ViewCompat.SCROLL_AXIS_VERTICAL)
    }

    // ----- NestedScrollingChild3 (delegated to the helper) -----

    override fun setNestedScrollingEnabled(enabled: Boolean) {
        helper.isNestedScrollingEnabled = enabled
    }

    override fun isNestedScrollingEnabled(): Boolean = helper.isNestedScrollingEnabled

    override fun startNestedScroll(axes: Int): Boolean = helper.startNestedScroll(axes)

    override fun startNestedScroll(axes: Int, type: Int): Boolean = helper.startNestedScroll(axes, type)

    override fun stopNestedScroll() = helper.stopNestedScroll()

    override fun stopNestedScroll(type: Int) = helper.stopNestedScroll(type)

    override fun hasNestedScrollingParent(): Boolean = helper.hasNestedScrollingParent()

    override fun hasNestedScrollingParent(type: Int): Boolean = helper.hasNestedScrollingParent(type)

    override fun dispatchNestedScroll(
        dxConsumed: Int,
        dyConsumed: Int,
        dxUnconsumed: Int,
        dyUnconsumed: Int,
        offsetInWindow: IntArray?,
    ): Boolean = helper.dispatchNestedScroll(dxConsumed, dyConsumed, dxUnconsumed, dyUnconsumed, offsetInWindow)

    override fun dispatchNestedScroll(
        dxConsumed: Int,
        dyConsumed: Int,
        dxUnconsumed: Int,
        dyUnconsumed: Int,
        offsetInWindow: IntArray?,
        type: Int,
    ): Boolean = helper.dispatchNestedScroll(dxConsumed, dyConsumed, dxUnconsumed, dyUnconsumed, offsetInWindow, type)

    override fun dispatchNestedScroll(
        dxConsumed: Int,
        dyConsumed: Int,
        dxUnconsumed: Int,
        dyUnconsumed: Int,
        offsetInWindow: IntArray?,
        type: Int,
        consumed: IntArray,
    ) = helper.dispatchNestedScroll(dxConsumed, dyConsumed, dxUnconsumed, dyUnconsumed, offsetInWindow, type, consumed)

    override fun dispatchNestedPreScroll(dx: Int, dy: Int, consumed: IntArray?, offsetInWindow: IntArray?): Boolean =
        helper.dispatchNestedPreScroll(dx, dy, consumed, offsetInWindow)

    override fun dispatchNestedPreScroll(
        dx: Int,
        dy: Int,
        consumed: IntArray?,
        offsetInWindow: IntArray?,
        type: Int,
    ): Boolean = helper.dispatchNestedPreScroll(dx, dy, consumed, offsetInWindow, type)

    override fun dispatchNestedFling(velocityX: Float, velocityY: Float, consumed: Boolean): Boolean =
        helper.dispatchNestedFling(velocityX, velocityY, consumed)

    override fun dispatchNestedPreFling(velocityX: Float, velocityY: Float): Boolean =
        helper.dispatchNestedPreFling(velocityX, velocityY)
}
