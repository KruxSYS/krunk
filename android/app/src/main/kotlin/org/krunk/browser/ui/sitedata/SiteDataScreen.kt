package org.krunk.browser.ui.sitedata

import android.text.format.Formatter
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Sort
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import kotlinx.coroutines.launch
import org.krunk.browser.KrunkApp
import org.krunk.browser.R
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.bookmarks.SearchField
import org.krunk.browser.ui.newtab.SiteIcon

/**
 * Site data: the sites that hold cookies or stored data, by name or by last use, with search;
 * a tap shows a site's addresses, the bin clears the site after a confirmation that says what
 * goes. Where the list comes from and what a cleared site does to it: see [SiteDataModel].
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SiteDataScreen(vm: BrowserViewModel) {
    val app = LocalContext.current.applicationContext as? KrunkApp
    val controller = remember(vm, app) {
        SiteDataController(vm.core, vm.engine, vm.tabs, app?.sitePermissions?.let { it::resetOrigin })
    }
    val scope = rememberCoroutineScope()
    val res = LocalResources.current
    val snackbar = remember { SnackbarHostState() }

    var list by remember { mutableStateOf<SiteList?>(null) }
    var generation by remember { mutableIntStateOf(0) }
    var searching by rememberSaveable { mutableStateOf(false) }
    var query by rememberSaveable { mutableStateOf("") }
    var order by rememberSaveable { mutableStateOf(SiteOrder.NAME) }
    var sortMenu by remember { mutableStateOf(false) }
    var expanded by rememberSaveable { mutableStateOf<String?>(null) }
    var confirm by remember { mutableStateOf<SiteRow?>(null) }

    LaunchedEffect(generation) {
        // The engine's answer normally comes at once. While its helper is still starting, the
        // list from Krunk's own records is shown first and replaced when the answer arrives.
        val first = controller.engineData(SiteDataController.FIRST_ANSWER_MS)
        list = controller.load(first)
        if (first == null) {
            val late = controller.engineData(SiteDataController.LATE_ANSWER_MS)
            if (late != null) list = controller.load(late)
        }
    }

    fun closeSearch() {
        searching = false
        query = ""
    }
    BackHandler(enabled = searching) { closeSearch() }

    val current = list
    val rows = remember(current, order, query) {
        current?.let { SiteDataModel.sorted(SiteDataModel.search(it.rows, query), order) }.orEmpty()
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    if (searching) {
                        SearchField(query, { query = it }, stringResource(R.string.site_data_search_hint))
                    } else {
                        Text(stringResource(R.string.site_data_title))
                    }
                },
                navigationIcon = {
                    IconButton(onClick = { if (searching) closeSearch() else vm.pop() }) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = stringResource(R.string.action_back),
                        )
                    }
                },
                actions = {
                    if (searching) {
                        IconButton(onClick = { closeSearch() }) {
                            Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.action_close))
                        }
                    } else {
                        IconButton(onClick = { searching = true }) {
                            Icon(Icons.Filled.Search, contentDescription = stringResource(R.string.action_search))
                        }
                        Box {
                            IconButton(onClick = { sortMenu = true }) {
                                Icon(
                                    Icons.AutoMirrored.Filled.Sort,
                                    contentDescription = stringResource(R.string.site_data_sort),
                                )
                            }
                            DropdownMenu(expanded = sortMenu, onDismissRequest = { sortMenu = false }) {
                                SortItem(R.string.site_data_sort_name, order == SiteOrder.NAME) {
                                    order = SiteOrder.NAME
                                    sortMenu = false
                                }
                                SortItem(R.string.site_data_sort_last_used, order == SiteOrder.LAST_USED) {
                                    order = SiteOrder.LAST_USED
                                    sortMenu = false
                                }
                            }
                        }
                    }
                },
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        LazyColumn(Modifier.fillMaxSize().padding(padding)) {
            val note = when (current?.source) {
                SiteSource.HISTORY -> R.string.site_data_note_history
                SiteSource.PARTIAL -> R.string.site_data_note_partial
                else -> null
            }
            if (note != null && current != null && current.rows.isNotEmpty()) {
                item(key = "note") { Note(stringResource(note)) }
            }
            if (rows.isEmpty()) {
                item(key = "empty") {
                    Note(
                        when {
                            current == null -> stringResource(R.string.site_data_loading)
                            query.isNotBlank() -> stringResource(R.string.site_data_no_results, query.trim())
                            current.source == SiteSource.ENGINE -> stringResource(R.string.site_data_empty)
                            else -> stringResource(R.string.site_data_empty_history)
                        },
                    )
                }
            }
            items(rows, key = { "site-${it.site}" }) { row ->
                SiteItem(
                    row = row,
                    vm = vm,
                    expanded = expanded == row.site,
                    onToggle = { expanded = if (expanded == row.site) null else row.site },
                    onClear = { confirm = row },
                )
            }
        }
    }

    confirm?.let { row ->
        ClearSiteDialog(
            site = row.site,
            onDismiss = { confirm = null },
            onClear = { withPermissions ->
                confirm = null
                scope.launch {
                    val ok = controller.clear(row.site, withPermissions)
                    val before = list
                    if (ok && before != null) list = SiteDataModel.cleared(before, row.site, withPermissions)
                    // The engine is asked again: a reloaded tab may have stored something anew.
                    generation++
                    val stays = list?.rows?.any { it.site == row.site } == true
                    val text = when {
                        !ok -> R.string.site_data_clear_failed
                        stays && before?.source != SiteSource.ENGINE -> R.string.site_data_cleared_listed
                        else -> R.string.site_data_cleared
                    }
                    snackbar.showSnackbar(res.getString(text, row.site))
                }
            },
        )
    }
}

@Composable
private fun Note(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
    )
}

@Composable
private fun SortItem(label: Int, selected: Boolean, onClick: () -> Unit) {
    DropdownMenuItem(
        text = { Text(stringResource(label)) },
        leadingIcon = { RadioButton(selected = selected, onClick = null) },
        onClick = onClick,
    )
}

@Composable
private fun SiteItem(row: SiteRow, vm: BrowserViewModel, expanded: Boolean, onToggle: () -> Unit, onClear: () -> Unit) {
    val context = LocalContext.current
    val parts = ArrayList<String>()
    row.cookies?.let { n ->
        parts += if (n == 0) {
            stringResource(R.string.site_data_no_cookies)
        } else {
            pluralStringResource(R.plurals.site_data_cookies, n, n)
        }
    }
    row.bytes?.takeIf { it > 0L }?.let { bytes ->
        parts += stringResource(R.string.site_data_stored, Formatter.formatShortFileSize(context, bytes))
    }
    if (row.hasPermissions) parts += stringResource(R.string.site_data_permissions)
    row.lastUsed?.let { ts -> parts += stringResource(R.string.site_data_last_used, remember(ts) { dateLabel(ts) }) }
    Column {
        ListItem(
            headlineContent = { Text(row.site, maxLines = 1, overflow = TextOverflow.Ellipsis) },
            supportingContent = { if (parts.isNotEmpty()) Text(parts.joinToString(" · ")) },
            leadingContent = {
                SiteIcon(row.origins.firstOrNull() ?: "https://${row.site}/", vm.tabs.favicons, fallbackText = row.site)
            },
            trailingContent = {
                IconButton(onClick = onClear) {
                    Icon(
                        Icons.Filled.Delete,
                        contentDescription = stringResource(R.string.site_data_clear_description, row.site),
                    )
                }
            },
            colors = ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.surface),
            modifier = Modifier.fillMaxWidth().clickable(onClick = onToggle),
        )
        if (expanded) {
            Column(Modifier.fillMaxWidth().padding(start = 72.dp, end = 16.dp, bottom = 12.dp)) {
                if (row.origins.isEmpty()) {
                    Text(
                        stringResource(R.string.site_data_no_origins),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                } else {
                    Text(
                        stringResource(R.string.site_data_origins),
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.primary,
                    )
                    for (origin in row.origins) {
                        Text(
                            origin,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(top = 4.dp),
                        )
                    }
                }
                TextButton(onClick = onClear) {
                    Text(stringResource(R.string.site_data_clear), color = MaterialTheme.colorScheme.error)
                }
            }
        }
    }
}

@Composable
private fun ClearSiteDialog(site: String, onDismiss: () -> Unit, onClear: (withPermissions: Boolean) -> Unit) {
    var permissions by remember { mutableStateOf(true) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.site_data_clear_title, site)) },
        text = {
            Column {
                Text(stringResource(R.string.site_data_clear_text, site))
                Row(
                    Modifier.fillMaxWidth().padding(top = 12.dp)
                        .toggleable(value = permissions, role = Role.Checkbox, onValueChange = { permissions = it })
                        .padding(vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Checkbox(checked = permissions, onCheckedChange = null)
                    Spacer(Modifier.width(12.dp))
                    Text(stringResource(R.string.site_data_clear_permissions))
                }
            }
        },
        confirmButton = {
            TextButton(onClick = { onClear(permissions) }) {
                Text(stringResource(R.string.action_clear), color = MaterialTheme.colorScheme.error)
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

private fun dateLabel(ts: Long): String = Instant.ofEpochMilli(ts).atZone(ZoneId.systemDefault()).toLocalDate()
    .format(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM))
