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
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.krunk.browser.R
import org.krunk.browser.core.HistoryCursor
import org.krunk.browser.core.HistoryDayKind
import org.krunk.browser.core.HistoryItem
import org.krunk.browser.core.HistoryRepository
import org.krunk.browser.core.HistorySection
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.bookmarks.SearchField
import org.krunk.browser.ui.bookmarks.copyToClipboard
import org.krunk.browser.ui.newtab.SiteIcon
import org.krunk.ffi.HistoryEntry

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
    val res = LocalResources.current
    val snackbar = remember { SnackbarHostState() }

    var searching by remember { mutableStateOf(false) }
    var query by remember { mutableStateOf("") }
    var entries by remember { mutableStateOf<List<HistoryItem>>(emptyList()) }
    var sections by remember { mutableStateOf<List<HistorySection>>(emptyList()) }
    // Where the next page starts: the last row *read*, which a deletion on screen cannot move.
    var cursor by remember { mutableStateOf<HistoryCursor?>(null) }
    var exhausted by remember { mutableStateOf(false) }
    var loading by remember { mutableStateOf(false) }
    var generation by remember { mutableIntStateOf(0) }
    var confirmClear by remember { mutableStateOf(false) }

    suspend fun loadMore(reset: Boolean) {
        // A new query always loads; only "more of the same" waits for the load under way.
        if (loading && !reset) return
        loading = true
        try {
            val before = if (reset) null else cursor
            // A cancelled load (the query changed, the screen left) is not an empty page: it
            // must not mark the list as finished.
            val page = try {
                repo.page(query.trim(), before)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                null
            }
            // A newer query replaced the list while this "more" page was read: it belongs nowhere.
            if (!reset && cursor != before) return
            if (page == null) {
                // The read failed: show what there is and stop asking.
                if (reset) {
                    entries = emptyList()
                    sections = emptyList()
                    cursor = null
                }
                exhausted = true
                return
            }
            // Pages are cut by (time, id): nothing of a page can be in the list already.
            val all = if (reset) page.items else entries + page.items
            // Day runs are built off the main thread; the rows already carry day and time.
            val grouped = repo.sections(all)
            entries = all
            sections = grouped
            if (reset || page.next != null) cursor = page.next
            exhausted = page.last
        } finally {
            loading = false
        }
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
        // One pass over rows in memory that already carry their day: no FFI, no date maths.
        entries = entries.filterNot { it.entry.id == e.id }
        sections = HistoryRepository.groupByDay(entries)
        scope.launch {
            runCatching { repo.delete(listOf(e.id)) }
            snackbar.showSnackbar(res.getString(R.string.history_entry_deleted))
        }
    }

    val now = System.currentTimeMillis()

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
                            dayLabel(section.day, now),
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
                    items(section.items, key = { it.entry.id }) { item ->
                        val e = item.entry
                        HistoryRow(
                            e = e,
                            time = item.time,
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
                                scope.launch { snackbar.showSnackbar(res.getString(R.string.copied)) }
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
                        // The same clearing as Settings > Clear browsing data > History.
                        val ok = vm.clearBrowsingData(history = true).isSuccess
                        entries = emptyList()
                        sections = emptyList()
                        cursor = null
                        generation++
                        val text = if (ok) R.string.history_cleared else R.string.settings_clear_failed
                        snackbar.showSnackbar(res.getString(text))
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

@Composable
private fun dayLabel(day: String, now: Long): String = when (HistoryRepository.dayKind(day, now)) {
    HistoryDayKind.TODAY -> stringResource(R.string.history_today)
    HistoryDayKind.YESTERDAY -> stringResource(R.string.history_yesterday)
    HistoryDayKind.DATE -> remember(day) { formatDay(day) }
}

/** `YYYY-MM-DD` as a full localised date ("Tuesday, 11 November 2026"); the key itself if odd. */
fun formatDay(day: String): String = runCatching {
    LocalDate.parse(day).format(DateTimeFormatter.ofLocalizedDate(FormatStyle.FULL))
}.getOrDefault(day)
