package org.krunk.browser.ui.browser

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.ArrowForward
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.Screen
import org.krunk.browser.ui.newtab.NewTabScreen
import org.krunk.browser.ui.tabs.RestoreTabsPrompt
import org.krunk.ffi.ToolbarPosition
import org.mozilla.geckoview.GeckoView

/**
 * The browser: page (GeckoView or the native new-tab page), toolbar top/bottom with the address
 * bar. This is the M2 foundation; M3 replaces the toolbar and address bar with the full ones.
 */
@Composable
fun BrowserScreen(vm: BrowserViewModel) {
    val tab by vm.activeTab.collectAsStateWithLifecycle()
    val settings by vm.settings.collectAsStateWithLifecycle()
    val page = tab?.page
    val session = tab?.let { vm.tabs.session(it.id) }

    BackHandler(enabled = page?.canGoBack == true) { session?.goBack() }

    Column(Modifier.fillMaxSize().safeDrawingPadding()) {
        if (settings.toolbarPosition == ToolbarPosition.TOP) Toolbar(vm)
        Box(Modifier.fillMaxWidth().weight(1f)) {
            if (tab?.isNewTab == true || session == null) {
                NewTabScreen(vm, Modifier.fillMaxSize())
            } else {
                AndroidView(
                    modifier = Modifier.fillMaxSize(),
                    factory = { ctx -> GeckoView(ctx) },
                    update = { view -> if (view.session != session.gecko) view.setSession(session.gecko) },
                    onRelease = { view -> view.releaseSession() },
                )
            }
            if (page != null && page.loading) {
                LinearProgressIndicator(progress = { page.progress }, modifier = Modifier.fillMaxWidth().height(3.dp))
            }
        }
        if (settings.toolbarPosition == ToolbarPosition.BOTTOM) Toolbar(vm)
    }
    RestoreTabsPrompt(vm)
}

@Composable
private fun Toolbar(vm: BrowserViewModel) {
    val tab by vm.activeTab.collectAsStateWithLifecycle()
    val tabs by vm.tabs.tabs.collectAsStateWithLifecycle()
    val page = tab?.page
    val session = tab?.let { vm.tabs.session(it.id) }
    var typed by remember(tab?.displayUrl) { mutableStateOf(tab?.displayUrl ?: "") }

    Column(Modifier.fillMaxWidth()) {
        OutlinedTextField(
            value = typed,
            onValueChange = { typed = it },
            singleLine = true,
            placeholder = { Text("Search or enter address") },
            modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Go),
            keyboardActions = KeyboardActions(onGo = { vm.go(typed) }),
        )
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = { session?.goBack() }, enabled = page?.canGoBack == true) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
            }
            IconButton(onClick = { session?.goForward() }, enabled = page?.canGoForward == true) {
                Icon(Icons.AutoMirrored.Filled.ArrowForward, contentDescription = "Forward")
            }
            IconButton(onClick = { vm.tabs.reload() }) { Icon(Icons.Filled.Refresh, contentDescription = "Reload") }
            IconButton(onClick = {
                vm.goTo(vm.settingsRepo.current.homepage)
            }) { Icon(Icons.Filled.Home, contentDescription = "Home") }
            Box(Modifier.weight(1f))
            IconButton(onClick = { vm.push(Screen.TabSwitcher) }) { Text("${tabs.size}") }
            IconButton(onClick = {
                vm.push(Screen.Settings)
            }) { Icon(Icons.Filled.MoreVert, contentDescription = "Menu") }
        }
    }
}
