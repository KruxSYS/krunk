package org.krunk.browser.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchColors
import androidx.compose.material3.SwitchDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier

/**
 * Switch colours with a visible "off" thumb. Material's defaults draw the off thumb and border
 * in `outline` on a `surfaceContainerHighest` track, and Krunk's schemes map both to the
 * theme's border colour, so the thumb vanished. The muted text colour reads on the
 * background in every light and dark theme (`ffi/src/theme.rs` checks it).
 */
@Composable
fun krunkSwitchColors(): SwitchColors {
    val c = MaterialTheme.colorScheme
    return SwitchDefaults.colors(
        uncheckedThumbColor = c.onSurfaceVariant,
        uncheckedTrackColor = c.surfaceContainerHighest,
        uncheckedBorderColor = c.onSurfaceVariant,
        uncheckedIconColor = c.surfaceContainerHighest,
    )
}

/** Material's [Switch] with [krunkSwitchColors]: use it for every switch in the app. */
@Composable
fun KrunkSwitch(
    checked: Boolean,
    onCheckedChange: ((Boolean) -> Unit)?,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    Switch(
        checked = checked,
        onCheckedChange = onCheckedChange,
        modifier = modifier,
        enabled = enabled,
        colors = krunkSwitchColors(),
    )
}
