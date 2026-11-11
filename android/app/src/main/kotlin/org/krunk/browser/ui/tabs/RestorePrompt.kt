package org.krunk.browser.ui.tabs

import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.window.DialogProperties
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.krunk.browser.R
import org.krunk.browser.ui.BrowserViewModel

/**
 * After a crash: "Restore tabs?" with Restore / Start fresh. Driven by
 * `TabManager.restorePrompt` (state, so it also shows when the event fired before the UI was
 * composed). Rendered inside the browser screen. A tap outside does nothing; back = start fresh.
 */
@Composable
fun RestoreTabsPrompt(vm: BrowserViewModel) {
    val prompt by vm.tabs.restorePrompt.collectAsStateWithLifecycle()
    val state = prompt ?: return
    AlertDialog(
        onDismissRequest = { vm.tabs.discardPendingRestore() },
        properties = DialogProperties(dismissOnClickOutside = false),
        title = { Text(stringResource(R.string.restore_title)) },
        text = { Text(pluralStringResource(R.plurals.restore_text, state.tabCount, state.tabCount)) },
        confirmButton = {
            TextButton(onClick = {
                vm.tabs.restorePending()
                vm.showBrowser()
            }) { Text(stringResource(R.string.restore_accept)) }
        },
        dismissButton = {
            TextButton(onClick = { vm.tabs.discardPendingRestore() }) { Text(stringResource(R.string.restore_fresh)) }
        },
    )
}
