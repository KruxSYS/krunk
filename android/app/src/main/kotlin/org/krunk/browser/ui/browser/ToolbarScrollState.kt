package org.krunk.browser.ui.browser

import androidx.compose.animation.core.Animatable
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.unit.Velocity
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

/**
 * How far the toolbar has slid out of view, in pixels (0 = fully shown, [heightPx] = hidden).
 * Fed by the page's nested scroll (see `NestedGeckoView`): scrolling down hides it, scrolling
 * up shows it, letting go settles it to the nearer end. Nothing is consumed: the page scrolls
 * at the same time, and Gecko's dynamic-toolbar support uses the freed space.
 */
@Stable
class ToolbarScrollState(private val scope: CoroutineScope) : NestedScrollConnection {
    var heightPx by mutableFloatStateOf(0f)
    var offsetPx by mutableFloatStateOf(0f)
        private set

    /** When false the toolbar stays put (address editing, find bar, no page). */
    var enabled = true
    private val anim = Animatable(0f)
    private var settle: Job? = null

    override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
        if (!enabled || heightPx <= 0f || source != NestedScrollSource.UserInput) return Offset.Zero
        settle?.cancel()
        // Finger up (content scrolls down): available.y < 0 -> hide.
        offsetPx = (offsetPx - available.y).coerceIn(0f, heightPx)
        return Offset.Zero
    }

    override suspend fun onPreFling(available: Velocity): Velocity {
        settleNow()
        return Velocity.Zero
    }

    override suspend fun onPostFling(consumed: Velocity, available: Velocity): Velocity {
        settleNow()
        return Velocity.Zero
    }

    private fun settleNow() {
        if (heightPx <= 0f) return
        val target = if (offsetPx < heightPx / 2f) 0f else heightPx
        animateTo(target)
    }

    fun show() = animateTo(0f)

    private fun animateTo(target: Float) {
        if (offsetPx == target) return
        settle?.cancel()
        settle = scope.launch {
            anim.snapTo(offsetPx)
            anim.animateTo(target) { offsetPx = value }
        }
    }
}

@Composable
fun rememberToolbarScrollState(): ToolbarScrollState {
    val scope = rememberCoroutineScope()
    return remember { ToolbarScrollState(scope) }
}
