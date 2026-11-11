package org.krunk.browser.ui.history

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import org.krunk.browser.ui.BrowserViewModel

/** History grouped by day, search, delete, clear. Owned by work package C. */
@Composable
fun HistoryScreen(vm: BrowserViewModel) {
    Box(Modifier.fillMaxSize()) { Text("History") }
}
