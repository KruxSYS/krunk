package org.krunk.browser.ui.tabs

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import org.krunk.browser.ui.BrowserViewModel

/** The tab switcher (grid, private section, recently closed). Owned by work package B. */
@Composable
fun TabSwitcherScreen(vm: BrowserViewModel) {
    Box(Modifier.fillMaxSize()) { Text("Tabs: ${vm.tabs.tabs.value.size}") }
}
