package org.krunk.browser.ui.history

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.DeleteSweep
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.krunk.browser.R
import org.krunk.browser.core.HistoryRepository
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.bookmarks.SearchField
import org.krunk.browser.ui.bookmarks.copyToClipboard
import org.krunk.browser.ui.newtab.SiteIcon
import org.krunk.ffi.HistoryEntry
import org.krunk.ffi.nowMs

/**
 * History: visits newest first, grouped by local day (Today, Yesterday, then dates), loaded a
 * page at a time while scrolling; search; swipe or long-press to delete one visit; "Clear
 * history". Private tabs never write visits, so there is nothing to filter here.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HistoryScreen(vm: BrowserViewModel) {
    val repo = remember(vm) { HistoryRepository(vm.core.store) }
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val snackbar = remember { SnackbarHostState() }

    var searching by remember { mutableStateOf(false) }
    var query by remember { mutableStateOf("") }
    var entries by remember { mutableStateOf<List<HistoryEntry>>(emptyList()) }
    var exhausted by remember { mutableStateOf(false) }
    var loading by remember { mutableStateOf(false) }
    var generation by remember { mutableIntStateOf(0) }
    var confirmClear by remember { mutableStateOf(false) }

    suspend fun loadMore(reset: Boolean) {
        if (loading) return
        loading = true
        val before = if (reset) null else entries.lastOrNull()?.ts
        val page = runCatching { repo.page(query.trim(), before) }.getOrDefault(emptyList())
        entries = if (reset) page else entries + page.filter { p -> entries.none { it.id == p.id } }
        exhausted = page.size < HistoryRepository.PAGE
        loading = false
    }

    LaunchedEffect(query, generation) {
        if (query.isNotBlank()) delay(200)
        loadMore(reset = true)
    }

    val listState = rememberLazyListState()
    val nearEnd by remember {
        derivedStateOf {
            val info = listState.layoutInfo
            val last = info.visibleItemsInfo.lastOrNull()?.index ?: 0
            info.totalItemsCount > 0 && last >= info.totalItemsCount - 8
        }
    }
    LaunchedEffect(nearEnd, exhausted, entries.size) {
        if (nearEnd && !exhausted && !loading && entries.isNotEmpty()) loadMore(reset = false)
    }

    BackHandler(enabled = searching) {
        searching = false
        query = ""
    }

    fun delete(e: HistoryEntry) {
        entries = entries.filterNot { it.id == e.id }
        scope.launch {
            runCatching { repo.delete(listOf(e.id)) }
            snackbar.showSnackbar(context.getString(R.string.history_entry_deleted))
        }
    }

    val zone = remember { ZoneId.systemDefault() }
    val now = nowMs()
    val today = HistoryRepository.localDay(now)
    val yesterday = HistoryRepository.localDay(now - DAY_MS)
    val sections = remember(entries) { HistoryRepository.groupByDay(entries) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    if (searching) {
                        SearchField(query, { query = it }, stringResource(R.string.history_search_hint))
                    } else {
                        Text(stringResource(R.string.history_title))
                    }
                },
                navigationIcon = {
                    IconButton(onClick = {
                        if (searching) {
                            searching = false
                            query = ""
                        } else {
                            vm.pop()
                        }
                    }) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = stringResource(R.string.action_back),
                        )
                    }
                },
                actions = {
                    if (searching) {
                        IconButton(onClick = {
                            searching = false
                            query = ""
                        }) { Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.action_close)) }
                    } else {
                        IconButton(onClick = { searching = true }) {
                            Icon(Icons.Filled.Search, contentDescription = stringResource(R.string.action_search))
                        }
                        IconButton(onClick = { confirmClear = true }, enabled = entries.isNotEmpty()) {
                            Icon(Icons.Filled.DeleteSweep, contentDescription = stringResource(R.string.history_clear))
                        }
                    }
                },
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        if (entries.isEmpty() && !loading) {
            Box(Modifier.fillMaxSize().padding(padding).padding(32.dp), contentAlignment = Alignment.Center) {
                Text(
                    if (query.isNotBlank()) {
                        stringResource(R.string.history_no_results, query.trim())
                    } else {
                        stringResource(R.string.history_empty)
                    },
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyLarge,
                )
            }
        } else {
            LazyColumn(state = listState, modifier = Modifier.fillMaxSize().padding(padding)) {
                for (section in sections) {
                    item(key = "day-${section.day}") {
                        Text(
                            dayLabel(section.day, today, yesterday),
                            style = MaterialTheme.typography.titleSmall,
                            color = MaterialTheme.colorScheme.primary,
                            modifier = Modifier.fillMaxWidth().padding(
                                start = 16.dp,
                                top = 16.dp,
                                end = 16.dp,
                                bottom = 4.dp,
                            ),
                        )
                    }
                    items(section.entries, key = { it.id }) { e ->
                        HistoryRow(
                            e = e,
                            time = timeLabel(e.ts, zone),
                            vm = vm,
                            onOpen = {
                                vm.goTo(e.url)
                                vm.showBrowser()
                            },
                            onOpenNewTab = {
                                vm.tabs.openTab(e.url, activate = true, live = true)
                                vm.showBrowser()
                            },
                            onCopy = {
                                copyToClipboard(context, e.url)
                                scope.launch { snackbar.showSnackbar(context.getString(R.string.copied)) }
                            },
                            onDelete = { delete(e) },
                        )
                    }
                }
                if (!exhausted) {
                    item(key = "more") {
                        Text(
                            stringResource(R.string.history_load_more),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.fillMaxWidth().padding(16.dp),
                        )
                    }
                }
            }
        }
    }

    if (confirmClear) {
        AlertDialog(
            onDismissRequest = { confirmClear = false },
            title = { Text(stringResource(R.string.history_clear_confirm_title)) },
            text = { Text(stringResource(R.string.history_clear_confirm_text)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmClear = false
                    scope.launch {
                        runCatching { repo.clear() }
                        entries = emptyList()
                        generation++
                        snackbar.showSnackbar(context.getString(R.string.history_cleared))
                    }
                }) { Text(stringResource(R.string.action_clear), color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = {
                TextButton(onClick = { confirmClear = false }) { Text(stringResource(R.string.action_cancel)) }
            },
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
private fun HistoryRow(
    e: HistoryEntry,
    time: String,
    vm: BrowserViewModel,
    onOpen: () -> Unit,
    onOpenNewTab: () -> Unit,
    onCopy: () -> Unit,
    onDelete: () -> Unit,
) {
    var menu by remember { mutableStateOf(false) }
    val dismiss = rememberSwipeToDismissBoxState(
        confirmValueChange = { value ->
            if (value == SwipeToDismissBoxValue.Settled) {
                false
            } else {
                onDelete()
                true
            }
        },
    )
    SwipeToDismissBox(
        state = dismiss,
        backgroundContent = {
            Box(
                Modifier.fillMaxSize().background(MaterialTheme.colorScheme.errorContainer).padding(horizontal = 24.dp),
                contentAlignment = if (dismiss.dismissDirection == SwipeToDismissBoxValue.StartToEnd) {
                    Alignment.CenterStart
                } else {
                    Alignment.CenterEnd
                },
            ) {
                Icon(
                    Icons.Filled.Delete,
                    contentDescription = stringResource(R.string.history_delete_entry),
                    tint = MaterialTheme.colorScheme.onErrorContainer,
                )
            }
        },
    ) {
        Box {
            ListItem(
                headlineContent = { Text(e.title.ifBlank { e.url }, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                supportingContent = { Text("$time · ${e.url}", maxLines = 1, overflow = TextOverflow.Ellipsis) },
                leadingContent = { SiteIcon(e.url, vm.tabs.favicons, size = 32.dp, fallbackText = e.title) },
                colors = ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.surface),
                modifier = Modifier.fillMaxWidth().combinedClickable(onClick = onOpen, onLongClick = { menu = true }),
            )
            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                DropdownMenuItem(text = { Text(stringResource(R.string.action_open_new_tab)) }, onClick = {
                    menu = false
                    onOpenNewTab()
                })
                DropdownMenuItem(text = { Text(stringResource(R.string.action_copy_link)) }, onClick = {
                    menu = false
                    onCopy()
                })
                DropdownMenuItem(text = { Text(stringResource(R.string.history_delete_entry)) }, onClick = {
                    menu = false
                    onDelete()
                })
            }
        }
    }
}

private const val DAY_MS = 86_400_000L

@Composable
private fun dayLabel(day: String, today: String, yesterday: String): String = when (day) {
    today -> stringResource(R.string.history_today)
    yesterday -> stringResource(R.string.history_yesterday)
    else -> remember(day) { formatDay(day) }
}

/** `YYYY-MM-DD` as a full localised date ("Tuesday, 11 November 2026"); the key itself if odd. */
fun formatDay(day: String): String = runCatching {
    LocalDate.parse(day).format(DateTimeFormatter.ofLocalizedDate(FormatStyle.FULL))
}.getOrDefault(day)

fun timeLabel(ts: Long, zone: ZoneId): String =
    Instant.ofEpochMilli(ts).atZone(zone).toLocalTime().format(DateTimeFormatter.ofPattern("HH:mm"))
