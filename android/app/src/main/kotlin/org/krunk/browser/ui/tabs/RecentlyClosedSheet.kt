package org.krunk.browser.ui.tabs

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.History
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.krunk.browser.R
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.ffi.ClosedTab
import org.krunk.ffi.displayUrl

/**
 * Recently closed tabs, newest first; a tap reopens the tab in the browser. The list is the
 * core's (persisted in `kv["android.closed_tabs"]`), so it survives a restart.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RecentlyClosedSheet(vm: BrowserViewModel, onDismiss: () -> Unit) {
    val closed by vm.tabs.closedTabs.collectAsStateWithLifecycle()
    val sheet = rememberModalBottomSheetState()
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet) {
        Row(
            Modifier.fillMaxWidth().padding(start = 24.dp, end = 12.dp, bottom = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                stringResource(R.string.tabs_recently_closed),
                style = MaterialTheme.typography.titleMedium,
                modifier = Modifier.weight(1f),
            )
            if (closed.isNotEmpty()) {
                TextButton(onClick = { vm.tabs.clearClosedTabs() }) {
                    Text(stringResource(R.string.tabs_recently_closed_clear))
                }
            }
        }
        if (closed.isEmpty()) {
            Text(
                stringResource(R.string.tabs_recently_closed_empty),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 24.dp),
            )
        } else {
            val newestFirst = closed.asReversed()
            LazyColumn(Modifier.fillMaxWidth().heightIn(max = 420.dp).padding(bottom = 24.dp)) {
                items(newestFirst.size, key = { it }) { i ->
                    // `closedTabs` is oldest first; reopen by the core's index.
                    val coreIndex = closed.size - 1 - i
                    ClosedRow(newestFirst[i]) {
                        if (vm.tabs.reopenClosed(coreIndex) != null) {
                            onDismiss()
                            vm.showBrowser()
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun ClosedRow(tab: ClosedTab, onReopen: () -> Unit) {
    val title = tab.title.ifBlank { stringResource(R.string.tabs_untitled) }
    val reopenLabel = stringResource(R.string.tabs_reopen, title)
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClickLabel = reopenLabel, onClick = onReopen)
            .semantics { contentDescription = reopenLabel }
            .heightIn(min = 56.dp)
            .padding(horizontal = 24.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Icons.Filled.History, contentDescription = null, modifier = Modifier.size(24.dp))
        Spacer(Modifier.width(16.dp))
        Column(Modifier.weight(1f)) {
            Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyLarge)
            Text(
                displayUrl(tab.url),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
