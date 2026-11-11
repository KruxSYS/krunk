package org.krunk.browser.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import org.krunk.ffi.ThemeMode

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

private val LightScheme: ColorScheme = lightColorScheme(
    primary = KrunkColors.accentLight,
    onPrimary = KrunkColors.accentFgLight,
    primaryContainer = Color(0xFFDBEAFE),
    onPrimaryContainer = Color(0xFF1E3A8A),
    secondary = KrunkColors.fgMutedLight,
    onSecondary = Color.White,
    background = KrunkColors.bgLight,
    onBackground = KrunkColors.fgLight,
    surface = KrunkColors.bgLight,
    onSurface = KrunkColors.fgLight,
    surfaceVariant = KrunkColors.bgHoverLight,
    onSurfaceVariant = KrunkColors.fgMutedLight,
    surfaceContainer = KrunkColors.bgRaisedLight,
    surfaceContainerHigh = KrunkColors.bgHoverLight,
    surfaceContainerHighest = KrunkColors.borderLight,
    surfaceContainerLow = KrunkColors.bgRaisedLight,
    surfaceContainerLowest = Color.White,
    outline = KrunkColors.borderLight,
    outlineVariant = KrunkColors.bgHoverLight,
    error = KrunkColors.danger,
    onError = Color.White,
)

private val DarkScheme: ColorScheme = darkColorScheme(
    primary = KrunkColors.accentDark,
    onPrimary = KrunkColors.accentFgDark,
    primaryContainer = Color(0xFF1E3A8A),
    onPrimaryContainer = Color(0xFFDBEAFE),
    secondary = KrunkColors.fgMutedDark,
    onSecondary = KrunkColors.bgDark,
    background = KrunkColors.bgDark,
    onBackground = KrunkColors.fgDark,
    surface = KrunkColors.bgDark,
    onSurface = KrunkColors.fgDark,
    surfaceVariant = KrunkColors.bgHoverDark,
    onSurfaceVariant = KrunkColors.fgMutedDark,
    surfaceContainer = KrunkColors.bgRaisedDark,
    surfaceContainerHigh = KrunkColors.bgHoverDark,
    surfaceContainerHighest = KrunkColors.borderDark,
    surfaceContainerLow = KrunkColors.bgRaisedDark,
    surfaceContainerLowest = Color(0xFF17181B),
    outline = KrunkColors.borderDark,
    outlineVariant = KrunkColors.bgHoverDark,
    error = Color(0xFFF87171),
    onError = Color(0xFF450A0A),
)

/** Private tabs: the same dark scheme tinted with Krunk's private purple. */
private val PrivateScheme: ColorScheme = DarkScheme.copy(
    primary = Color(0xFFC4B5FD),
    onPrimary = Color(0xFF2E1065),
    primaryContainer = KrunkColors.privateAccent,
    onPrimaryContainer = Color(0xFFEDE9FE),
    background = Color(0xFF1B1526),
    surface = Color(0xFF1B1526),
    surfaceContainer = Color(0xFF2A2140),
    surfaceContainerLow = Color(0xFF2A2140),
    surfaceContainerHigh = KrunkColors.privateBgDark,
    surfaceVariant = KrunkColors.privateBgDark,
)

/** Whether the current theme is dark (for icons that are not Material colours). */
val LocalKrunkDark = staticCompositionLocalOf { false }
val LocalKrunkPrivate = staticCompositionLocalOf { false }

@Composable
fun isDarkTheme(mode: ThemeMode): Boolean = when (mode) {
    ThemeMode.SYSTEM -> isSystemInDarkTheme()
    ThemeMode.LIGHT -> false
    ThemeMode.DARK -> true
}

@Composable
fun KrunkTheme(
    mode: ThemeMode = ThemeMode.SYSTEM,
    privateMode: Boolean = false,
    content: @Composable () -> Unit,
) {
    val dark = isDarkTheme(mode)
    val scheme = when {
        privateMode -> PrivateScheme
        dark -> DarkScheme
        else -> LightScheme
    }
    CompositionLocalProvider(LocalKrunkDark provides (dark || privateMode), LocalKrunkPrivate provides privateMode) {
        MaterialTheme(colorScheme = scheme, content = content)
    }
}
