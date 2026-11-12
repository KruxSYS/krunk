package org.krunk.browser.ui.settings

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.krunk.browser.R
import org.krunk.browser.ui.components.ColorPickerDialog
import org.krunk.browser.ui.theme.CardMark
import org.krunk.browser.ui.theme.LocalKrunkDark
import org.krunk.browser.ui.theme.ThemeStyles
import org.krunk.browser.ui.theme.styleOf
import org.krunk.ffi.AccentSwatch
import org.krunk.ffi.ResolvedTheme
import org.krunk.ffi.ThemeAppearance
import org.krunk.ffi.ThemePreset
import org.krunk.ffi.accentSwatches
import org.krunk.ffi.themePresets

private const val PREVIEWS_PER_ROW = 2
private const val SWATCHES_PER_ROW = 4

/** The name of the style in effect: a preset's, or "Default" for a look that is none of them. */
@Composable
fun styleName(appearance: ThemeAppearance): String {
    val presets = remember { themePresets("", false) }
    return presets.firstOrNull { it.id == appearance.preset }?.label ?: stringResource(R.string.settings_style_default)
}

/** The accent's name: one of the offered colours by name, any other by its value. */
@Composable
fun accentName(appearance: ThemeAppearance): String {
    val swatches = remember { accentSwatches() }
    if (appearance.accent.isEmpty()) return swatches.first().name
    return swatches.firstOrNull { it.color == appearance.accent }?.name ?: appearance.accent
}

/**
 * Chooses one of Krunk's styles. Each is shown as a small drawing of the toolbar and two tab
 * cards in the colours it has right now: the current accent, light or dark as the app is.
 * A tap applies the style at once (the screen behind the dialog changes with it).
 */
@Composable
fun StylePickerDialog(current: ThemeAppearance, onPick: (String) -> Unit, onDismiss: () -> Unit) {
    val dark = LocalKrunkDark.current
    val presets = remember(current.accent, dark) { themePresets(current.accent, dark) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.settings_style)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                presets.chunked(PREVIEWS_PER_ROW).forEach { row ->
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        row.forEach { preset ->
                            StylePreview(
                                preset,
                                selected = preset.id == current.preset,
                                modifier = Modifier.weight(1f),
                            ) { onPick(preset.id) }
                        }
                        repeat(PREVIEWS_PER_ROW - row.size) { Spacer(Modifier.weight(1f)) }
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.settings_done)) } },
    )
}

/** One style: its drawing and its name, announced as a radio button by the name. */
@Composable
private fun StylePreview(preset: ThemePreset, selected: Boolean, modifier: Modifier, onClick: () -> Unit) {
    val frame = RoundedCornerShape(10.dp)
    Column(
        modifier
            .padding(vertical = 6.dp)
            .selectable(selected = selected, role = Role.RadioButton, onClick = onClick)
            .semantics { contentDescription = preset.label },
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(
            Modifier
                .fillMaxWidth()
                .aspectRatio(1.5f)
                .clip(frame)
                .border(
                    if (selected) 3.dp else 1.dp,
                    if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline,
                    frame,
                ),
        ) {
            ThemeDrawing(preset.theme)
        }
        Text(
            preset.label,
            style = MaterialTheme.typography.labelLarge,
            color = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(top = 4.dp),
        )
    }
}

/**
 * A theme in small: the toolbar with the address field, and two tab cards, the first one marked
 * as the active tab the way the style marks it. Drawn from the same resolved colours and shapes
 * the real chrome uses.
 */
@Composable
private fun ThemeDrawing(theme: ResolvedTheme) {
    val style = remember(theme) { styleOf(theme) }

    fun c(hex: String) = Color(ThemeStyles.argb(hex))
    val pillShape = RoundedCornerShape((ThemeStyles.pillCornerDp(theme.shape) / 2).dp)
    val cardShape = RoundedCornerShape((ThemeStyles.cardCornerDp(theme.shape) / 2).dp)
    Column(Modifier.fillMaxSize().background(c(theme.background))) {
        Row(
            Modifier.fillMaxWidth().height(26.dp).background(style.toolbar).padding(horizontal = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier.weight(1f).height(16.dp).clip(pillShape).background(style.pill).padding(horizontal = 6.dp),
                contentAlignment = Alignment.CenterStart,
            ) {
                Box(Modifier.width(34.dp).height(3.dp).background(style.pillText, CircleShape))
            }
            Spacer(Modifier.width(6.dp))
            Box(Modifier.size(9.dp).border(1.5.dp, style.toolbarText, RoundedCornerShape(2.dp)))
            Spacer(Modifier.width(5.dp))
            Box(Modifier.size(width = 3.dp, height = 9.dp).background(style.toolbarText, CircleShape))
        }
        Row(
            Modifier.fillMaxSize().padding(6.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            for (active in listOf(true, false)) {
                val mark = ThemeStyles.cardMark(theme.highlight, active)
                Column(
                    Modifier
                        .weight(1f)
                        .fillMaxHeight()
                        .clip(cardShape)
                        .background(c(theme.surfaceRaised))
                        .then(
                            if (mark ==
                                CardMark.OUTLINE
                            ) {
                                Modifier.border(1.5.dp, c(theme.primary), cardShape)
                            } else {
                                Modifier
                            },
                        ),
                ) {
                    Box(
                        Modifier
                            .fillMaxWidth()
                            .height(10.dp)
                            .background(if (mark == CardMark.FILL) style.accent else Color.Transparent)
                            .padding(horizontal = 5.dp),
                        contentAlignment = Alignment.CenterStart,
                    ) {
                        val line = when (mark) {
                            CardMark.FILL -> style.onAccent
                            CardMark.TITLE, CardMark.UNDERLINE -> c(theme.primary)
                            else -> c(theme.text)
                        }
                        Box(Modifier.width(22.dp).height(2.5.dp).background(line, CircleShape))
                    }
                    if (mark ==
                        CardMark.UNDERLINE
                    ) {
                        Box(Modifier.fillMaxWidth().height(2.dp).background(c(theme.primary)))
                    }
                    Box(Modifier.fillMaxSize().background(c(theme.surfaceLowest)))
                }
            }
        }
    }
}

/**
 * Chooses the accent colour: the offered colours by name, or any colour through the colour
 * chooser. A tap applies it at once.
 */
@Composable
fun AccentPickerDialog(current: ThemeAppearance, onPick: (String) -> Unit, onDismiss: () -> Unit) {
    val swatches = remember { accentSwatches() }
    val chosen = current.accent.ifEmpty { swatches.first().color }
    var custom by remember { mutableStateOf(false) }
    if (custom) {
        ColorPickerDialog(
            title = stringResource(R.string.settings_accent_custom),
            initial = chosen,
            swatches = swatches.map { it.color },
            onPick = {
                custom = false
                onPick(it)
            },
            onDismiss = { custom = false },
        )
        return
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.settings_accent)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                swatches.chunked(SWATCHES_PER_ROW).forEach { row ->
                    Row(Modifier.fillMaxWidth()) {
                        row.forEach { swatch ->
                            AccentDot(swatch, selected = swatch.color == chosen) { onPick(swatch.color) }
                        }
                    }
                }
                TextButton(onClick = { custom = true }, modifier = Modifier.padding(top = 8.dp)) {
                    Text(
                        if (swatches.any { it.color == chosen }) {
                            stringResource(R.string.settings_accent_custom)
                        } else {
                            stringResource(R.string.settings_accent_custom_now, chosen)
                        },
                    )
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.settings_done)) } },
    )
}

/** One offered accent: a 56 dp target around a 40 dp dot, announced by the colour's name. */
@Composable
private fun AccentDot(swatch: AccentSwatch, selected: Boolean, onClick: () -> Unit) {
    Box(
        Modifier
            .size(56.dp)
            .selectable(selected = selected, role = Role.RadioButton, onClick = onClick)
            .semantics { contentDescription = swatch.name },
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier
                .size(40.dp)
                .background(Color(ThemeStyles.argb(swatch.color)), CircleShape)
                .border(
                    if (selected) 3.dp else 1.dp,
                    if (selected) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.outline,
                    CircleShape,
                ),
        )
    }
}
