package org.krunk.browser.ui.browser

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.krunk.browser.R
import org.krunk.browser.engine.PagePrompt
import org.krunk.browser.engine.logins.HttpAuthOffer
import org.krunk.browser.engine.logins.LoginPolicy
import org.krunk.browser.engine.logins.LoginVault
import org.krunk.browser.engine.originOf
import org.krunk.browser.ui.vault.SecureDialog
import org.krunk.browser.vault.SaveDecision
import org.krunk.browser.vault.VaultFailure
import org.krunk.browser.vault.VaultMatch
import org.krunk.browser.vault.VaultSecret

/**
 * What the sign-in dialogs need from the app: the vault, the unlock dialog, the way to the vault
 * screen. Null (no vault built in) shows the dialogs without any vault part.
 */
class LoginUiHost(
    val vault: LoginVault,
    val now: () -> Long,
    /** Opens the vault screen ("Create vault"). */
    val openVault: () -> Unit,
    /** Keeps a login for the vault the user is about to create. */
    val keepForNewVault: (origin: String, secret: VaultSecret) -> Unit,
    /** The vault's unlock dialog. */
    val unlockDialog: @Composable (onUnlocked: () -> Unit, onDismiss: () -> Unit) -> Unit,
    /** Marks the window secure while composed (no screenshots, blank in Recents). */
    val secure: @Composable () -> Unit,
)

val LocalLoginUi = staticCompositionLocalOf<LoginUiHost?> { null }

private enum class FillStep { WARN, UNLOCK, LIST }

/**
 * The picker for a tapped sign-in field. Nothing of the vault is shown before the user has
 * confirmed the warning of an unencrypted page and unlocked the vault.
 */
@Composable
fun LoginFillDialog(prompt: PagePrompt.LoginFill) {
    val host = LocalLoginUi.current
    if (host == null) {
        LaunchedEffect(prompt) { prompt.dismiss() }
        return
    }
    host.secure()
    var step by remember {
        mutableStateOf(
            when {
                prompt.warnInsecure -> FillStep.WARN
                host.vault.state() != LoginVault.State.UNLOCKED -> FillStep.UNLOCK
                else -> FillStep.LIST
            },
        )
    }
    // A lock while the picker is open: what it showed of the vault goes, the unlock is asked again.
    LaunchedEffect(prompt) { host.vault.locked.collect { if (step == FillStep.LIST) step = FillStep.UNLOCK } }
    val afterWarn = { step = if (host.vault.state() == LoginVault.State.UNLOCKED) FillStep.LIST else FillStep.UNLOCK }
    when (step) {
        FillStep.WARN -> AlertDialog(
            properties = SecureDialog,
            onDismissRequest = prompt::dismiss,
            title = { Text(stringResource(R.string.login_insecure_title)) },
            text = { Text(stringResource(R.string.login_insecure_fill, prompt.site)) },
            confirmButton = {
                TextButton(onClick = afterWarn) { Text(stringResource(R.string.login_insecure_fill_anyway)) }
            },
            dismissButton = {
                TextButton(onClick = prompt::dismiss) { Text(stringResource(android.R.string.cancel)) }
            },
        )

        FillStep.UNLOCK -> host.unlockDialog({ step = FillStep.LIST }, prompt::dismiss)

        FillStep.LIST -> FillList(prompt, host, onLocked = { step = FillStep.UNLOCK })
    }
}

@Composable
private fun FillList(prompt: PagePrompt.LoginFill, host: LoginUiHost, onLocked: () -> Unit) {
    val scope = rememberCoroutineScope()
    var matches by remember { mutableStateOf<List<VaultMatch>?>(null) }
    var failed by remember { mutableStateOf(false) }
    // The rows take a tap only after a moment: a dialog that appears under a finger already on
    // its way down must not turn that tap into a choice.
    var armed by remember { mutableStateOf(false) }
    LaunchedEffect(prompt) {
        delay(ROW_DELAY_MS)
        armed = true
    }
    LaunchedEffect(prompt) {
        try {
            matches = host.vault.use { it.matchesFor(prompt.pageUrl) }
        } catch (e: VaultFailure.Locked) {
            onLocked()
        } catch (e: VaultFailure) {
            matches = emptyList()
        }
    }
    val pageOrigin = originOf(prompt.pageUrl)
    AlertDialog(
        properties = SecureDialog,
        onDismissRequest = prompt::dismiss,
        title = { Text(stringResource(R.string.login_fill_title, prompt.site)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                val list = matches
                when {
                    list == null -> CircularProgressIndicator()

                    list.isEmpty() -> Text(stringResource(R.string.login_fill_none))

                    else -> {
                        Text(stringResource(R.string.login_fill_choose), style = MaterialTheme.typography.bodyMedium)
                        list.forEach { m ->
                            Column(
                                Modifier
                                    .fillMaxWidth()
                                    .clickable(enabled = armed) {
                                        scope.launch {
                                            try {
                                                prompt.pick(host.vault.use { it.secretForFill(m.id, prompt.pageUrl) })
                                            } catch (e: VaultFailure.Locked) {
                                                onLocked()
                                            } catch (e: VaultFailure) {
                                                failed = true
                                            }
                                        }
                                    }
                                    .padding(vertical = 12.dp),
                            ) {
                                Text(
                                    m.username.ifEmpty { stringResource(R.string.login_fill_no_username) },
                                    style = MaterialTheme.typography.bodyLarge,
                                )
                                if (m.origin != pageOrigin) {
                                    Text(
                                        stringResource(R.string.login_fill_other_site, m.origin.substringAfter("://")),
                                        style = MaterialTheme.typography.bodySmall,
                                    )
                                }
                            }
                        }
                    }
                }
                if (failed) {
                    Text(
                        stringResource(R.string.login_fill_failed),
                        color = MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
        },
        confirmButton = {},
        dismissButton = {
            TextButton(onClick = prompt::dismiss) { Text(stringResource(android.R.string.cancel)) }
        },
    )
}

private const val ROW_DELAY_MS = 500L

/**
 * "Use a strong password?" for a tapped new-password field (Android 0.13.0). The password is
 * hidden until "Show"; "Use password" has the engine fill it in. With saved logins for the page,
 * "Saved sign-ins" switches to the usual picker for the same field.
 */
@Composable
fun PasswordSuggestDialog(prompt: PagePrompt.PasswordSuggest) {
    val host = LocalLoginUi.current
    if (host == null) {
        LaunchedEffect(prompt) { prompt.dismiss() }
        return
    }
    host.secure()
    var showFill by remember { mutableStateOf(false) }
    val fill = prompt.fill
    if (showFill && fill != null) {
        LoginFillDialog(fill)
        return
    }
    var shown by remember { mutableStateOf(false) }
    // As with the picker: a dialog appearing under a finger must not take that tap as "Use".
    var armed by remember { mutableStateOf(false) }
    LaunchedEffect(prompt) {
        delay(ROW_DELAY_MS)
        armed = true
    }
    AlertDialog(
        properties = SecureDialog,
        onDismissRequest = prompt::dismiss,
        title = { Text(stringResource(R.string.login_suggest_title)) },
        text = {
            Column {
                Text(stringResource(R.string.login_suggest_text, prompt.site))
                Row(Modifier.padding(top = 12.dp)) {
                    Text(
                        if (shown) prompt.password() else "\u2022".repeat(SUGGEST_DOTS),
                        style = MaterialTheme.typography.bodyLarge.copy(fontFamily = FontFamily.Monospace),
                        modifier = Modifier.weight(1f).padding(top = 12.dp),
                    )
                    TextButton(onClick = { shown = !shown }) {
                        Text(stringResource(if (shown) R.string.login_suggest_hide else R.string.login_suggest_show))
                    }
                }
                if (fill != null) {
                    TextButton(onClick = { showFill = true }, modifier = Modifier.padding(top = 4.dp)) {
                        Text(stringResource(R.string.login_suggest_saved))
                    }
                }
            }
        },
        confirmButton = {
            TextButton(enabled = armed, onClick = prompt::use) { Text(stringResource(R.string.login_suggest_use)) }
        },
        dismissButton = {
            TextButton(onClick = prompt::dismiss) { Text(stringResource(R.string.login_not_now)) }
        },
    )
}

private const val SUGGEST_DOTS = 12

private enum class SaveStep { ASK, WARN, UNLOCK, SAVING }

/** "Save this sign-in?" after a form or an HTTP sign-in was sent. The password is never shown. */
@Composable
fun LoginSaveDialog(prompt: PagePrompt.LoginSave) {
    val host = LocalLoginUi.current
    if (host == null) {
        LaunchedEffect(prompt) { prompt.dismiss() }
        return
    }
    val scope = rememberCoroutineScope()
    var step by remember { mutableStateOf(SaveStep.ASK) }
    var failed by remember { mutableStateOf(false) }

    fun store() {
        step = SaveStep.SAVING
        scope.launch {
            try {
                val login = prompt.login()
                host.vault.use { b ->
                    // Asked again here: the vault may have been locked when the offer was made.
                    if (b.saveDecision(prompt.origin, login.username, login.password) != SaveDecision.Unchanged) {
                        b.saveLogin(prompt.origin, login.username, login.password, host.now())
                    }
                }
                prompt.close()
            } catch (e: VaultFailure.Locked) {
                step = SaveStep.UNLOCK
            } catch (e: VaultFailure) {
                failed = true
                step = SaveStep.ASK
            }
        }
    }

    fun confirmed() {
        when {
            prompt.kind == LoginPolicy.SaveKind.NO_VAULT -> {
                host.keepForNewVault(prompt.origin, prompt.login())
                prompt.close()
                host.openVault()
            }

            host.vault.state() != LoginVault.State.UNLOCKED -> step = SaveStep.UNLOCK

            else -> store()
        }
    }

    when (step) {
        SaveStep.UNLOCK -> {
            host.secure()
            host.unlockDialog({ store() }, { step = SaveStep.ASK })
        }

        SaveStep.WARN -> AlertDialog(
            properties = SecureDialog,
            onDismissRequest = { step = SaveStep.ASK },
            title = { Text(stringResource(R.string.login_insecure_title)) },
            text = { Text(stringResource(R.string.login_insecure_save, prompt.site)) },
            confirmButton = {
                TextButton(onClick = { confirmed() }) { Text(stringResource(R.string.login_insecure_save_anyway)) }
            },
            dismissButton = {
                TextButton(onClick = prompt::dismiss) { Text(stringResource(android.R.string.cancel)) }
            },
        )

        SaveStep.ASK, SaveStep.SAVING -> AlertDialog(
            properties = SecureDialog,
            onDismissRequest = prompt::dismiss,
            title = {
                Text(
                    stringResource(
                        when {
                            prompt.kind == LoginPolicy.SaveKind.UPDATE -> R.string.login_update_title
                            prompt.newPassword -> R.string.login_save_new_password_title
                            else -> R.string.login_save_title
                        },
                    ),
                )
            },
            text = {
                Column {
                    Text(
                        if (prompt.username.isEmpty()) {
                            stringResource(R.string.login_save_text_no_user, prompt.site)
                        } else {
                            stringResource(R.string.login_save_text, prompt.username, prompt.site)
                        },
                    )
                    when (prompt.kind) {
                        LoginPolicy.SaveKind.NO_VAULT -> Text(
                            stringResource(R.string.login_save_no_vault),
                            style = MaterialTheme.typography.bodySmall,
                            modifier = Modifier.padding(top = 8.dp),
                        )

                        LoginPolicy.SaveKind.LOCKED -> Text(
                            stringResource(R.string.login_save_locked),
                            style = MaterialTheme.typography.bodySmall,
                            modifier = Modifier.padding(top = 8.dp),
                        )

                        else -> Unit
                    }
                    if (failed) {
                        Text(
                            stringResource(R.string.login_save_failed),
                            color = MaterialTheme.colorScheme.error,
                            style = MaterialTheme.typography.bodySmall,
                            modifier = Modifier.padding(top = 8.dp),
                        )
                    }
                    Row(Modifier.padding(top = 8.dp)) {
                        TextButton(
                            onClick = {
                                host.vault.addNeverSave(prompt.origin)
                                prompt.close()
                            },
                        ) { Text(stringResource(R.string.login_never)) }
                        if (prompt.kind == LoginPolicy.SaveKind.NO_VAULT) {
                            TextButton(
                                onClick = {
                                    host.vault.setOffersEnabled(false)
                                    prompt.close()
                                },
                            ) { Text(stringResource(R.string.login_dont_ask)) }
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(
                    enabled = step == SaveStep.ASK,
                    onClick = { if (prompt.warnInsecure) step = SaveStep.WARN else confirmed() },
                ) {
                    Text(
                        stringResource(
                            when (prompt.kind) {
                                LoginPolicy.SaveKind.NO_VAULT -> R.string.login_create_vault
                                LoginPolicy.SaveKind.UPDATE -> R.string.login_update
                                else -> R.string.login_save
                            },
                        ),
                    )
                }
            },
            dismissButton = {
                TextButton(onClick = prompt::dismiss) { Text(stringResource(R.string.login_not_now)) }
            },
        )
    }
}

/** What a page may put into a dialog: bounded, so megabytes of text cannot stall the layout. */
private const val MAX_AUTH_MESSAGE = 4000

/**
 * HTTP authentication. Since 0.3.0 with the vault's logins for the server that asks: listed when
 * the vault is open, behind "Unlock vault" when it is locked, and in a private tab only behind
 * the explicit "Fill from vault". A saved login on an unencrypted connection needs a second
 * confirmation. What the user picks lands in the fields; nothing is sent before "Sign in".
 */
@Composable
internal fun SignInDialog(prompt: PagePrompt.SignIn) {
    val host = LocalLoginUi.current
    val scope = rememberCoroutineScope()
    var user by remember { mutableStateOf(prompt.username) }
    var password by remember { mutableStateOf("") }
    // What of the vault is shown now; moves from UNLOCK/EXPLICIT to LIST by the user's tap.
    var saved by remember { mutableStateOf(if (host == null) HttpAuthOffer.Saved.NONE else prompt.saved) }
    var unlocking by remember { mutableStateOf(false) }
    var matches by remember { mutableStateOf<List<VaultMatch>>(emptyList()) }
    var confirmInsecure by remember { mutableStateOf<VaultMatch?>(null) }

    // The password in the field came from the vault (not typed): a lock takes it out again.
    var fromVault by remember { mutableStateOf(false) }
    if (host != null) {
        LaunchedEffect(prompt) {
            host.vault.locked.collect {
                matches = emptyList()
                confirmInsecure = null
                if (fromVault) {
                    password = ""
                    fromVault = false
                }
                saved = HttpAuthOffer.afterLock(saved, prompt.private)
            }
        }
    }

    fun use(m: VaultMatch) {
        val h = host ?: return
        scope.launch {
            try {
                val secret = h.vault.use { it.secretForFill(m.id, prompt.authUrl) }
                if (!prompt.onlyPassword) user = secret.username
                password = secret.password
                fromVault = true
            } catch (e: VaultFailure) {
                saved = HttpAuthOffer.Saved.NONE
            }
        }
    }

    if (host != null && saved == HttpAuthOffer.Saved.LIST) {
        LaunchedEffect(prompt, saved) {
            matches = try {
                host.vault.use { it.forHttpAuth(prompt.authUrl) }
            } catch (e: VaultFailure) {
                emptyList()
            }
        }
    }
    if (host != null && (unlocking || password.isNotEmpty() || matches.isNotEmpty())) host.secure()

    if (host != null && unlocking) {
        host.unlockDialog(
            {
                unlocking = false
                saved = HttpAuthOffer.Saved.LIST
            },
            { unlocking = false },
        )
        return
    }
    confirmInsecure?.let { m ->
        AlertDialog(
            properties = SecureDialog,
            onDismissRequest = { confirmInsecure = null },
            title = { Text(stringResource(R.string.login_insecure_title)) },
            text = { Text(stringResource(R.string.login_insecure_auth)) },
            confirmButton = {
                TextButton(
                    onClick = {
                        confirmInsecure = null
                        use(m)
                    },
                ) { Text(stringResource(R.string.login_insecure_use_anyway)) }
            },
            dismissButton = {
                TextButton(onClick = { confirmInsecure = null }) { Text(stringResource(android.R.string.cancel)) }
            },
        )
        return
    }
    AlertDialog(
        properties = SecureDialog,
        onDismissRequest = prompt::dismiss,
        title = { Text(stringResource(R.string.prompt_sign_in_title, prompt.site)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                if (prompt.message.isNotEmpty()) {
                    Text(
                        prompt.message.let {
                            if (it.length >
                                MAX_AUTH_MESSAGE
                            ) {
                                it.take(MAX_AUTH_MESSAGE) + "…"
                            } else {
                                it
                            }
                        },
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
                if (prompt.insecure) {
                    Text(
                        stringResource(R.string.prompt_sign_in_insecure),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error,
                        modifier = Modifier.padding(top = 8.dp),
                    )
                }
                when (saved) {
                    HttpAuthOffer.Saved.NONE -> Unit

                    HttpAuthOffer.Saved.UNLOCK -> TextButton(onClick = { unlocking = true }) {
                        Text(stringResource(R.string.login_auth_unlock))
                    }

                    HttpAuthOffer.Saved.EXPLICIT -> TextButton(
                        onClick = {
                            if (host?.vault?.state() == LoginVault.State.UNLOCKED) {
                                saved = HttpAuthOffer.Saved.LIST
                            } else {
                                unlocking = true
                            }
                        },
                    ) { Text(stringResource(R.string.login_auth_from_vault)) }

                    HttpAuthOffer.Saved.LIST -> if (matches.isNotEmpty()) {
                        Text(
                            stringResource(R.string.login_auth_saved),
                            style = MaterialTheme.typography.labelMedium,
                            modifier = Modifier.padding(top = 8.dp),
                        )
                        matches.forEach { m ->
                            TextButton(onClick = { if (prompt.insecure) confirmInsecure = m else use(m) }) {
                                Text(
                                    stringResource(
                                        R.string.login_auth_use,
                                        m.username.ifEmpty { stringResource(R.string.login_fill_no_username) },
                                    ),
                                )
                            }
                        }
                    }
                }
                if (!prompt.onlyPassword) {
                    OutlinedTextField(
                        value = user,
                        onValueChange = { user = it },
                        singleLine = true,
                        label = { Text(stringResource(R.string.prompt_username)) },
                        keyboardOptions = KeyboardOptions(autoCorrectEnabled = false),
                        modifier = Modifier.fillMaxWidth().padding(top = 12.dp),
                    )
                }
                OutlinedTextField(
                    value = password,
                    onValueChange = {
                        password = it
                        fromVault = false
                    },
                    singleLine = true,
                    label = { Text(stringResource(R.string.prompt_password)) },
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false),
                    modifier = Modifier.fillMaxWidth().padding(top = 12.dp),
                )
            }
        },
        confirmButton = {
            TextButton(onClick = { prompt.answer(user, password) }) {
                Text(stringResource(R.string.prompt_sign_in))
            }
        },
        dismissButton = {
            TextButton(onClick = prompt::dismiss) { Text(stringResource(android.R.string.cancel)) }
        },
    )
}
