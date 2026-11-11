package org.krunk.browser.ui.newtab

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import org.krunk.browser.ui.BrowserViewModel

/** Krunk's native new-tab page (search box, top sites, shortcuts, wallpaper). Owned by work package C. */
@Composable
fun NewTabScreen(vm: BrowserViewModel, modifier: Modifier = Modifier) {
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Text("New tab", style = MaterialTheme.typography.headlineSmall)
    }
}
