package org.krunk.browser.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.unit.dp
import org.krunk.ffi.ResolvedTheme
import org.krunk.ffi.ThemeAppearance
import org.krunk.ffi.ThemeHighlight
import org.krunk.ffi.ThemeMode
import org.krunk.ffi.ToolbarFill
import org.krunk.ffi.resolveTheme

/** Krunk's palette (ui/shared/common.css). Material dynamic colour is off on purpose. */
object KrunkColors {
    val bgLight = Color(0xFFF3F4F6)
    val bgRaisedLight = Color(0xFFFFFFFF)
    val bgHoverLight = Color(0xFFE5E7EB)
    val fgLight = Color(0xFF1F2937)
    val fgMutedLight = Color(0xFF6B7280)
    val borderLight = Color(0xFFD1D5DB)
    val accentLight = Color(0xFF2563EB)
    val accentFgLight = Color(0xFFFFFFFF)

    val bgDark = Color(0xFF1F2023)
    val bgRaisedDark = Color(0xFF2B2D31)
    val bgHoverDark = Color(0xFF3A3C42)
    val fgDark = Color(0xFFE5E7EB)
    val fgMutedDark = Color(0xFF9CA3AF)
    val borderDark = Color(0xFF44474E)
    val accentDark = Color(0xFF60A5FA)
    val accentFgDark = Color(0xFF0B1220)

    val danger = Color(0xFFDC2626)
    val privateAccent = Color(0xFF6D28D9)
    val privateBgLight = Color(0xFFEDE9FE)
    val privateBgDark = Color(0xFF3B2A5C)
    val secure = Color(0xFF16A34A)
    val bookmarkStar = Color(0xFFF59E0B)
    val brandOrange = Color(0xFFFF6A3D)
    val brandPink = Color(0xFFC2185B)
}

/** The Material colours of a resolved theme (`ffi/src/theme.rs` checks their contrast). */
fun colorSchemeOf(t: ResolvedTheme): ColorScheme {
    fun c(hex: String) = Color(ThemeStyles.argb(hex))
    val base = if (t.dark) darkColorScheme() else lightColorScheme()
    return base.copy(
        primary = c(t.primary),
        onPrimary = c(t.onPrimary),
        primaryContainer = c(t.primaryContainer),
        onPrimaryContainer = c(t.onPrimaryContainer),
        secondary = c(t.textMuted),
        onSecondary = c(t.background),
        background = c(t.background),
        onBackground = c(t.text),
        surface = c(t.background),
        onSurface = c(t.text),
        surfaceVariant = c(t.surfaceHover),
        onSurfaceVariant = c(t.textMuted),
        surfaceContainer = c(t.surfaceRaised),
        surfaceContainerHigh = c(t.surfaceHover),
        surfaceContainerHighest = c(t.border),
        surfaceContainerLow = c(t.surfaceRaised),
        surfaceContainerLowest = c(t.surfaceLowest),
        outline = c(t.border),
        outlineVariant = c(t.surfaceHover),
        error = c(t.error),
        onError = c(t.onError),
    )
}

/**
 * What a theme adds to the Material colours: the toolbar's fill and the colours on it, the
 * address field, the accent as chosen (for fills), and the style's shapes.
 */
@Immutable
data class KrunkStyle(
    /** The toolbar's fill, also drawn behind the status bar when the toolbar is at the top. */
    val toolbar: Brush,
    /** Text and icons on the toolbar. */
    val toolbarText: Color,
    /** The toolbar is not the plain surface (a gradient or the tinted glass). */
    val toolbarStyled: Boolean,
    /** The strip behind the status bar, filled like the toolbar, is dark: light status icons. */
    val stripDark: Boolean,
    val pill: Color,
    val pillText: Color,
    /** The lock of a secure page, readable on the address field. */
    val secure: Color,
    /** The accent for filled areas and what is drawn on it. */
    val accent: Color,
    val onAccent: Color,
    val pillShape: Shape,
    val cardShape: Shape,
    val highlight: ThemeHighlight,
)

fun styleOf(t: ResolvedTheme): KrunkStyle {
    fun c(hex: String) = Color(ThemeStyles.argb(hex))
    val fill = ThemeStyles.leftToRight(t.toolbarColors, t.toolbarAngle.toInt()).map { Color(it) }
    return KrunkStyle(
        toolbar = if (fill.size >
            1
        ) {
            Brush.horizontalGradient(fill)
        } else {
            SolidColor(fill.firstOrNull() ?: c(t.surfaceRaised))
        },
        toolbarText = c(t.toolbarText),
        toolbarStyled = t.toolbarFill != ToolbarFill.SOLID,
        stripDark = t.statusStripDark,
        pill = c(t.pill),
        pillText = c(t.pillText),
        secure = c(t.secure),
        accent = c(t.accent),
        onAccent = c(t.onAccent),
        pillShape = RoundedCornerShape(ThemeStyles.pillCornerDp(t.shape).dp),
        cardShape = RoundedCornerShape(ThemeStyles.cardCornerDp(t.shape).dp),
        highlight = t.highlight,
    )
}

/** The style of the theme in effect (see [KrunkTheme]). */
val LocalKrunkStyle = staticCompositionLocalOf<KrunkStyle> { error("KrunkTheme is missing") }

/** Whether the current theme is dark (for icons that are not Material colours). */
val LocalKrunkDark = staticCompositionLocalOf { false }
val LocalKrunkPrivate = staticCompositionLocalOf { false }

@Composable
fun isDarkTheme(mode: ThemeMode): Boolean = when (mode) {
    ThemeMode.SYSTEM -> isSystemInDarkTheme()
    ThemeMode.LIGHT -> false
    ThemeMode.DARK -> true
}

/**
 * Krunk's theme: light or dark by [mode], the fixed private look for a private tab, and the
 * user's style and accent from the shared appearance settings ([appearance]). No default for
 * [appearance] on purpose: a screen that sets up its own theme (the tab switcher does, for its
 * private list) must pass the user's, or it silently falls back to Krunk's own look.
 */
@Composable
fun KrunkTheme(mode: ThemeMode, privateMode: Boolean, appearance: ThemeAppearance, content: @Composable () -> Unit) {
    val dark = isDarkTheme(mode)
    val theme = remember(appearance, dark, privateMode) { resolveTheme(appearance, dark, privateMode) }
    val scheme = remember(theme) { colorSchemeOf(theme) }
    val style = remember(theme) { styleOf(theme) }
    CompositionLocalProvider(
        LocalKrunkDark provides theme.dark,
        LocalKrunkPrivate provides privateMode,
        LocalKrunkStyle provides style,
    ) {
        MaterialTheme(colorScheme = scheme, content = content)
    }
}
