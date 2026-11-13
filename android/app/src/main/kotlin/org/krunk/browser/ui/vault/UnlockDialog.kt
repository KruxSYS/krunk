package org.krunk.browser.ui.vault

import androidx.activity.compose.LocalActivity
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Fingerprint
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.VisibilityOff
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.SecureFlagPolicy
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.krunk.browser.R
import org.krunk.browser.vault.VaultState

/** The sentence of a [VaultText]. */
@Composable
fun VaultText.text(): String = if (arg != null) stringResource(id, arg) else stringResource(id)

/**
 * Every vault dialog is its own window: it is marked secure itself, whatever the activity's
 * window says at the moment it opens.
 */
internal val SecureDialog = DialogProperties(securePolicy = SecureFlagPolicy.SecureOn)

/**
 * Asks for the vault password (or the fingerprint, when that is switched on) wherever the vault
 * is needed: the screen, a fill, a save. [reason] says what for ("To fill in your login for
 * example.com"); null for none. [onUnlocked] runs once the vault is open, also when it was open
 * already; the caller then takes the dialog away. The typed password lives in this dialog only.
 */
@Composable
fun UnlockVaultDialog(port: VaultPort, reason: String?, onUnlocked: () -> Unit, onDismiss: () -> Unit) {
    SecureVaultSurface()
    val scope = rememberCoroutineScope()
    val unlock = remember(port) { UnlockUiState(port, scope) }
    val model by unlock.model.collectAsStateWithLifecycle()
    val vault by port.state.collectAsStateWithLifecycle()
    val unlocked by rememberUpdatedState(onUnlocked)
    LaunchedEffect(vault) { if (vault == VaultState.UNLOCKED) unlocked() }

    var password by remember { mutableStateOf("") }
    val activity = LocalActivity.current
    val canUnlock = vault == VaultState.LOCKED
    AlertDialog(
        onDismissRequest = onDismiss,
        properties = SecureDialog,
        title = { Text(stringResource(R.string.vault_unlock_title)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                if (reason != null) Text(reason, style = MaterialTheme.typography.bodyMedium)
                when (vault) {
                    VaultState.NONE -> Text(stringResource(R.string.vault_unlock_no_vault))

                    VaultState.KEY_LOST -> Text(
                        stringResource(R.string.vault_error_key_lost),
                        color = MaterialTheme.colorScheme.error,
                    )

                    VaultState.LOCKED, VaultState.UNLOCKED -> UnlockFields(
                        model = model,
                        password = password,
                        onPassword = { password = it },
                        onSubmit = { unlock.submit(password) },
                        onBiometric = { activity?.let { a -> unlock.biometric { port.biometric.unlock(a) } } },
                    )
                }
            }
        },
        confirmButton = {
            if (canUnlock) {
                TextButton(onClick = { unlock.submit(password) }, enabled = model.canSubmit && password.isNotEmpty()) {
                    Text(stringResource(R.string.vault_unlock))
                }
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

/**
 * The unlock form without its frame: why fingerprint unlock is off (if it was switched off),
 * the password field with the error or the countdown under it, the busy line, the fingerprint
 * button, and the note that nothing recovers a forgotten password.
 */
@Composable
internal fun UnlockFields(
    model: UnlockUiModel,
    password: String,
    onPassword: (String) -> Unit,
    onSubmit: () -> Unit,
    onBiometric: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        bioOffText(model.offReason)?.let {
            Text(
                it.text(),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        PasswordField(
            value = password,
            onChange = onPassword,
            label = stringResource(R.string.vault_password),
            error = (model.waitText ?: model.error)?.text(),
            enabled = !model.busy,
            imeAction = ImeAction.Done,
            onIme = onSubmit,
        )
        if (model.busy) BusyLine(stringResource(R.string.vault_unlocking))
        if (model.biometricOffered) {
            OutlinedButton(onClick = onBiometric, enabled = !model.busy) {
                Icon(Icons.Filled.Fingerprint, contentDescription = null, modifier = Modifier.size(18.dp))
                Text(stringResource(R.string.vault_use_fingerprint), modifier = Modifier.padding(start = 8.dp))
            }
        }
        Text(
            stringResource(R.string.vault_forgotten),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
internal fun BusyLine(text: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
        Text(text, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(start = 12.dp))
    }
}

/**
 * A password field: dots until the eye is tapped, a password keyboard, no autocorrect (so the
 * keyboard does not learn it). Whether it is shown is not kept across a recreation.
 */
@Composable
internal fun PasswordField(
    value: String,
    onChange: (String) -> Unit,
    label: String,
    modifier: Modifier = Modifier,
    error: String? = null,
    enabled: Boolean = true,
    imeAction: ImeAction = ImeAction.Next,
    onIme: (() -> Unit)? = null,
) {
    var visible by remember { mutableStateOf(false) }
    OutlinedTextField(
        value = value,
        onValueChange = onChange,
        singleLine = true,
        enabled = enabled,
        label = { Text(label) },
        isError = error != null,
        supportingText = error?.let { { Text(it) } },
        visualTransformation = if (visible) VisualTransformation.None else PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(
            keyboardType = KeyboardType.Password,
            autoCorrectEnabled = false,
            imeAction = imeAction,
        ),
        keyboardActions = KeyboardActions(onDone = onIme?.let { { it() } }),
        trailingIcon = {
            IconButton(onClick = { visible = !visible }) {
                Icon(
                    if (visible) Icons.Filled.VisibilityOff else Icons.Filled.Visibility,
                    contentDescription = stringResource(
                        if (visible) R.string.vault_hide_password else R.string.vault_show_password,
                    ),
                )
            }
        },
        modifier = modifier.fillMaxWidth(),
    )
}
