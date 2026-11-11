package org.krunk.browser.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.krunk.browser.ui.bookmarks.BookmarksScreen
import org.krunk.browser.ui.browser.BrowserScreen
import org.krunk.browser.ui.browser.SecureWindowEffect
import org.krunk.browser.ui.browser.SystemBarsEffect
import org.krunk.browser.ui.history.HistoryScreen
import org.krunk.browser.ui.settings.AboutScreen
import org.krunk.browser.ui.settings.SettingsScreen
import org.krunk.browser.ui.tabs.TabSwitcherScreen
import org.krunk.browser.ui.theme.KrunkTheme
import org.krunk.browser.ui.theme.LocalKrunkDark

/** The app: theme, the screen stack and back handling. */
@Composable
fun KrunkRoot(vm: BrowserViewModel) {
    val settings by vm.settings.collectAsStateWithLifecycle()
    val activeTab by vm.activeTab.collectAsStateWithLifecycle()
    val screens by vm.screens.collectAsStateWithLifecycle()
    val screen = screens.last()
    val hasPrivateTabs by vm.hasPrivateTabs.collectAsStateWithLifecycle()

    // Private pages stay out of the Recents thumbnail, screenshots and screen recordings.
    SecureWindowEffect(activeTab?.private == true || (screen == Screen.TabSwitcher && hasPrivateTabs))

    KrunkTheme(mode = settings.theme, privateMode = activeTab?.private == true && screen == Screen.Browser) {
        Surface(Modifier.fillMaxSize()) {
            BackHandler(enabled = screens.size > 1) { vm.pop() }
            // The browser screen sets the system bar icons for its own (possibly private) theme;
            // every other screen needs them set for the theme it is drawn in.
            if (screen != Screen.Browser) SystemBarsEffect(fullScreen = false, darkTheme = LocalKrunkDark.current)
            when (screen) {
                Screen.Browser -> BrowserScreen(vm)
                Screen.TabSwitcher -> TabSwitcherScreen(vm)
                Screen.Bookmarks -> BookmarksScreen(vm)
                Screen.History -> HistoryScreen(vm)
                Screen.Settings -> SettingsScreen(vm)
                Screen.About -> AboutScreen(vm)
            }
        }
    }
}
