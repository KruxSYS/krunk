package org.krunk.browser.ui.browser

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.toMutableStateList
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import org.krunk.browser.R
import org.krunk.browser.engine.PagePrompt
import org.krunk.browser.engine.PermissionPrompt
import org.krunk.browser.ui.components.ColorPickerDialog
import org.krunk.browser.ui.components.HexColor

/** What a page may put into a dialog: bounded, so megabytes of text cannot stall the layout. */
private const val MAX_MESSAGE = 4000

private fun String.bounded(): String = if (length > MAX_MESSAGE) take(MAX_MESSAGE) + "…" else this

/**
 * The dialog for a [PagePrompt]. The title always names the site that asks (never Krunk), the
 * page's text is plain text, and Back or a tap outside dismisses without an answer. A file
 * field and a share go to Android's own picker and share sheet (see `PagePromptPickers.kt`).
 */
@Composable
fun PagePromptDialog(prompt: PagePrompt) {
    // A new prompt is a new dialog: nothing typed into the previous one carries over.
    key(prompt) {
        when (prompt) {
            is PagePrompt.Alert -> MessageDialog(prompt, prompt.message, onOk = prompt::ok, onCancel = null)

            is PagePrompt.Confirm -> MessageDialog(
                prompt,
                prompt.message,
                onOk = { prompt.answer(true) },
                onCancel = { prompt.answer(false) },
            )

            is PagePrompt.Text -> TextPromptDialog(prompt)

            is PagePrompt.SignIn -> SignInDialog(prompt)

            is PagePrompt.LoginFill -> LoginFillDialog(prompt)

            is PagePrompt.LoginSave -> LoginSaveDialog(prompt)

            is PagePrompt.PasswordSuggest -> PasswordSuggestDialog(prompt)

            is PagePrompt.Choice -> ChoiceDialog(prompt)

            is PagePrompt.File -> FilePromptUi(prompt)

            is PagePrompt.DateTime -> DateTimePromptDialog(prompt)

            is PagePrompt.Colour -> ColorPickerDialog(
                title = stringResource(R.string.prompt_colour_title, askingSite(prompt)),
                initial = prompt.value,
                swatches = HexColor.swatches(prompt.suggested, prompt.value),
                onPick = prompt::answer,
                onDismiss = prompt::dismiss,
            )

            is PagePrompt.LeavePage -> LeavePageDialog(prompt)

            is PagePrompt.Repost -> RepostDialog(prompt)

            is PagePrompt.Share -> SharePromptUi(prompt)

            is PermissionPrompt -> PermissionPromptDialog(prompt)
        }
    }
}

@Composable
private fun siteTitle(prompt: PagePrompt): String = if (prompt.site.isEmpty()) {
    stringResource(R.string.prompt_page_says)
} else {
    stringResource(R.string.prompt_site_says, prompt.site)
}

/** The page's text and, from the second dialog of a page on, the way out of a dialog loop. */
@Composable
private fun PromptBody(prompt: PagePrompt, message: String, extra: @Composable () -> Unit = {}) {
    Column(Modifier.heightIn(max = 360.dp).verticalScroll(rememberScrollState())) {
        if (message.isNotEmpty()) Text(message.bounded(), style = MaterialTheme.typography.bodyMedium)
        extra()
        if (prompt.canBlock) {
            TextButton(onClick = prompt::block, modifier = Modifier.padding(top = 8.dp)) {
                Text(stringResource(R.string.prompt_block_more))
            }
        }
    }
}

@Composable
private fun MessageDialog(prompt: PagePrompt, message: String, onOk: () -> Unit, onCancel: (() -> Unit)?) {
    AlertDialog(
        onDismissRequest = prompt::dismiss,
        title = { Text(siteTitle(prompt)) },
        text = { PromptBody(prompt, message) },
        confirmButton = { TextButton(onClick = onOk) { Text(stringResource(android.R.string.ok)) } },
        dismissButton = onCancel?.let {
            { TextButton(onClick = it) { Text(stringResource(android.R.string.cancel)) } }
        },
    )
}

@Composable
private fun TextPromptDialog(prompt: PagePrompt.Text) {
    var text by remember { mutableStateOf(prompt.default.take(MAX_MESSAGE)) }
    AlertDialog(
        onDismissRequest = prompt::dismiss,
        title = { Text(siteTitle(prompt)) },
        text = {
            PromptBody(prompt, prompt.message) {
                OutlinedTextField(
                    value = text,
                    onValueChange = { text = it },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().padding(top = 12.dp),
                )
            }
        },
        confirmButton = {
            TextButton(onClick = { prompt.answer(text) }) { Text(stringResource(android.R.string.ok)) }
        },
        dismissButton = {
            TextButton(onClick = prompt::dismiss) { Text(stringResource(android.R.string.cancel)) }
        },
    )
}

/** A `<select>`: one tap picks and closes, or check boxes with OK when several may be chosen. */
@Composable
private fun ChoiceDialog(prompt: PagePrompt.Choice) {
    val chosen = remember { prompt.options.filter { it.selected }.map { it.id }.toMutableStateList() }
    AlertDialog(
        onDismissRequest = prompt::dismiss,
        title = { Text(prompt.message.bounded().ifEmpty { stringResource(R.string.prompt_choose) }) },
        text = {
            LazyColumn(Modifier.heightIn(max = 420.dp)) {
                itemsIndexed(prompt.options) { _, option ->
                    when {
                        option.separator -> HorizontalDivider(Modifier.padding(vertical = 4.dp))

                        option.header -> Text(
                            option.label.bounded(),
                            style = MaterialTheme.typography.labelLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(top = 12.dp, bottom = 4.dp),
                        )

                        else -> ChoiceRow(
                            option = option,
                            multiple = prompt.multiple,
                            checked = option.id in chosen,
                            onClick = {
                                if (prompt.multiple) {
                                    if (!chosen.remove(option.id)) chosen += option.id
                                } else {
                                    prompt.answer(listOf(option.id))
                                }
                            },
                        )
                    }
                }
            }
        },
        confirmButton = {
            if (prompt.multiple) {
                TextButton(onClick = { prompt.answer(chosen.toList()) }) { Text(stringResource(android.R.string.ok)) }
            }
        },
        dismissButton = {
            TextButton(onClick = prompt::dismiss) { Text(stringResource(android.R.string.cancel)) }
        },
    )
}

@Composable
private fun ChoiceRow(option: PagePrompt.Option, multiple: Boolean, checked: Boolean, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .clickable(
                enabled = !option.disabled,
                role = if (multiple) Role.Checkbox else Role.RadioButton,
                onClick = onClick,
            )
            .padding(start = if (option.indented) 16.dp else 0.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (multiple) {
            Checkbox(checked = checked, onCheckedChange = null, enabled = !option.disabled)
        } else {
            RadioButton(selected = checked, onClick = null, enabled = !option.disabled)
        }
        Spacer(Modifier.width(12.dp))
        Text(
            option.label.bounded(),
            style = MaterialTheme.typography.bodyLarge,
            color = if (option.disabled) {
                MaterialTheme.colorScheme.onSurfaceVariant
            } else {
                MaterialTheme.colorScheme.onSurface
            },
        )
    }
}
