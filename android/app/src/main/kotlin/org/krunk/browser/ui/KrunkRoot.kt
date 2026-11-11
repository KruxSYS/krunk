package org.krunk.browser.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.krunk.browser.ui.browser.BrowserScreen
import org.krunk.browser.ui.theme.KrunkTheme

/** The app: theme, the screen stack and back handling. */
@Composable
fun KrunkRoot(vm: BrowserViewModel) {
    val settings by vm.settings.collectAsStateWithLifecycle()
    val activeTab by vm.activeTab.collectAsStateWithLifecycle()
    val screens by vm.screens.collectAsStateWithLifecycle()
    val screen = screens.last()

    KrunkTheme(mode = settings.theme, privateMode = activeTab?.private == true && screen == Screen.Browser) {
        Surface(Modifier.fillMaxSize()) {
            BackHandler(enabled = screens.size > 1) { vm.pop() }
            when (screen) {
                Screen.Browser -> BrowserScreen(vm)
                else -> PlaceholderScreen(screen)
            }
        }
    }
}

@Composable
private fun PlaceholderScreen(screen: Screen) {
    Box(Modifier.fillMaxSize()) { Text("$screen (coming in this round)") }
}
