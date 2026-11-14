package org.krunk.browser.ui.permissions

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.krunk.browser.R
import org.krunk.browser.engine.permissions.PermissionDefault
import org.krunk.browser.engine.permissions.PermissionKind
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.Screen
import org.krunk.ffi.SitePermission

/** The name of a kind on the screens; a key this version does not know is shown as it is stored. */
@Composable
fun kindLabel(key: String): String = when (PermissionKind.ofKey(key)) {
    PermissionKind.CAMERA -> stringResource(R.string.perm_kind_camera)
    PermissionKind.MICROPHONE -> stringResource(R.string.perm_kind_microphone)
    PermissionKind.GEOLOCATION -> stringResource(R.string.perm_kind_geolocation)
    PermissionKind.NOTIFICATIONS -> stringResource(R.string.perm_kind_notifications)
    PermissionKind.PERSISTENT_STORAGE -> stringResource(R.string.perm_kind_persistent_storage)
    PermissionKind.XR -> stringResource(R.string.perm_kind_xr)
    PermissionKind.AUTOPLAY_INAUDIBLE -> stringResource(R.string.perm_kind_autoplay_inaudible)
    PermissionKind.AUTOPLAY_AUDIBLE -> stringResource(R.string.perm_kind_autoplay_audible)
    PermissionKind.DRM -> stringResource(R.string.perm_kind_drm)
    PermissionKind.STORAGE_ACCESS -> stringResource(R.string.perm_kind_storage_access)
    PermissionKind.LOCAL_DEVICE -> stringResource(R.string.perm_kind_local_device)
    PermissionKind.LOCAL_NETWORK -> stringResource(R.string.perm_kind_local_network)
    null -> key
}

/** The table's rows, read off the main thread; [generation] changes after every change made here. */
@Composable
private fun rememberRows(vm: BrowserViewModel, generation: Int): List<SitePermission>? {
    var rows by remember { mutableStateOf<List<SitePermission>?>(null) }
    LaunchedEffect(generation) {
        val perms = vm.engine.permissions
        rows = if (perms == null) emptyList() else withContext(Dispatchers.IO) { perms.list() }
    }
    return rows
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun PermissionsScaffold(title: String, onBack: () -> Unit, content: @Composable (Modifier) -> Unit) {
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(title, maxLines = 1, overflow = TextOverflow.StartEllipsis) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = stringResource(R.string.action_back),
                        )
                    }
                },
            )
        },
    ) { padding -> content(Modifier.fillMaxSize().padding(padding)) }
}

/**
 * Settings > Site permissions: the sites with a remembered answer, with a search field, what
 * each is allowed and denied, and a button that removes all answers of a site. A tap opens the
 * site's page.
 */
@Composable
fun SitePermissionsScreen(vm: BrowserViewModel) {
    var generation by remember { mutableIntStateOf(0) }
    var query by rememberSaveable { mutableStateOf("") }
    var removing by remember { mutableStateOf<SitePermissionsUi.SiteRow?>(null) }
    val rows = rememberRows(vm, generation)
    val sites = remember(rows, query) { SitePermissionsUi.sites(rows.orEmpty(), query) }

    PermissionsScaffold(stringResource(R.string.perm_sites_title), onBack = { vm.pop() }) { modifier ->
        Column(modifier) {
            OutlinedTextField(
                value = query,
                onValueChange = { query = it.take(200) },
                label = { Text(stringResource(R.string.perm_sites_search)) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
            )
            if (rows != null && sites.isEmpty()) {
                Box(Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
                    Text(
                        stringResource(
                            if (rows.isEmpty()) R.string.perm_sites_empty else R.string.perm_sites_none_found,
                        ),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            } else {
                LazyColumn(Modifier.fillMaxSize()) {
                    items(sites, key = { it.origin }) { site ->
                        SiteLine(site, onOpen = { vm.push(Screen.SiteDetail(site.origin)) }, onRemove = {
                            removing =
                                site
                        })
                        HorizontalDivider()
                    }
                }
            }
        }
    }
    removing?.let { site ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text(stringResource(R.string.perm_sites_remove_title)) },
            text = { Text(stringResource(R.string.perm_sites_remove_text, site.label)) },
            confirmButton = {
                TextButton(onClick = {
                    vm.engine.permissions?.resetOrigin(site.origin)
                    removing = null
                    generation++
                }) { Text(stringResource(R.string.perm_sites_remove_confirm)) }
            },
            dismissButton = {
                TextButton(onClick = { removing = null }) { Text(stringResource(R.string.action_cancel)) }
            },
        )
    }
}

@Composable
private fun kindNames(keys: List<String>): String {
    val names = ArrayList<String>(keys.size)
    for (key in keys) names += kindLabel(key)
    return names.joinToString(", ")
}

@Composable
private fun SiteLine(site: SitePermissionsUi.SiteRow, onOpen: () -> Unit, onRemove: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onOpen).padding(start = 24.dp, end = 8.dp).heightIn(min = 64.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f).padding(vertical = 10.dp)) {
            Text(
                site.label,
                style = MaterialTheme.typography.bodyLarge,
                maxLines = 1,
                overflow = TextOverflow.StartEllipsis,
            )
            if (site.allowed.isNotEmpty()) {
                Text(
                    stringResource(R.string.perm_sites_allowed, kindNames(site.allowed)),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (site.denied.isNotEmpty()) {
                Text(
                    stringResource(R.string.perm_sites_denied, kindNames(site.denied)),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        IconButton(onClick = onRemove) {
            Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.perm_sites_remove, site.label))
        }
    }
}

/**
 * One site: every permission kind with Allow, Deny and Ask (or the kind's default), and "Reset
 * permissions". A change is stored at once and counts from the site's next request on.
 */
@Composable
fun SiteDetailScreen(vm: BrowserViewModel, origin: String) {
    var generation by remember { mutableIntStateOf(0) }
    val rows = rememberRows(vm, generation)
    val kinds = remember(rows, origin) { SitePermissionsUi.detail(origin, rows.orEmpty()) }
    val hasRows = rows.orEmpty().any { it.origin == origin }

    PermissionsScaffold(origin.removePrefix("https://"), onBack = { vm.pop() }) { modifier ->
        LazyColumn(modifier) {
            item {
                Text(
                    stringResource(R.string.perm_detail_help),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 24.dp, vertical = 12.dp),
                )
            }
            if (rows != null) {
                items(kinds, key = { it.key }) { row ->
                    KindLine(row) { choice ->
                        vm.engine.permissions?.setFromUi(origin, row.key, SitePermissionsUi.valueOf(choice))
                        generation++
                    }
                    HorizontalDivider()
                }
                item {
                    TextButton(
                        onClick = {
                            vm.engine.permissions?.resetOrigin(origin)
                            generation++
                        },
                        enabled = hasRows,
                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                    ) { Text(stringResource(R.string.perm_detail_reset)) }
                }
            }
        }
    }
}

@Composable
private fun choiceLabel(choice: SitePermissionsUi.Choice, default: PermissionDefault): String = when (choice) {
    SitePermissionsUi.Choice.ALLOW -> stringResource(R.string.perm_choice_allow)

    SitePermissionsUi.Choice.DENY -> stringResource(R.string.perm_choice_deny)

    SitePermissionsUi.Choice.DEFAULT -> when (default) {
        PermissionDefault.ASK -> stringResource(R.string.perm_choice_ask)
        PermissionDefault.ALLOW -> stringResource(R.string.perm_choice_default_allow)
        PermissionDefault.DENY -> stringResource(R.string.perm_choice_default_deny)
    }
}

@Composable
private fun KindLine(row: SitePermissionsUi.KindRow, onChoice: (SitePermissionsUi.Choice) -> Unit) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 8.dp)) {
        Text(kindLabel(row.key), style = MaterialTheme.typography.bodyLarge)
        if (row.kind == null) {
            Text(
                stringResource(R.string.perm_kind_unknown_help),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        FlowRow(Modifier.fillMaxWidth().selectableGroup()) {
            for (choice in row.choices) {
                Row(
                    Modifier
                        .heightIn(min = 48.dp)
                        .selectable(selected = row.choice == choice, role = Role.RadioButton) { onChoice(choice) }
                        .padding(end = 16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RadioButton(selected = row.choice == choice, onClick = null)
                    Text(
                        choiceLabel(choice, row.default),
                        style = MaterialTheme.typography.bodyMedium,
                        modifier = Modifier.padding(start = 8.dp),
                    )
                }
            }
        }
    }
}
