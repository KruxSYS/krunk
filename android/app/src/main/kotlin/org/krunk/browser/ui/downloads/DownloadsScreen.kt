package org.krunk.browser.ui.downloads

import android.content.Context
import android.text.format.Formatter
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.DeleteSweep
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.krunk.browser.R
import org.krunk.browser.downloads.DownloadOpener
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.bookmarks.SearchField

/**
 * Krunk's downloads: running, paused and finished ones, newest first, a page at a time while
 * scrolling; search; per entry open, share, pause or resume, retry or restart, cancel, remove
 * from the list; "Clear finished". Files are never deleted from here.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DownloadsScreen(vm: BrowserViewModel) {
    val downloads = vm.downloads
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    var state by remember { mutableStateOf(DownloadsUiState()) }
    var searching by remember { mutableStateOf(false) }
    val revision by downloads.revision.collectAsStateWithLifecycle()

    LaunchedEffect(state.query, state.limit, revision) {
        val query = state.query
        // Typing: wait for a pause before asking the database.
        if (query.isNotBlank()) delay(150)
        downloads.query(query, state.limit)?.let { state = state.answer(query, it) }
    }

    val listState = rememberLazyListState()
    val nearEnd by remember {
        derivedStateOf {
            val info = listState.layoutInfo
            val last = info.visibleItemsInfo.lastOrNull()?.index ?: 0
            info.totalItemsCount > 0 && last >= info.totalItemsCount - 8
        }
    }
    LaunchedEffect(nearEnd, state.rows.size) {
        if (nearEnd && state.canLoadMore) state = state.more()
    }

    fun endSearch() {
        searching = false
        state = state.search("")
    }
    BackHandler(enabled = searching) { endSearch() }

    val fileGone = stringResource(R.string.downloads_file_gone)
    val noApp = stringResource(R.string.downloads_no_app)
    val shareTitle = stringResource(R.string.downloads_share)
    fun report(result: DownloadOpener.Result) {
        val text = when (result) {
            DownloadOpener.Result.OK -> return
            DownloadOpener.Result.GONE -> fileGone
            DownloadOpener.Result.NO_APP -> noApp
        }
        scope.launch { snackbar.showSnackbar(text) }
    }

    fun run(row: DownloadRow, action: RowAction) {
        when (action) {
            RowAction.OPEN -> report(DownloadOpener.open(context, row.path))
            RowAction.SHARE -> report(DownloadOpener.share(context, row.path, shareTitle))
            RowAction.PAUSE -> downloads.pause(row.id)
            RowAction.RESUME, RowAction.RETRY -> downloads.resume(row.id)
            RowAction.RESTART -> downloads.restart(row.id)
            RowAction.CANCEL -> downloads.cancel(row.id)
            RowAction.REMOVE -> downloads.remove(row.id)
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    if (searching) {
                        SearchField(
                            state.query,
                            { state = state.search(it) },
                            stringResource(R.string.downloads_search_hint),
                        )
                    } else {
                        Text(stringResource(R.string.downloads_title))
                    }
                },
                navigationIcon = {
                    IconButton(onClick = { if (searching) endSearch() else vm.pop() }) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = stringResource(R.string.action_back),
                        )
                    }
                },
                actions = {
                    if (searching) {
                        IconButton(onClick = { endSearch() }) {
                            Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.action_close))
                        }
                    } else {
                        IconButton(onClick = { searching = true }) {
                            Icon(Icons.Filled.Search, contentDescription = stringResource(R.string.action_search))
                        }
                        IconButton(onClick = { downloads.clearFinished() }, enabled = state.canClearFinished) {
                            Icon(
                                Icons.Filled.DeleteSweep,
                                contentDescription = stringResource(R.string.downloads_clear_finished),
                            )
                        }
                    }
                },
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        val empty = state.empty
        if (empty != null) {
            Box(Modifier.fillMaxSize().padding(padding).padding(32.dp), contentAlignment = Alignment.Center) {
                Text(
                    when (empty) {
                        EmptyKind.NO_DOWNLOADS -> stringResource(R.string.downloads_empty)
                        EmptyKind.NO_MATCH -> stringResource(R.string.downloads_no_results, state.query.trim())
                    },
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyLarge,
                )
            }
        } else {
            LazyColumn(state = listState, modifier = Modifier.fillMaxSize().padding(padding)) {
                items(state.rows, key = { it.id.toLong() }) { row ->
                    DownloadRowItem(row, onAction = { run(row, it) })
                }
            }
        }
    }
}

@Composable
private fun DownloadRowItem(row: DownloadRow, onAction: (RowAction) -> Unit) {
    val context = LocalContext.current
    var menu by remember { mutableStateOf(false) }
    val failed = row.status == RowStatus.FAILED || row.status == RowStatus.FAILED_CAN_RETRY
    val primary = row.primary
    ListItem(
        modifier = if (primary != null) Modifier.clickable { onAction(primary) } else Modifier,
        headlineContent = { Text(row.fileName, maxLines = 1, overflow = TextOverflow.MiddleEllipsis) },
        supportingContent = {
            Column {
                Text(
                    statusLine(context, row),
                    color = if (failed) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 3,
                    overflow = TextOverflow.Ellipsis,
                )
                if (row.status == RowStatus.RUNNING || row.status == RowStatus.PAUSED) {
                    val fraction = row.fraction
                    val bar = Modifier.fillMaxWidth().padding(top = 6.dp)
                    if (fraction != null) {
                        LinearProgressIndicator(progress = { fraction }, modifier = bar)
                    } else if (row.status == RowStatus.RUNNING) {
                        LinearProgressIndicator(modifier = bar)
                    }
                }
                val note = when (row.note) {
                    RowNote.CANNOT_PAUSE -> stringResource(R.string.downloads_note_cannot_pause)
                    RowNote.AGAIN_FROM_PAGE -> stringResource(R.string.downloads_note_again_from_page)
                    null -> null
                }
                if (note != null) {
                    Text(note, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(top = 4.dp))
                }
            }
        },
        trailingContent = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                val quick = row.actions.firstOrNull()?.takeIf { quickIcon(it) != null }
                if (quick != null) {
                    IconButton(onClick = { onAction(quick) }) {
                        Icon(quickIcon(quick) ?: Icons.Filled.Close, contentDescription = actionLabel(quick))
                    }
                }
                Box {
                    IconButton(onClick = { menu = true }) {
                        Icon(
                            Icons.Filled.MoreVert,
                            contentDescription = stringResource(R.string.downloads_more_actions),
                        )
                    }
                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                        for (action in row.actions) {
                            DropdownMenuItem(
                                text = { Text(actionLabel(action)) },
                                onClick = {
                                    menu = false
                                    onAction(action)
                                },
                            )
                        }
                    }
                }
            }
        },
    )
}

/** The actions that get a button of their own next to the row's menu. */
private fun quickIcon(action: RowAction): ImageVector? = when (action) {
    RowAction.PAUSE -> Icons.Filled.Pause
    RowAction.RESUME -> Icons.Filled.PlayArrow
    RowAction.RETRY, RowAction.RESTART -> Icons.Filled.Refresh
    RowAction.CANCEL -> Icons.Filled.Close
    RowAction.OPEN, RowAction.SHARE, RowAction.REMOVE -> null
}

@Composable
private fun actionLabel(action: RowAction): String = stringResource(
    when (action) {
        RowAction.OPEN -> R.string.downloads_open
        RowAction.SHARE -> R.string.downloads_share
        RowAction.PAUSE -> R.string.downloads_pause
        RowAction.RESUME -> R.string.downloads_resume
        RowAction.RETRY -> R.string.downloads_retry
        RowAction.RESTART -> R.string.downloads_restart
        RowAction.CANCEL -> R.string.downloads_cancel
        RowAction.REMOVE -> R.string.downloads_remove
    },
)

/** "Private · 1.2 MB of 5 MB · 300 kB/s · 12 s left · example.org", as far as each part is known. */
private fun statusLine(context: Context, row: DownloadRow): String {
    fun size(bytes: Long) = Formatter.formatShortFileSize(context, bytes)
    val amount = row.total?.let { context.getString(R.string.downloads_amount_of, size(row.received), size(it)) }
        ?: size(row.received)
    val parts = ArrayList<String>()
    if (row.private) parts += context.getString(R.string.downloads_private)
    when (row.status) {
        RowStatus.RUNNING -> {
            parts += amount
            if (row.speed > 0) parts += context.getString(R.string.downloads_speed, size(row.speed))
            row.timeLeft?.let { left ->
                parts += when {
                    left.hours > 0 -> context.getString(R.string.downloads_left_hours, left.hours, left.minutes)
                    left.minutes > 0 -> context.getString(R.string.downloads_left_minutes, left.minutes)
                    else -> context.getString(R.string.downloads_left_seconds, left.seconds)
                }
            }
        }

        RowStatus.PAUSED -> {
            parts += context.getString(R.string.downloads_status_paused)
            parts += amount
        }

        RowStatus.FAILED_CAN_RETRY, RowStatus.FAILED -> {
            parts += row.reason?.let { context.getString(R.string.downloads_status_failed_reason, it) }
                ?: context.getString(R.string.downloads_status_failed)
        }

        RowStatus.DONE -> parts += size(row.total ?: row.received)

        RowStatus.CANCELLED -> parts += context.getString(R.string.downloads_status_cancelled)
    }
    if (row.site.isNotEmpty()) parts += row.site
    return parts.joinToString(" · ")
}
