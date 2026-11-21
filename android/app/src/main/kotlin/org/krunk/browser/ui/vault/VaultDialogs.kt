package org.krunk.browser.ui.vault

import android.content.Context
import android.widget.Toast
import androidx.activity.compose.LocalActivity
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Fingerprint
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.krunk.browser.R
import org.krunk.browser.vault.VaultItemDetail
import org.krunk.browser.vault.VaultState

/** Whichever dialog the state holder has open. */
@Composable
internal fun VaultDialogHost(holder: VaultUiState, port: VaultPort, dialog: VaultDialog?) {
    when (dialog) {
        null -> Unit

        is VaultDialog.Entry -> EntryDialog(holder, dialog)

        is VaultDialog.Editor -> EditorDialog(holder, dialog)

        is VaultDialog.ConfirmDelete -> ConfirmDeleteDialog(holder, dialog.detail)

        is VaultDialog.ChangePassword -> ChangePasswordDialog(holder, dialog)

        VaultDialog.Settings -> VaultSettingsDialog(holder, port)

        is VaultDialog.ConfirmDestroy -> DestroyDialog(holder, dialog)

        VaultDialog.OfferBiometric -> {
            val activity = LocalActivity.current
            BiometricOfferDialog(
                onYes = {
                    holder.acceptBiometricOffer {
                        activity?.let { port.biometric.enable(it) } ?: error("no activity")
                    }
                },
                onNo = holder::dismissDialog,
            )
        }
    }
}

/** Asked once, right after a vault was made (here or in the sync setup). */
@Composable
internal fun BiometricOfferDialog(onYes: () -> Unit, onNo: () -> Unit) {
    AlertDialog(
        onDismissRequest = onNo,
        properties = SecureDialog,
        icon = { Icon(Icons.Filled.Fingerprint, contentDescription = null) },
        title = { Text(stringResource(R.string.vault_bio_offer_title)) },
        text = { Text(stringResource(R.string.vault_bio_offer_text)) },
        confirmButton = { TextButton(onClick = onYes) { Text(stringResource(R.string.vault_bio_offer_yes)) } },
        dismissButton = { TextButton(onClick = onNo) { Text(stringResource(R.string.vault_bio_offer_no)) } },
    )
}

/** The site as the list and the titles name it: `https://` is the rule and left out. */
internal fun siteLabel(origin: String): String = origin.removePrefix("https://")

private fun confirmCopy(context: Context, text: Int) = Toast.makeText(context, text, Toast.LENGTH_SHORT).show()

@Composable
private fun EntryDialog(holder: VaultUiState, entry: VaultDialog.Entry) {
    val context = LocalContext.current
    val detail = entry.detail
    val revealed = entry.revealed
    AlertDialog(
        onDismissRequest = holder::dismissDialog,
        properties = SecureDialog,
        title = { Text(siteLabel(detail.origin), maxLines = 2) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Field(stringResource(R.string.vault_field_site)) { Text(detail.origin) }
                Field(
                    stringResource(R.string.vault_field_username),
                    action = {
                        IconButton(
                            onClick = {
                                if (VaultClipboard.copyText(context, detail.username)) {
                                    confirmCopy(context, R.string.vault_copied)
                                }
                            },
                            enabled = detail.username.isNotEmpty(),
                        ) {
                            Icon(Icons.Filled.ContentCopy, stringResource(R.string.vault_copy_username))
                        }
                    },
                ) {
                    if (detail.username.isEmpty()) {
                        Text(
                            stringResource(R.string.vault_no_username),
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    } else {
                        Text(detail.username)
                    }
                }
                Field(
                    stringResource(R.string.vault_field_password),
                    action = {
                        TextButton(onClick = { if (revealed == null) holder.reveal() else holder.hide() }) {
                            Text(stringResource(if (revealed == null) R.string.vault_show else R.string.vault_hide))
                        }
                        IconButton(
                            onClick = {
                                holder.withPassword { password ->
                                    if (VaultClipboard.copyPassword(context, password)) {
                                        confirmCopy(context, R.string.vault_copied_password)
                                    }
                                }
                            },
                        ) {
                            Icon(Icons.Filled.ContentCopy, stringResource(R.string.vault_copy_password))
                        }
                    },
                ) {
                    if (revealed == null) {
                        val hidden = stringResource(R.string.vault_password_hidden)
                        Text(HIDDEN_DOTS, modifier = Modifier.semantics { contentDescription = hidden })
                    } else {
                        Text(revealed.value, fontFamily = FontFamily.Monospace)
                    }
                }
                if (detail.note.isNotEmpty()) {
                    Field(stringResource(R.string.vault_field_note)) { Text(detail.note) }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = holder::dismissDialog) { Text(stringResource(R.string.action_close)) }
        },
        dismissButton = {
            Row {
                TextButton(onClick = holder::askDelete) {
                    Text(stringResource(R.string.action_delete), color = MaterialTheme.colorScheme.error)
                }
                TextButton(onClick = holder::edit) { Text(stringResource(R.string.action_edit)) }
            }
        },
    )
}

/** A labelled value with its actions at the end of the line. */
@Composable
private fun Field(label: String, action: @Composable () -> Unit = {}, value: @Composable () -> Unit) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(
                label,
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            value()
        }
        action()
    }
}

@Composable
private fun EditorDialog(holder: VaultUiState, editor: VaultDialog.Editor) {
    // Plain remember on purpose: a typed password or note must not reach saved instance state.
    var site by remember(editor.id) { mutableStateOf(editor.origin) }
    var username by remember(editor.id) { mutableStateOf(editor.username) }
    var password by remember(editor.id) { mutableStateOf("") }
    var note by remember(editor.id) { mutableStateOf(editor.note) }
    val error = editor.error?.text()

    fun errorFor(field: EntryField): String? = error.takeIf { editor.errorField == field }

    AlertDialog(
        onDismissRequest = holder::backToEntry,
        properties = SecureDialog,
        title = {
            Text(stringResource(if (editor.id == null) R.string.vault_add_title else R.string.vault_edit_title))
        },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                OutlinedTextField(
                    value = site,
                    onValueChange = { site = it },
                    singleLine = true,
                    label = { Text(stringResource(R.string.vault_field_site)) },
                    placeholder = { Text(stringResource(R.string.vault_field_site_hint)) },
                    isError = errorFor(EntryField.SITE) != null,
                    supportingText = errorFor(EntryField.SITE)?.let { { Text(it) } },
                    keyboardOptions = KeyboardOptions(
                        keyboardType = KeyboardType.Uri,
                        autoCorrectEnabled = false,
                        imeAction = ImeAction.Next,
                    ),
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = username,
                    onValueChange = { username = it },
                    singleLine = true,
                    label = { Text(stringResource(R.string.vault_field_username)) },
                    isError = errorFor(EntryField.USERNAME) != null,
                    supportingText = errorFor(EntryField.USERNAME)?.let { { Text(it) } },
                    keyboardOptions = KeyboardOptions(
                        keyboardType = KeyboardType.Email,
                        autoCorrectEnabled = false,
                        capitalization = KeyboardCapitalization.None,
                        imeAction = ImeAction.Next,
                    ),
                    modifier = Modifier.fillMaxWidth(),
                )
                PasswordField(
                    value = password,
                    onChange = { password = it },
                    label = stringResource(
                        if (editor.id == null) R.string.vault_field_password else R.string.vault_field_password_keep,
                    ),
                    error = errorFor(EntryField.PASSWORD),
                )
                OutlinedTextField(
                    value = note,
                    onValueChange = { note = it },
                    label = { Text(stringResource(R.string.vault_field_note)) },
                    isError = errorFor(EntryField.NOTE) != null,
                    supportingText = errorFor(EntryField.NOTE)?.let { { Text(it) } },
                    keyboardOptions = KeyboardOptions(autoCorrectEnabled = false),
                    minLines = 2,
                    maxLines = 5,
                    modifier = Modifier.fillMaxWidth(),
                )
                if (error != null && editor.errorField == null) {
                    Text(error, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
                }
            }
        },
        confirmButton = {
            TextButton(onClick = { holder.save(site, username, password, note) }, enabled = !editor.busy) {
                Text(stringResource(R.string.action_save))
            }
        },
        dismissButton = {
            TextButton(onClick = holder::backToEntry) { Text(stringResource(R.string.action_cancel)) }
        },
    )
}

@Composable
private fun ConfirmDeleteDialog(holder: VaultUiState, detail: VaultItemDetail) {
    AlertDialog(
        onDismissRequest = holder::backToEntry,
        properties = SecureDialog,
        title = { Text(stringResource(R.string.vault_delete_title)) },
        text = {
            Text(
                if (detail.username.isEmpty()) {
                    stringResource(R.string.vault_delete_text_no_user, siteLabel(detail.origin))
                } else {
                    stringResource(R.string.vault_delete_text, siteLabel(detail.origin), detail.username)
                },
            )
        },
        confirmButton = {
            TextButton(onClick = holder::confirmDelete) {
                Text(stringResource(R.string.action_delete), color = MaterialTheme.colorScheme.error)
            }
        },
        dismissButton = {
            TextButton(onClick = holder::backToEntry) { Text(stringResource(R.string.action_cancel)) }
        },
    )
}

@Composable
private fun ChangePasswordDialog(holder: VaultUiState, dialog: VaultDialog.ChangePassword) {
    var old by remember { mutableStateOf("") }
    var new by remember { mutableStateOf("") }
    var repeat by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = { if (!dialog.busy) holder.dismissDialog() },
        properties = SecureDialog,
        title = { Text(stringResource(R.string.vault_change_password)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                PasswordField(old, {
                    old = it
                }, stringResource(R.string.vault_current_password), enabled = !dialog.busy)
                PasswordField(new, {
                    new = it
                }, stringResource(R.string.vault_new_vault_password), enabled = !dialog.busy)
                PasswordField(
                    value = repeat,
                    onChange = { repeat = it },
                    label = stringResource(R.string.vault_repeat_password),
                    error = dialog.error?.text(),
                    enabled = !dialog.busy,
                    imeAction = ImeAction.Done,
                    onIme = { holder.changePassword(old, new, repeat) },
                )
                if (dialog.busy) BusyLine(stringResource(R.string.vault_unlocking))
                Text(
                    stringResource(R.string.vault_no_recovery),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    stringResource(R.string.vault_change_note),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = { holder.changePassword(old, new, repeat) },
                enabled = !dialog.busy && old.isNotEmpty() && new.isNotEmpty(),
            ) { Text(stringResource(R.string.vault_change_password)) }
        },
        dismissButton = {
            TextButton(onClick = holder::dismissDialog, enabled = !dialog.busy) {
                Text(stringResource(R.string.action_cancel))
            }
        },
    )
}

@Composable
private fun VaultSettingsDialog(holder: VaultUiState, port: VaultPort) {
    val minutes by port.lockMinutes.collectAsStateWithLifecycle()
    val grace by port.backgroundGraceSeconds.collectAsStateWithLifecycle()
    val offerToSave by port.offerToSave.collectAsStateWithLifecycle()
    val suggestPasswords by port.suggestPasswords.collectAsStateWithLifecycle()
    val vault by port.state.collectAsStateWithLifecycle()
    val bioOn by port.biometric.enabled.collectAsStateWithLifecycle()
    val activity = LocalActivity.current
    AlertDialog(
        onDismissRequest = holder::dismissDialog,
        properties = SecureDialog,
        title = { Text(stringResource(R.string.vault_settings_title)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                SwitchRow(
                    stringResource(R.string.vault_offer_save),
                    null,
                    offerToSave,
                    onChange = holder::setOfferToSave,
                )
                SwitchRow(
                    stringResource(R.string.vault_suggest_passwords),
                    stringResource(R.string.vault_suggest_passwords_detail),
                    suggestPasswords,
                    onChange = holder::setSuggestPasswords,
                )
                HorizontalDivider()
                Text(
                    stringResource(R.string.vault_lock_timer),
                    style = MaterialTheme.typography.titleSmall,
                    modifier = Modifier.padding(top = 12.dp),
                )
                for (choice in LOCK_MINUTE_CHOICES) {
                    ChoiceRow(pluralStringResource(R.plurals.vault_minutes, choice, choice), choice == minutes) {
                        holder.setLockMinutes(choice)
                    }
                }
                Text(
                    stringResource(R.string.vault_grace_title),
                    style = MaterialTheme.typography.titleSmall,
                    modifier = Modifier.padding(top = 12.dp),
                )
                for (choice in BACKGROUND_GRACE_CHOICES) {
                    ChoiceRow(stringResource(graceLabel(choice)), choice == grace) {
                        holder.setBackgroundGraceSeconds(choice)
                    }
                }
                Text(
                    stringResource(R.string.vault_grace_help),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 4.dp, bottom = 12.dp),
                )
                // Fingerprint unlock wraps the open vault's key: there is nothing to switch
                // before a vault exists and is open.
                val row = if (vault == VaultState.UNLOCKED) bioRow(port.biometric.availability()) else BioRow.Hidden
                when (row) {
                    BioRow.Hidden -> Unit

                    BioRow.Switch -> {
                        HorizontalDivider()
                        SwitchRow(
                            stringResource(R.string.vault_bio_switch),
                            stringResource(R.string.vault_bio_switch_help),
                            bioOn,
                        ) { on ->
                            holder.setBiometric(on) {
                                activity?.let { port.biometric.enable(it) }
                                    ?: error("no activity")
                            }
                        }
                    }

                    is BioRow.Unavailable -> {
                        HorizontalDivider()
                        SwitchRow(
                            stringResource(R.string.vault_bio_switch),
                            row.reason.text(),
                            false,
                            enabled = false,
                        ) {
                        }
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = holder::dismissDialog) { Text(stringResource(R.string.action_close)) }
        },
    )
}

@Composable
private fun ChoiceRow(label: String, selected: Boolean, onPick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .selectable(selected = selected, role = Role.RadioButton, onClick = onPick)
            .padding(vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RadioButton(selected = selected, onClick = null)
        Spacer(Modifier.width(12.dp))
        Text(label)
    }
}

@Composable
private fun SwitchRow(
    title: String,
    summary: String?,
    checked: Boolean,
    enabled: Boolean = true,
    onChange: (Boolean) -> Unit,
) {
    Row(
        Modifier
            .fillMaxWidth()
            // A switch is "on" or "off" to a screen reader, not "selected".
            .toggleable(value = checked, enabled = enabled, role = Role.Switch, onValueChange = onChange)
            .padding(vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f).padding(end = 12.dp)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            if (summary != null) {
                Text(
                    summary,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        Switch(checked = checked, onCheckedChange = null, enabled = enabled)
    }
}

@Composable
private fun DestroyDialog(holder: VaultUiState, dialog: VaultDialog.ConfirmDestroy) {
    var typed by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = { if (!dialog.busy) holder.dismissDialog() },
        properties = SecureDialog,
        title = { Text(stringResource(R.string.vault_destroy_title)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(stringResource(R.string.vault_destroy_text, DESTROY_WORD))
                OutlinedTextField(
                    value = typed,
                    onValueChange = { typed = it },
                    singleLine = true,
                    enabled = !dialog.busy,
                    label = { Text(stringResource(R.string.vault_destroy_field, DESTROY_WORD)) },
                    isError = dialog.refused,
                    supportingText = if (dialog.refused) {
                        { Text(stringResource(R.string.vault_destroy_refused, DESTROY_WORD)) }
                    } else {
                        null
                    },
                    keyboardOptions = KeyboardOptions(
                        capitalization = KeyboardCapitalization.Characters,
                        autoCorrectEnabled = false,
                        imeAction = ImeAction.Done,
                    ),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
        confirmButton = {
            TextButton(onClick = { holder.destroy(typed) }, enabled = !dialog.busy && typed.isNotBlank()) {
                Text(stringResource(R.string.vault_destroy), color = MaterialTheme.colorScheme.error)
            }
        },
        dismissButton = {
            TextButton(onClick = holder::dismissDialog, enabled = !dialog.busy) {
                Text(stringResource(R.string.action_cancel))
            }
        },
    )
}

private const val HIDDEN_DOTS = "••••••••"
