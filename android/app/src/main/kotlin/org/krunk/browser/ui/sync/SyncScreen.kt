package org.krunk.browser.ui.sync

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import java.text.DateFormat
import java.util.Date
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.krunk.browser.KrunkApp
import org.krunk.browser.R
import org.krunk.browser.engine.LoadSource
import org.krunk.browser.sync.SyncController
import org.krunk.browser.sync.sentence
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.vault.SecureVaultSurface
import org.krunk.browser.ui.vault.VaultClipboard
import org.krunk.ffi.DeviceTabs
import org.krunk.ffi.LinkPoll
import org.krunk.ffi.SetupSecretsText
import org.krunk.ffi.SyncFfiException
import org.krunk.ffi.SyncStage
import org.krunk.ffi.SyncStatus
import org.krunk.ffi.TurnOn

/**
 * Settings › Account & sync (0.6.0): sign-in by code, turning sync on (first device, ask a
 * device, restore), the status (address, last sync and its result, what syncs, Sync now), the
 * other devices' tabs and signing out. Errors are the core's sentences, as Windows shows them.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SyncScreen(vm: BrowserViewModel) {
    val app = LocalContext.current.applicationContext as KrunkApp
    val sync = app.sync
    val status by sync.status.collectAsState()
    val scope = rememberCoroutineScope()
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var secrets by remember { mutableStateOf<SetupSecretsText?>(null) }

    fun act(block: suspend () -> Result<*>) {
        if (busy) return
        busy = true
        error = null
        scope.launch {
            block().onFailure { e -> error = (e as? SyncFfiException)?.sentence ?: e.message }
            busy = false
        }
    }

    LaunchedEffect(Unit) { sync.refresh() }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.sync_title)) },
                navigationIcon = {
                    IconButton(onClick = { vm.pop() }) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = stringResource(R.string.toolbar_back),
                        )
                    }
                },
            )
        },
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            val st = status
            if (st == null) {
                CircularProgressIndicator()
                return@Column
            }
            st.message?.let { Text(it, style = MaterialTheme.typography.bodyMedium) }
            when (val stage = st.stage) {
                SyncStage.SignedOut -> SignedOut(busy) { act { sync.linkStart() } }

                is SyncStage.Linking -> Linking(stage, sync, vm, onError = { error = it })

                SyncStage.Off -> {
                    Account(st)
                    Text(stringResource(R.string.sync_turn_on_help), style = MaterialTheme.typography.bodySmall)
                    Button(enabled = !busy, onClick = {
                        act { sync.turnOn().onSuccess { if (it is TurnOn.Setup) secrets = it.secrets } }
                    }) { Text(stringResource(R.string.sync_turn_on)) }
                    SignOutButton(sync, busy) { act(it) }
                }

                SyncStage.Setup -> {
                    val s = secrets
                    if (s == null) {
                        // The secrets are shown once; after a restart of the screen, start again.
                        LaunchedEffect(Unit) { sync.cancel() }
                    } else {
                        Setup(s, busy) { group -> act { sync.setupConfirm(group).onSuccess { secrets = null } } }
                    }
                }

                is SyncStage.Join -> Join(app, busy, onAsk = { share -> act { sync.joinStart(share) } }) { secret, rk ->
                    act { sync.restoreTry(secret, rk) }
                }

                is SyncStage.RestoreConfirm -> RestoreConfirm(stage, busy, onConfirm = {
                    act { sync.restoreConfirm() }
                }) {
                    scope.launch { sync.cancel() }
                }

                is SyncStage.Joining -> Joining(stage, sync, busy, onCode = { code -> act { sync.joinCode(code) } }) {
                    scope.launch { sync.cancel() }
                }

                SyncStage.On -> On(st, sync, vm, busy, act = { act(it) })

                SyncStage.Alarm -> {
                    Text(stringResource(R.string.sync_alarm_title), style = MaterialTheme.typography.titleMedium)
                    Account(st)
                    SignOutButton(sync, busy) { act(it) }
                }
            }
            if (busy) CircularProgressIndicator(Modifier.semantics { contentDescription = "working" })
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        }
    }
}

@Composable
private fun SignedOut(busy: Boolean, onSignIn: () -> Unit) {
    Text(stringResource(R.string.sync_signed_out_text))
    Button(enabled = !busy, onClick = onSignIn) { Text(stringResource(R.string.sync_sign_in)) }
}

@Composable
private fun Linking(stage: SyncStage.Linking, sync: SyncController, vm: BrowserViewModel, onError: (String) -> Unit) {
    val scope = rememberCoroutineScope()
    Text(stringResource(R.string.sync_linking_text))
    Text(stage.userCode, style = MaterialTheme.typography.headlineMedium, fontFamily = FontFamily.Monospace)
    Button(onClick = {
        vm.tabs.openTab(stage.verifyUrl, activate = true, live = true)
        vm.showBrowser()
    }) { Text(stringResource(R.string.sync_open_approval)) }
    OutlinedButton(onClick = { scope.launch { sync.linkCancel() } }) { Text(stringResource(R.string.sync_cancel)) }
    // Poll while the screen shows the code (the core paces it: 5 s, more after slow_down).
    LaunchedEffect(stage.userCode) {
        while (true) {
            val r = sync.linkPoll()
            val next = r.getOrNull()
            when {
                r.isFailure -> {
                    (r.exceptionOrNull() as? SyncFfiException)?.let { onError(it.sentence) }
                    delay(10_000)
                }

                next is LinkPoll.Wait -> delay(next.ms.toLong())

                else -> break
            }
        }
    }
}

@Composable
private fun Account(st: SyncStatus) {
    st.email?.let { Text(stringResource(R.string.sync_signed_in_as, it), style = MaterialTheme.typography.titleMedium) }
    st.deviceName?.let {
        Text(stringResource(R.string.sync_device_name, it), style = MaterialTheme.typography.bodySmall)
    }
}

@Composable
private fun SignOutButton(sync: SyncController, busy: Boolean, act: (suspend () -> Result<*>) -> Unit) {
    var ask by remember { mutableStateOf(false) }
    OutlinedButton(enabled = !busy, onClick = { ask = true }) { Text(stringResource(R.string.sync_sign_out)) }
    if (ask) {
        AlertDialog(
            onDismissRequest = { ask = false },
            title = { Text(stringResource(R.string.sync_sign_out_title)) },
            text = { Text(stringResource(R.string.sync_sign_out_text)) },
            confirmButton = {
                TextButton(onClick = {
                    ask = false
                    act { Result.success(sync.signOut(removeSyncedData = false)) }
                }) { Text(stringResource(R.string.sync_sign_out_keep)) }
            },
            dismissButton = {
                TextButton(onClick = {
                    ask = false
                    act { Result.success(sync.signOut(removeSyncedData = true)) }
                }) { Text(stringResource(R.string.sync_sign_out_remove)) }
            },
        )
    }
}

@Composable
private fun Setup(s: SetupSecretsText, busy: Boolean, onConfirm: (String) -> Unit) {
    // The passphrase and the recovery key are on screen: no screenshots, no recents thumbnail.
    SecureVaultSurface()
    val context = LocalContext.current
    var group by remember { mutableStateOf("") }
    Text(stringResource(R.string.sync_setup_title), style = MaterialTheme.typography.titleMedium)
    Text(stringResource(R.string.sync_setup_text))
    Secret(stringResource(R.string.sync_passphrase), s.passphrase) {
        VaultClipboard.copyPassword(context, s.passphrase)
    }
    Secret(stringResource(R.string.sync_recovery_key), s.recoveryKey) {
        VaultClipboard.copyPassword(context, s.recoveryKey)
    }
    OutlinedTextField(
        value = group,
        onValueChange = { group = it.take(8) },
        label = { Text(stringResource(R.string.sync_setup_group)) },
        singleLine = true,
        modifier = Modifier.fillMaxWidth(),
    )
    Button(enabled = !busy && group.isNotBlank(), onClick = { onConfirm(group) }) {
        Text(stringResource(R.string.sync_setup_confirm))
    }
}

@Composable
private fun Secret(label: String, value: String, onCopy: () -> Unit) {
    Column {
        Text(label, style = MaterialTheme.typography.labelLarge)
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(value, fontFamily = FontFamily.Monospace, modifier = Modifier.weight(1f))
            TextButton(onClick = onCopy) { Text(stringResource(R.string.sync_copy)) }
        }
    }
}

@Composable
private fun Join(app: KrunkApp, busy: Boolean, onAsk: (Boolean) -> Unit, onRestore: (String, Boolean) -> Unit) {
    SecureVaultSurface()
    var share by remember { mutableStateOf(true) }
    var secret by remember { mutableStateOf("") }
    val vaultOpen = app.vault.state.collectAsState().value == org.krunk.browser.vault.VaultState.UNLOCKED
    Text(stringResource(R.string.sync_join_text))
    Button(enabled = !busy, onClick = { onAsk(share && vaultOpen) }) { Text(stringResource(R.string.sync_join_ask)) }
    if (vaultOpen) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(stringResource(R.string.sync_join_share_passwords), Modifier.weight(1f))
            Switch(checked = share, onCheckedChange = { share = it })
        }
    }
    HorizontalDivider()
    OutlinedTextField(
        value = secret,
        onValueChange = { secret = it.take(256) },
        label = { Text(stringResource(R.string.sync_restore_label)) },
        visualTransformation = PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false),
        singleLine = true,
        modifier = Modifier.fillMaxWidth(),
    )
    Button(enabled = !busy && secret.isNotBlank(), onClick = {
        onRestore(secret, secret.trim().uppercase().startsWith("KRNK-"))
    }) { Text(stringResource(R.string.sync_restore)) }
    if (busy) Text(stringResource(R.string.sync_restore_working), style = MaterialTheme.typography.bodySmall)
}

@Composable
private fun RestoreConfirm(
    stage: SyncStage.RestoreConfirm,
    busy: Boolean,
    onConfirm: () -> Unit,
    onCancel: () -> Unit,
) {
    Text(stringResource(R.string.sync_restore_confirm_title), style = MaterialTheme.typography.titleMedium)
    for (d in stage.devices) Text("• $d")
    if (stage.revoked > 0u) Text(pluralStringResource(R.plurals.sync_restore_confirm_revoked, stage.revoked.toInt(), stage.revoked.toInt()))
    Button(enabled = !busy, onClick = onConfirm) { Text(stringResource(R.string.sync_restore_confirm)) }
    OutlinedButton(onClick = onCancel) { Text(stringResource(R.string.sync_cancel)) }
}

@Composable
private fun Joining(
    stage: SyncStage.Joining,
    sync: SyncController,
    busy: Boolean,
    onCode: (String) -> Unit,
    onCancel: () -> Unit,
) {
    var code by remember { mutableStateOf("") }
    Text(stringResource(R.string.sync_joining_text))
    if (stage.enterCode) {
        OutlinedTextField(
            value = code,
            onValueChange = { v -> code = v.filter { it.isDigit() }.take(6) },
            label = { Text(stringResource(R.string.sync_joining_code)) },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.NumberPassword),
            singleLine = true,
        )
        Button(enabled = !busy && code.length == 6, onClick = { onCode(code) }) {
            Text(stringResource(R.string.sync_joining_confirm))
        }
    } else {
        CircularProgressIndicator()
    }
    OutlinedButton(onClick = onCancel) { Text(stringResource(R.string.sync_cancel)) }
    // Every 3 s while the request is open (the server's limit for one approval is every 2 s).
    LaunchedEffect(Unit) {
        while (true) {
            delay(3_000)
            val s = sync.joinPoll().getOrNull()
            if (s != null && s !is SyncStage.Joining) break
        }
    }
}

@Composable
private fun On(
    st: SyncStatus,
    sync: SyncController,
    vm: BrowserViewModel,
    busy: Boolean,
    act: (suspend () -> Result<*>) -> Unit,
) {
    val scope = rememberCoroutineScope()
    val format = remember { DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT) }
    var remote by remember { mutableStateOf<List<DeviceTabs>>(emptyList()) }
    LaunchedEffect(st.lastSyncMs) { remote = sync.remoteTabs() }

    Account(st)
    Text(stringResource(R.string.sync_status_title), style = MaterialTheme.typography.titleMedium)
    Text(
        st.lastSyncMs?.let { stringResource(R.string.sync_last, format.format(Date(it))) }
            ?: stringResource(R.string.sync_last_never),
    )
    st.lastResult?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
    if (st.pending >
        0
    ) {
        Text(pluralStringResource(R.plurals.sync_pending, st.pending.toInt(), st.pending.toInt()), style = MaterialTheme.typography.bodySmall)
    }
    Button(enabled = !busy, onClick = { act { sync.syncNow() } }) { Text(stringResource(R.string.sync_now)) }

    Text(stringResource(R.string.sync_types_title), style = MaterialTheme.typography.titleMedium)
    for (c in st.collections) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(stringResource(typeLabel(c.name)), Modifier.weight(1f))
            Switch(checked = c.on, enabled = !busy, onCheckedChange = { on -> act { sync.setCollection(c.name, on) } })
        }
    }

    Text(stringResource(R.string.sync_remote_tabs), style = MaterialTheme.typography.titleMedium)
    if (remote.all { it.tabs.isEmpty() }) Text(stringResource(R.string.sync_remote_tabs_none))
    for (d in remote) {
        if (d.tabs.isEmpty()) continue
        Text(d.name, style = MaterialTheme.typography.labelLarge)
        for (t in d.tabs) {
            ListItem(
                headlineContent = { Text(t.title.ifBlank { t.url }, maxLines = 1) },
                supportingContent = { Text(t.url, maxLines = 1, style = MaterialTheme.typography.bodySmall) },
                modifier = Modifier.padding(0.dp),
                trailingContent = {
                    TextButton(onClick = {
                        // Another device's address: opened like a link, never privileged.
                        vm.tabs.openUntrusted(t.url, LoadSource.EXTERNAL_APP, activate = true, live = true)
                        vm.showBrowser()
                    }) { Text(stringResource(R.string.sync_open_tab)) }
                },
            )
        }
    }
    Spacer(Modifier.height(8.dp))
    SignOutButton(sync, busy) { act(it) }
    LaunchedEffect(Unit) { scope.launch { sync.refresh() } }
}

private fun typeLabel(name: String): Int = when (name) {
    "bookmarks" -> R.string.sync_type_bookmarks
    "passwords" -> R.string.sync_type_passwords
    "history" -> R.string.sync_type_history
    "settings" -> R.string.sync_type_settings
    "pins" -> R.string.sync_type_pins
    "allowlist" -> R.string.sync_type_allowlist
    else -> R.string.sync_type_tabs
}
