package org.krunk.browser.ui.tv

import android.app.UiModeManager
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.widget.Toast
import androidx.compose.foundation.IndicationNodeFactory
import androidx.compose.foundation.LocalIndication
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.interaction.FocusInteraction
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.interaction.PressInteraction
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.ripple.RippleAlpha
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.LocalRippleConfiguration
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RippleConfiguration
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusDirection
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.ContentDrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.node.DelegatableNode
import androidx.compose.ui.node.DrawModifierNode
import androidx.compose.ui.node.invalidateDraw
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalInputModeManager
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.krunk.browser.BuildConfig
import org.krunk.browser.R

/**
 * Android TV / Google TV (0.12.0): a television, or a device with the leanback feature. Every
 * TV-only behaviour hangs off this (through [LocalIsTv] in the UI); phones and tablets keep theirs.
 */
fun Context.isTv(): Boolean {
    // Test builds only (BuildConfig.TV_OVERRIDE is a constant false in release builds, so R8
    // drops this): the D-pad checks run on a phone emulator at TV size.
    if (BuildConfig.TV_OVERRIDE && forcedTv()) return true
    return systemIsTv()
}

/** What the system says: a television, or a device with the leanback feature. */
fun Context.systemIsTv(): Boolean {
    val uiMode = getSystemService(UiModeManager::class.java)?.currentModeType
    return uiMode == Configuration.UI_MODE_TYPE_TELEVISION ||
        packageManager.hasSystemFeature(PackageManager.FEATURE_LEANBACK)
}

/** `adb shell setprop debug.krunk.force_tv 1` (read once at start-up). */
private fun forcedTv(): Boolean = runCatching {
    val cls = Class.forName("android.os.SystemProperties")
    val get = cls.getMethod("get", String::class.java, String::class.java)
    (get.invoke(null, FORCE_TV_PROP, "") as? String)?.trim() == "1"
}.getOrDefault(false)

const val FORCE_TV_PROP = "debug.krunk.force_tv"

/** Whether Krunk runs on a TV ([isTv]); provided once by the activity. */
val LocalIsTv = staticCompositionLocalOf { false }

/**
 * The TV overscan safe area for Krunk's own UI (about 5 % of a 960×540 dp TV screen): the
 * chrome and every screen keep inside it. Pages themselves use the whole screen.
 */
val TvSafeArea = PaddingValues(horizontal = 48.dp, vertical = 27.dp)

/** The toolbar's part of [TvSafeArea]: the sides and the top (the page is below it). */
val TvToolbarMargins = PaddingValues(start = 48.dp, end = 48.dp, top = 27.dp)

/**
 * The focus look on TV, for everything inside: plain `clickable`/`selectable`/`toggleable`
 * elements (settings rows, tiles, cards) draw a thick ring and a tint when focused
 * ([TvFocusIndication]); Material components (buttons, menu items, switches) show their
 * focused state layer strongly in the accent colour. Visible from across the room. A no-op on
 * phones and tablets.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TvFocusTheme(content: @Composable () -> Unit) {
    if (!LocalIsTv.current) {
        content()
        return
    }
    val scheme = MaterialTheme.colorScheme
    val indication = remember(scheme.primary, scheme.onSurface) { TvFocusIndication(scheme.primary, scheme.onSurface) }
    val ripple = remember(scheme.primary) {
        RippleConfiguration(
            color = scheme.primary,
            rippleAlpha = RippleAlpha(
                draggedAlpha = 0.16f,
                focusedAlpha = 0.42f,
                hoveredAlpha = 0.08f,
                pressedAlpha = 0.3f,
            ),
        )
    }
    CompositionLocalProvider(LocalIndication provides indication, LocalRippleConfiguration provides ripple) {
        content()
    }
}

/** A focus ring ([ring], 3 dp) over an [accent] tint; pressed adds a little more tint. */
class TvFocusIndication(private val accent: Color, private val ring: Color) : IndicationNodeFactory {
    override fun create(interactionSource: InteractionSource): DelegatableNode = Node(interactionSource, accent, ring)

    override fun equals(other: Any?): Boolean =
        other is TvFocusIndication && other.accent == accent && other.ring == ring

    override fun hashCode(): Int = 31 * accent.hashCode() + ring.hashCode()

    private class Node(private val source: InteractionSource, private val accent: Color, private val ring: Color) :
        Modifier.Node(),
        DrawModifierNode {
        private var focused = 0
        private var pressed = 0

        override fun onAttach() {
            coroutineScope.launch {
                source.interactions.collect { i ->
                    when (i) {
                        is FocusInteraction.Focus -> focused++

                        is FocusInteraction.Unfocus -> focused = (focused - 1).coerceAtLeast(0)

                        is PressInteraction.Press -> pressed++

                        is PressInteraction.Release, is PressInteraction.Cancel ->
                            pressed =
                                (pressed - 1).coerceAtLeast(0)
                    }
                    invalidateDraw()
                }
            }
        }

        override fun ContentDrawScope.draw() {
            drawContent()
            if (focused == 0 && pressed == 0) return
            val radius = CornerRadius(8.dp.toPx())
            drawRoundRect(accent.copy(alpha = if (pressed > 0) 0.38f else 0.24f), cornerRadius = radius)
            if (focused > 0) {
                val w = 3.dp.toPx()
                drawRoundRect(
                    ring,
                    topLeft = Offset(w / 2, w / 2),
                    size = Size(size.width - w, size.height - w),
                    cornerRadius = radius,
                    style = Stroke(w),
                )
            }
        }
    }
}

/**
 * A screen on TV: inside the overscan safe area, and the focus on its first element once it is
 * up, so the remote has somewhere to start (a screen that wants another element first requests it
 * itself). On phones and tablets, or when not [enabled], just [content].
 */
@Composable
fun TvScreen(enabled: Boolean, content: @Composable () -> Unit) {
    if (!enabled || !LocalIsTv.current) {
        content()
        return
    }
    val first = remember { FocusRequester() }
    Box(Modifier.fillMaxSize().padding(TvSafeArea).focusRequester(first).focusGroup()) {
        content()
    }
    // Again when the remote takes over from touch (nothing is focusable in touch mode).
    val inputMode = LocalInputModeManager.current.inputMode
    LaunchedEffect(inputMode) {
        // After the first frame: the lazy lists have their rows by then.
        withFrameNanos { }
        runCatching { first.requestFocus() }
    }
}

/**
 * Starts another app's screen (a file picker, a settings page) that a TV may not have: without
 * one it says so ([R.string.no_app_for_action]) instead of crashing. True when it started.
 */
fun Context.launchOrSay(start: () -> Unit): Boolean = try {
    start()
    true
} catch (_: ActivityNotFoundException) {
    Toast.makeText(this, R.string.no_app_for_action, Toast.LENGTH_LONG).show()
    false
}

/**
 * TV: Up and Down leave a focused text field. Compose does this itself for a hardware D-pad, but
 * not for every remote (virtual and some Bluetooth devices): without it the focus could be stuck
 * in a search field. Only while a text field has the focus (the view is a text editor then);
 * the page view (Gecko) keeps its keys. A no-op on phones and tablets.
 */
@Composable
fun Modifier.tvTextFieldEscape(): Modifier {
    if (!LocalIsTv.current) return this
    val focus = LocalFocusManager.current
    val view = LocalView.current
    return onPreviewKeyEvent { ev ->
        if (ev.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
        val dir = when (ev.key) {
            Key.DirectionUp -> FocusDirection.Up
            Key.DirectionDown -> FocusDirection.Down
            else -> return@onPreviewKeyEvent false
        }
        if (view.findFocus() !== view || !view.onCheckIsTextEditor()) return@onPreviewKeyEvent false
        focus.moveFocus(dir)
    }
}
