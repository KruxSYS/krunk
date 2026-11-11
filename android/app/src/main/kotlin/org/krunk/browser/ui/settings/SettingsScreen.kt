package org.krunk.browser.ui.settings

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import org.krunk.browser.ui.BrowserViewModel

/** Settings (core fields + Android-only ones, clear data). Owned by work package C. */
@Composable
fun SettingsScreen(vm: BrowserViewModel) {
    Box(Modifier.fillMaxSize()) { Text("Settings") }
}
