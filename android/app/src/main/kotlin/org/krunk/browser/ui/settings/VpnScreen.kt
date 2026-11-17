package org.krunk.browser.ui.settings

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Key
import androidx.compose.material.icons.outlined.UploadFile
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import java.io.IOException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.krunk.browser.R
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.theme.KrunkSwitch
import org.krunk.browser.vpn.ImportResult
import org.krunk.browser.vpn.VpnController
import org.krunk.browser.vpn.VpnCredentials
import org.krunk.browser.vpn.VpnPhase
import org.krunk.browser.vpn.VpnProfile
import org.krunk.browser.vpn.VpnScope
import org.krunk.browser.vpn.VpnUiState

/** A profile file read from another app (SAF picker, share, open with). */
sealed class VpnFile {
    class Ok(val name: String, val bytes: ByteArray) : VpnFile()

    data object TooLarge : VpnFile()

    data object Unreadable : VpnFile()
}

/** Reads a profile file of at most 64 KiB (blocking; call off the main thread). */
fun readVpnFile(context: Context, uri: Uri): VpnFile = try {
    val name = runCatching {
        context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) c.getString(0) else null
        }
    }.getOrNull() ?: uri.lastPathSegment?.substringAfterLast('/') ?: "profile"
    val input = context.contentResolver.openInputStream(uri) ?: throw IOException("no stream")
    // One byte more than allowed tells "too large" without reading a large file.
    val bytes = input.use { it.readNBytesCompat(VpnImportSniff.MAX_BYTES + 1) }
    if (bytes.size > VpnImportSniff.MAX_BYTES) {
        bytes.fill(0)
        VpnFile.TooLarge
    } else {
        VpnFile.Ok(name, bytes)
    }
} catch (_: IOException) {
    VpnFile.Unreadable
} catch (_: SecurityException) {
    VpnFile.Unreadable
} catch (_: IllegalArgumentException) {
    VpnFile.Unreadable
}

private fun java.io.InputStream.readNBytesCompat(max: Int): ByteArray {
    val out = java.io.ByteArrayOutputStream()
    val buf = ByteArray(8192)
    while (out.size() < max) {
        val n = read(buf, 0, minOf(buf.size, max - out.size()))
        if (n < 0) break
        out.write(buf, 0, n)
    }
    buf.fill(0)
    return out.toByteArray()
}

/** Reads [uri] and has the core check it; nothing is saved. */
suspend fun importVpnUri(context: Context, vpn: VpnController, uri: Uri): ImportResult {
    val file = withContext(Dispatchers.IO) { readVpnFile(context, uri) }
    return when (file) {
        is VpnFile.Ok -> vpn.importFile(file.name, file.bytes).also { file.bytes.fill(0) }
        VpnFile.TooLarge -> ImportResult.Failed(context.getString(R.string.vpn_import_too_large))
        VpnFile.Unreadable -> ImportResult.Failed(context.getString(R.string.vpn_import_unreadable))
    }
}

/** "WireGuard" / "OpenVPN" for the core's kind. */
fun vpnKindLabel(kind: String): String = when {
    kind.contains("wire", ignoreCase = true) -> "WireGuard"
    kind.contains("openvpn", ignoreCase = true) -> "OpenVPN"
    else -> kind
}

/** The status line: Off / Connecting… / Connected · tunnel IP / Failed: reason. */
@Composable
fun vpnStatusText(state: VpnUiState): String {
    val st = state.status
    return when (state.phase) {
        VpnPhase.OFF -> stringResource(R.string.vpn_status_off)

        VpnPhase.CONNECTING -> stringResource(R.string.vpn_status_connecting)

        VpnPhase.CONNECTED -> st?.tunnelIp?.takeIf { it.isNotBlank() }
            ?.let { stringResource(R.string.vpn_status_connected_ip, it) }
            ?: stringResource(R.string.vpn_status_connected)

        VpnPhase.FAILED -> st?.error?.takeIf { it.isNotBlank() }
            ?.let { stringResource(R.string.vpn_status_failed, it) }
            ?: stringResource(R.string.vpn_status_failed_plain)
    }
}

/**
 * Settings › Privacy & security › VPN (0.10.0): profiles (choose, import, remove), the master
 * switch and its status, the VPN's options and what it covers. A profile handed over by another
 * app ([BrowserViewModel.vpnImport]) is asked about here.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun VpnScreen(vm: BrowserViewModel) {
    val vpn = vm.vpn
    val state by vpn.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val s = state.settings
    val selected = s.selectedProfileId?.takeIf { id -> state.profiles.any { it.id == id } }
        ?: state.profiles.singleOrNull()?.id
    var error by remember { mutableStateOf<String?>(null) }
    // Dialogs. The import holds the profile: plain `remember`, never saved state.
    var pending by remember { mutableStateOf<ImportResult?>(null) }
    var askCredsFor by remember { mutableStateOf<ImportResult.Ok?>(null) }
    var credsFor by remember { mutableStateOf<VpnProfile?>(null) }
    var removing by remember { mutableStateOf<VpnProfile?>(null) }

    fun act(block: suspend () -> Result<*>) {
        error = null
        scope.launch { block().onFailure { e -> error = e.message } }
    }

    // A profile offered by another app.
    val offered by vm.vpnImport.collectAsStateWithLifecycle()
    LaunchedEffect(offered) {
        if (offered != null) {
            (pending as? ImportResult.Ok)?.bytes?.fill(0)
            pending = vm.takeVpnImport()
        }
    }
    // Leaving the screen drops an import that was not saved.
    val held by rememberUpdatedState(listOf(pending, askCredsFor))
    DisposableEffect(Unit) {
        onDispose { held.forEach { (it as? ImportResult.Ok)?.bytes?.fill(0) } }
    }

    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) scope.launch { pending = importVpnUri(context, vpn, uri) }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.vpn_title)) },
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
        Column(Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState())) {
            // ----- on / off -----
            VpnSwitchRow(stringResource(R.string.vpn_use), vpnStatusText(state), s.enabled) { on ->
                act { if (on) vpn.connect(selected) else vpn.disconnect() }
                if (on) pick = null
            }
            error?.let {
                Text(
                    it,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
                )
            }
            HorizontalDivider()

            // ----- profiles -----
            SectionTitle(stringResource(R.string.vpn_profiles))
            if (state.profiles.isEmpty()) {
                Text(
                    stringResource(R.string.vpn_profiles_empty),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                )
            }
            Column(Modifier.selectableGroup()) {
                for (p in state.profiles) {
                    ProfileRow(
                        profile = p,
                        selected = p.id == selected,
                        // Saved at once (VpnController.select); reconnects if the VPN is on.
                        onSelect = { if (p.id != s.selectedProfileId) act { vpn.select(p.id) } },
                        onCredentials = { credsFor = p },
                        onRemove = { removing = p },
                    )
                }
            }
            OutlinedButton(
                onClick = { picker.launch(arrayOf("*/*")) },
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
            ) {
                Icon(Icons.Outlined.UploadFile, contentDescription = null)
                Spacer(Modifier.width(8.dp))
                Text(stringResource(R.string.vpn_import))
            }
            HorizontalDivider()

            // ----- options -----
            VpnSwitchRow(
                stringResource(R.string.vpn_connect_at_start),
                stringResource(R.string.vpn_connect_at_start_help),
                s.connectAtStart,
            ) { v -> act { vpn.setConnectAtStart(v) } }
            SectionTitle(stringResource(R.string.vpn_scope))
            VpnScopeChoice(s.scope) { v -> act { vpn.setScope(v) } }
            VpnSwitchRow(
                stringResource(R.string.vpn_lan_direct),
                stringResource(R.string.vpn_lan_direct_help),
                s.lanDirect,
            ) { v -> act { vpn.setLanDirect(v) } }
            VpnSwitchRow(stringResource(R.string.vpn_show_in_address_bar), null, s.showInAddressBar) { v ->
                act { vpn.setShowInAddressBar(v) }
            }
            HorizontalDivider()
            Text(
                stringResource(R.string.vpn_footer),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(16.dp),
            )
        }
    }

    // ----- dialogs -----
    when (val p = pending) {
        is ImportResult.Ok -> ImportDialog(
            import = p,
            onSave = {
                pending = null
                if (p.info.needsCredentials) {
                    askCredsFor = p
                } else {
                    act { vpn.save(p, null) }
                }
            },
            onCancel = {
                p.bytes.fill(0)
                pending = null
            },
        )

        is ImportResult.Failed -> AlertDialog(
            onDismissRequest = { pending = null },
            title = { Text(stringResource(R.string.vpn_import_failed_title)) },
            text = { Text(p.message) },
            confirmButton = { TextButton(onClick = { pending = null }) { Text(stringResource(R.string.vpn_ok)) } },
        )

        null -> Unit
    }
    askCredsFor?.let { imp ->
        CredentialsDialog(
            profileName = imp.info.name,
            onSave = { creds ->
                askCredsFor = null
                act { vpn.save(imp, creds) }
            },
            onCancel = {
                imp.bytes.fill(0)
                askCredsFor = null
            },
        )
    }
    credsFor?.let { p ->
        CredentialsDialog(
            profileName = p.name,
            onSave = { creds ->
                credsFor = null
                act { vpn.setCredentials(p.id, creds) }
            },
            onCancel = { credsFor = null },
        )
    }
    removing?.let { p ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text(stringResource(R.string.vpn_remove_title, p.name)) },
            text = { Text(stringResource(R.string.vpn_remove_text)) },
            confirmButton = {
                TextButton(onClick = {
                    removing = null
                    if (pick == p.id) pick = null
                    act { vpn.remove(p.id) }
                }) { Text(stringResource(R.string.vpn_remove)) }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text(stringResource(R.string.vpn_cancel)) } },
        )
    }
}

@Composable
private fun SectionTitle(title: String) {
    Text(
        title,
        style = MaterialTheme.typography.titleSmall,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 16.dp, bottom = 4.dp),
    )
}

@Composable
private fun VpnSwitchRow(title: String, summary: String?, checked: Boolean, onChange: (Boolean) -> Unit) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = summary?.let { { Text(it, maxLines = 3) } },
        trailingContent = { KrunkSwitch(checked = checked, onCheckedChange = null) },
        modifier = Modifier.toggleable(value = checked, role = Role.Switch, onValueChange = onChange),
    )
}

/** "Everywhere" / "Private tabs only" (Windows 0.22.0); also used by the address bar's sheet. */
@Composable
fun VpnScopeChoice(scope: VpnScope, onPick: (VpnScope) -> Unit) {
    Column(Modifier.selectableGroup()) {
        for ((value, label) in listOf(
            VpnScope.ALL to stringResource(R.string.vpn_scope_all),
            VpnScope.PRIVATE to stringResource(R.string.vpn_scope_private),
        )) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .selectable(selected = scope == value, role = Role.RadioButton) { onPick(value) }
                    .padding(horizontal = 16.dp, vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(selected = scope == value, onClick = null)
                Spacer(Modifier.width(16.dp))
                Text(label, style = MaterialTheme.typography.bodyLarge)
            }
        }
    }
}

@Composable
private fun ProfileRow(
    profile: VpnProfile,
    selected: Boolean,
    onSelect: () -> Unit,
    onCredentials: () -> Unit,
    onRemove: () -> Unit,
) {
    val selectLabel = stringResource(R.string.vpn_profile_select, profile.name)
    ListItem(
        leadingContent = { RadioButton(selected = selected, onClick = null) },
        headlineContent = { Text(profile.name, maxLines = 1) },
        supportingContent = {
            Text(
                stringResource(R.string.vpn_profile_line, vpnKindLabel(profile.kind), profile.server),
                maxLines = 1,
            )
        },
        trailingContent = {
            Row {
                if (profile.needsCredentials) {
                    IconButton(onClick = onCredentials) {
                        Icon(
                            Icons.Outlined.Key,
                            contentDescription = stringResource(R.string.vpn_profile_sign_in, profile.name),
                        )
                    }
                }
                IconButton(onClick = onRemove) {
                    Icon(
                        Icons.Outlined.Delete,
                        contentDescription = stringResource(R.string.vpn_profile_remove, profile.name),
                    )
                }
            }
        },
        modifier = Modifier
            .selectable(selected = selected, role = Role.RadioButton, onClick = onSelect)
            .semantics { contentDescription = selectLabel },
    )
}

/** What the file is, and the core's warnings, before anything is saved. */
@Composable
private fun ImportDialog(import: ImportResult.Ok, onSave: () -> Unit, onCancel: () -> Unit) {
    val info = import.info
    AlertDialog(
        onDismissRequest = onCancel,
        title = { Text(stringResource(R.string.vpn_import_title)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(stringResource(R.string.vpn_import_name, info.name))
                Text(stringResource(R.string.vpn_import_kind, vpnKindLabel(info.kind)))
                Text(stringResource(R.string.vpn_import_server, info.server))
                if (import.warnings.isNotEmpty()) {
                    Text(
                        stringResource(R.string.vpn_import_warnings),
                        style = MaterialTheme.typography.titleSmall,
                        modifier = Modifier.padding(top = 8.dp),
                    )
                    for (w in import.warnings) {
                        Text("• $w", style = MaterialTheme.typography.bodyMedium)
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onSave) { Text(stringResource(R.string.vpn_save)) } },
        dismissButton = { TextButton(onClick = onCancel) { Text(stringResource(R.string.vpn_cancel)) } },
    )
}

/** User name and password of an OpenVPN profile; kept in `remember` only (never saved state). */
@Composable
private fun CredentialsDialog(profileName: String, onSave: (VpnCredentials) -> Unit, onCancel: () -> Unit) {
    var user by remember { mutableStateOf("") }
    var pass by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onCancel,
        title = { Text(stringResource(R.string.vpn_credentials_title, profileName)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(stringResource(R.string.vpn_credentials_text), style = MaterialTheme.typography.bodyMedium)
                OutlinedTextField(
                    value = user,
                    onValueChange = { user = it },
                    label = { Text(stringResource(R.string.vpn_username)) },
                    singleLine = true,
                )
                OutlinedTextField(
                    value = pass,
                    onValueChange = { pass = it },
                    label = { Text(stringResource(R.string.vpn_password)) },
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                )
            }
        },
        confirmButton = {
            TextButton(
                enabled = user.isNotEmpty() && pass.isNotEmpty(),
                onClick = { onSave(VpnCredentials(user, pass)) },
            ) { Text(stringResource(R.string.vpn_save)) }
        },
        dismissButton = { TextButton(onClick = onCancel) { Text(stringResource(R.string.vpn_cancel)) } },
    )
}
