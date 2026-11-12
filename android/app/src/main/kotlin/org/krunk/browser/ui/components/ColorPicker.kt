package org.krunk.browser.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import org.krunk.browser.R

private const val SWATCHES_PER_ROW = 5

/**
 * Chooses a colour: [swatches] to tap and a field for any `#rrggbb`. [onPick] gets a valid
 * colour once, on "OK"; Back, a tap outside and "Cancel" call only [onDismiss].
 */
@Composable
fun ColorPickerDialog(
    title: String,
    initial: String,
    swatches: List<String>,
    onPick: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var text by remember { mutableStateOf(HexColor.parse(initial) ?: "") }
    val chosen = HexColor.parse(text)
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                swatches.chunked(SWATCHES_PER_ROW).forEach { row ->
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.Start) {
                        row.forEach { colour -> Swatch(colour, selected = colour == chosen) { text = colour } }
                    }
                }
                Row(Modifier.padding(top = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                    Box(
                        Modifier
                            .size(40.dp)
                            .background(chosen.toColor() ?: Color.Transparent, CircleShape)
                            .border(1.dp, MaterialTheme.colorScheme.outline, CircleShape),
                    )
                    OutlinedTextField(
                        value = text,
                        onValueChange = { text = it.take(MAX_INPUT) },
                        singleLine = true,
                        isError = chosen == null,
                        label = { Text(stringResource(R.string.colour_hex_label)) },
                        keyboardOptions = KeyboardOptions(
                            capitalization = KeyboardCapitalization.None,
                            autoCorrectEnabled = false,
                        ),
                        modifier = Modifier.padding(start = 12.dp).fillMaxWidth(),
                    )
                }
            }
        },
        confirmButton = {
            TextButton(enabled = chosen != null, onClick = { chosen?.let(onPick) }) {
                Text(stringResource(android.R.string.ok))
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(android.R.string.cancel)) } },
    )
}

/** One colour to tap: a 48 dp target around a 36 dp dot; announced by its value, selected or not. */
@Composable
private fun Swatch(colour: String, selected: Boolean, onClick: () -> Unit) {
    Box(
        Modifier
            .size(48.dp)
            .selectable(selected = selected, role = Role.RadioButton, onClick = onClick)
            .semantics { contentDescription = colour },
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier
                .size(36.dp)
                .background(colour.toColor() ?: Color.Transparent, CircleShape)
                .border(
                    if (selected) 3.dp else 1.dp,
                    if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline,
                    CircleShape,
                ),
        )
    }
}

private fun String?.toColor(): Color? = HexColor.argb(this)?.let { Color(it) }

private const val MAX_INPUT = 9
