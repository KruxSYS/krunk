package org.krunk.browser.ui.vault

import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.Lock
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import org.krunk.browser.R
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.bookmarks.SearchField
import org.krunk.browser.ui.newtab.SiteIcon
import org.krunk.browser.vault.VaultItem
import org.krunk.browser.vault.VaultState

/**
 * The password vault: create it, unlock it, the logins with search, one login with its password
 * behind "Show", add, edit, delete, and the vault's own settings. What is on screen follows the
 * vault's state; a lock, from wherever, takes everything of the vault's content off the screen.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun VaultScreen(vm: BrowserViewModel, port: VaultPort) {
    SecureVaultSurface()
    val scope = rememberCoroutineScope()
    val holder = remember(port) { VaultUiState(port, scope) }
    val model by holder.model.collectAsStateWithLifecycle()
    val res = LocalResources.current
    val snackbar = remember { SnackbarHostState() }

    var searching by remember { mutableStateOf(false) }
    var menu by remember { mutableStateOf(false) }
    val unlocked = model.vault == VaultState.UNLOCKED

    // Leaving the screen, or the app leaving the foreground: no password stays shown.
    DisposableEffect(holder) { onDispose { holder.clearSecrets() } }
    LifecycleEventEffect(Lifecycle.Event.ON_STOP) { holder.hide() }
    LaunchedEffect(model.vault) {
        menu = false
        if (!unlocked) searching = false
    }
    LaunchedEffect(holder) {
        holder.model.map { it.notice }.filterNotNull().collect { notice ->
            holder.noticeShown()
            val text = notice.arg?.let { res.getString(notice.id, it) } ?: res.getString(notice.id)
            launch { snackbar.showSnackbar(text) }
        }
    }

    fun stopSearch() {
        searching = false
        holder.setQuery("")
    }
    BackHandler(enabled = searching) { stopSearch() }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    if (searching && unlocked) {
                        SearchField(model.query, holder::setQuery, stringResource(R.string.vault_search_hint))
                    } else {
                        Text(stringResource(R.string.vault_title), maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                },
                navigationIcon = {
                    IconButton(onClick = { if (searching) stopSearch() else vm.pop() }) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = stringResource(R.string.action_back),
                        )
                    }
                },
                actions = {
                    if (!unlocked && model.vault != VaultState.NONE) return@TopAppBar
                    if (searching) {
                        IconButton(onClick = { stopSearch() }) {
                            Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.action_close))
                        }
                    } else {
                        if (unlocked) {
                            IconButton(onClick = { searching = true }) {
                                Icon(Icons.Filled.Search, contentDescription = stringResource(R.string.action_search))
                            }
                        }
                        Box {
                            IconButton(onClick = { menu = true }) {
                                Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.action_more))
                            }
                            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                                // Without a vault only the settings: nothing to lock or change.
                                if (unlocked) {
                                    DropdownMenuItem(
                                        text = { Text(stringResource(R.string.vault_lock_now)) },
                                        onClick = {
                                            menu = false
                                            holder.lockNow()
                                        },
                                    )
                                    DropdownMenuItem(
                                        text = { Text(stringResource(R.string.vault_change_password)) },
                                        onClick = {
                                            menu = false
                                            holder.askChangePassword()
                                        },
                                    )
                                }
                                DropdownMenuItem(text = { Text(stringResource(R.string.vault_settings)) }, onClick = {
                                    menu = false
                                    holder.openSettings()
                                })
                            }
                        }
                    }
                },
            )
        },
        floatingActionButton = {
            if (unlocked) {
                FloatingActionButton(onClick = holder::add) {
                    Icon(Icons.Filled.Add, contentDescription = stringResource(R.string.vault_add))
                }
            }
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        val body = Modifier.fillMaxSize().padding(padding)
        when (model.vault) {
            VaultState.NONE -> CreatePane(holder, model, port.minPasswordLength, body)
            VaultState.LOCKED -> LockedPane(holder, port, body)
            VaultState.KEY_LOST -> KeyLostPane(holder, body)
            VaultState.UNLOCKED -> LoginList(vm, holder, model, body)
        }
    }

    VaultDialogHost(holder, port, model.dialog)
}

@Composable
private fun CreatePane(holder: VaultUiState, model: VaultUiModel, minLength: Int, modifier: Modifier) {
    // Plain remember on purpose: the vault password must not reach saved instance state.
    var password by remember { mutableStateOf("") }
    var repeat by remember { mutableStateOf("") }
    Pane(modifier) {
        Text(stringResource(R.string.vault_none_title), style = MaterialTheme.typography.headlineSmall)
        Text(stringResource(R.string.vault_none_text), style = MaterialTheme.typography.bodyMedium)
        PasswordField(
            value = password,
            onChange = {
                password = it
                holder.createEdited()
            },
            label = stringResource(R.string.vault_new_password),
            enabled = !model.busy,
        )
        PasswordField(
            value = repeat,
            onChange = {
                repeat = it
                holder.createEdited()
            },
            label = stringResource(R.string.vault_repeat_password),
            error = model.createError?.text(),
            enabled = !model.busy,
            imeAction = ImeAction.Done,
            onIme = { holder.create(password, repeat) },
        )
        Text(
            stringResource(R.string.vault_min_length, minLength),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        NoRecoveryNote()
        if (model.busy) BusyLine(stringResource(R.string.vault_creating))
        Button(onClick = { holder.create(password, repeat) }, enabled = !model.busy) {
            Text(stringResource(R.string.vault_create))
        }
    }
}

/** The one thing a user has to know before choosing the password, set apart from the rest. */
@Composable
private fun NoRecoveryNote() {
    Surface(
        color = MaterialTheme.colorScheme.errorContainer,
        contentColor = MaterialTheme.colorScheme.onErrorContainer,
        shape = MaterialTheme.shapes.medium,
    ) {
        Row(Modifier.padding(12.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Outlined.Info, contentDescription = null)
            Text(
                stringResource(R.string.vault_no_recovery),
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(start = 12.dp),
            )
        }
    }
}

@Composable
private fun LockedPane(holder: VaultUiState, port: VaultPort, modifier: Modifier) {
    val unlock by holder.unlock.model.collectAsStateWithLifecycle()
    val activity = LocalActivity.current
    // Plain remember on purpose; the pane leaves the composition when the vault opens.
    var password by remember { mutableStateOf("") }
    Pane(modifier) {
        Icon(
            Icons.Outlined.Lock,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.primary,
            modifier = Modifier.size(40.dp),
        )
        Text(stringResource(R.string.vault_locked_title), style = MaterialTheme.typography.headlineSmall)
        UnlockFields(
            model = unlock,
            password = password,
            onPassword = { password = it },
            onSubmit = { holder.unlock.submit(password) },
            onBiometric = { activity?.let { a -> holder.unlock.biometric { port.biometric.unlock(a) } } },
        )
        Button(onClick = { holder.unlock.submit(password) }, enabled = unlock.canSubmit && password.isNotEmpty()) {
            Text(stringResource(R.string.vault_unlock))
        }
    }
}

@Composable
private fun KeyLostPane(holder: VaultUiState, modifier: Modifier) {
    Pane(modifier) {
        Text(stringResource(R.string.vault_key_lost_title), style = MaterialTheme.typography.headlineSmall)
        Text(stringResource(R.string.vault_key_lost_text), style = MaterialTheme.typography.bodyMedium)
        Text(stringResource(R.string.vault_key_lost_next), style = MaterialTheme.typography.bodyMedium)
        Button(
            onClick = holder::askDestroy,
            colors = ButtonDefaults.buttonColors(
                containerColor = MaterialTheme.colorScheme.error,
                contentColor = MaterialTheme.colorScheme.onError,
            ),
        ) { Text(stringResource(R.string.vault_destroy)) }
    }
}

/** The frame of the three states without a list: scrolls, and stays readable on a tablet. */
@Composable
private fun Pane(modifier: Modifier, content: @Composable () -> Unit) {
    Box(modifier.verticalScroll(rememberScrollState()), contentAlignment = Alignment.TopCenter) {
        Column(
            Modifier.widthIn(max = 520.dp).fillMaxWidth().padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) { content() }
    }
}

@Composable
private fun LoginList(vm: BrowserViewModel, holder: VaultUiState, model: VaultUiModel, modifier: Modifier) {
    if (model.items.isEmpty()) {
        // Nothing before the first list arrived: "no logins" would be a guess.
        if (!model.loaded) return
        Box(modifier.padding(32.dp), contentAlignment = Alignment.Center) {
            Text(
                if (model.query.isNotBlank()) {
                    stringResource(R.string.vault_no_results, model.query.trim())
                } else {
                    stringResource(R.string.vault_empty)
                },
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyLarge,
            )
        }
        return
    }
    LazyColumn(modifier) {
        items(model.items, key = { it.id }) { item -> LoginRow(vm, item) { holder.open(item.id) } }
    }
}

@Composable
private fun LoginRow(vm: BrowserViewModel, item: VaultItem, onClick: () -> Unit) {
    val site = siteLabel(item.origin)
    ListItem(
        headlineContent = { Text(site, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        supportingContent = {
            Text(
                item.username.ifEmpty { stringResource(R.string.vault_no_username) },
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        },
        leadingContent = { SiteIcon(item.origin, vm.tabs.favicons, size = 32.dp, fallbackText = site) },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        modifier = Modifier.fillMaxWidth().clickable(onClick = onClick),
    )
}
