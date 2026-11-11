package org.krunk.browser.ui.browser

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.border
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.ArrowForward
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.outlined.Home
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.krunk.browser.R
import org.krunk.browser.tabs.TabUi
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.ffi.newTabUrl

/** The toolbar row's height (address pill plus padding); the page reserves this much. */
val ToolbarHeight = 56.dp

/**
 * Back, forward, home (when there is room), the address pill with reload/stop, the tab
 * counter and the menu. One row, [ToolbarHeight] tall, every button at least 48 dp.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun BrowserToolbar(
    vm: BrowserViewModel,
    tab: TabUi?,
    tabCount: Int,
    onOpenTabs: () -> Unit,
    onMessage: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val page = tab?.page
    val session = tab?.let { vm.tabs.session(it.id) }
    val haptic = LocalHapticFeedback.current
    val widthDp = with(LocalDensity.current) { LocalWindowInfo.current.containerSize.width.toDp() }
    val showHome = widthDp >= 400.dp
    var menuOpen by remember { mutableStateOf(false) }
    val placeholder = stringResource(R.string.address_placeholder, vm.settingsRepo.searchName)

    Surface(
        modifier = modifier.fillMaxWidth().height(ToolbarHeight),
        color = MaterialTheme.colorScheme.surfaceContainer,
    ) {
        Row(Modifier.padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = { session?.goBack() }, enabled = page?.canGoBack == true) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.toolbar_back))
            }
            IconButton(onClick = { session?.goForward() }, enabled = page?.canGoForward == true) {
                Icon(
                    Icons.AutoMirrored.Filled.ArrowForward,
                    contentDescription = stringResource(R.string.toolbar_forward),
                )
            }
            if (showHome) {
                IconButton(onClick = { vm.goTo(vm.settingsRepo.current.homepage.ifBlank { newTabUrl() }) }) {
                    Icon(Icons.Outlined.Home, contentDescription = stringResource(R.string.toolbar_home))
                }
            }
            AddressPill(
                tab = tab,
                placeholder = placeholder,
                onClick = { vm.startEditing(if (tab == null || tab.isNewTab) "" else tab.displayUrl) },
                modifier = Modifier.weight(1f).padding(horizontal = 4.dp),
            ) {
                if (tab != null && !tab.isNewTab && session != null) {
                    if (page?.loading == true) {
                        IconButton(onClick = { session.stop() }, modifier = Modifier.size(40.dp)) {
                            Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.toolbar_stop))
                        }
                    } else {
                        IconButton(onClick = { vm.tabs.reload() }, modifier = Modifier.size(40.dp)) {
                            Icon(Icons.Filled.Refresh, contentDescription = stringResource(R.string.toolbar_reload))
                        }
                    }
                }
            }
            TabCounter(
                count = tabCount,
                onClick = onOpenTabs,
                onLongClick = {
                    haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                    vm.tabs.openTab("", private = tab?.private == true)
                },
            )
            Box {
                IconButton(onClick = { menuOpen = true }) {
                    Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.toolbar_menu))
                }
                BrowserMenu(
                    expanded = menuOpen,
                    onDismiss = { menuOpen = false },
                    vm = vm,
                    tab = tab,
                    onMessage = onMessage,
                )
            }
        }
    }
}

/** A rounded square with the number of open tabs, like Firefox's. Long-press opens a new tab. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun TabCounter(count: Int, onClick: () -> Unit, onLongClick: () -> Unit) {
    val label = stringResource(R.string.toolbar_tabs, count)
    val color = MaterialTheme.colorScheme.onSurface
    Box(
        Modifier
            .size(48.dp)
            .clip(CircleShape)
            .combinedClickable(onClick = onClick, onLongClick = onLongClick)
            .semantics {
                role = Role.Button
                contentDescription = label
            },
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier.size(24.dp).border(2.dp, color, RoundedCornerShape(5.dp)),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                text = if (count > 99) "∞" else count.toString(),
                color = color,
                fontSize = if (count > 9) 11.sp else 13.sp,
                fontWeight = FontWeight.Bold,
                maxLines = 1,
            )
        }
    }
}
