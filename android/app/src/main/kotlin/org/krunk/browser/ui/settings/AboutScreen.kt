package org.krunk.browser.ui.settings

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import org.krunk.browser.ui.BrowserViewModel

/** About: versions, licences, link to krunk.209.wf. Owned by work package C. */
@Composable
fun AboutScreen(vm: BrowserViewModel) {
    Box(Modifier.fillMaxSize()) { Text("About") }
}
