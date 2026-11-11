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
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.krunk.browser.R
import org.krunk.browser.engine.blocker.AllowList
import org.krunk.browser.tabs.TabUi
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.ffi.NavItem
import org.krunk.ffi.allowEntryFor
import org.krunk.ffi.navMenu
import org.krunk.ffi.newTabUrl

/** The toolbar row's height (address pill plus padding); the page reserves this much. */
val ToolbarHeight = 56.dp

/**
 * The address pill (with the blocker's shield and reload/stop inside it), the tab counter and
 * the menu; from tablet width on also Home. Back and Forward are in the menu's first row and on
 * the system back gesture. One row, [ToolbarHeight] tall, every target 48 dp ([toolbarLayout]).
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
    val layout = toolbarLayout(widthDp.value.toInt())
    var menuOpen by remember { mutableStateOf(false) }
    var sheetOpen by remember { mutableStateOf(false) }
    var historyList by remember { mutableStateOf<List<NavItem>?>(null) }
    val placeholder = stringResource(R.string.address_placeholder, vm.settingsRepo.searchName)
    val goHome = { vm.goTo(vm.settingsRepo.current.homepage.ifBlank { newTabUrl() }) }

    // The shield: the blocker's reading for this tab only.
    val status by vm.blockerStatus.collectAsStateWithLifecycle()
    val shields by vm.shields.collectAsStateWithLifecycle()
    val allowHosts by vm.allowHosts.collectAsStateWithLifecycle()
    val isPage = tab != null && !tab.isNewTab && tab.internalPage == null && session != null
    val pageUrl = if (isPage) tab?.url.orEmpty() else ""
    val allowedEntry = remember(allowHosts, pageUrl) {
        AllowList.siteOf(pageUrl)?.let { allowEntryFor(allowHosts, it) }
    }
    val shield = shieldUi(
        url = pageUrl,
        isPage = isPage,
        status = status,
        shield = tab?.let { shields[it.id] },
        allowedEntry = allowedEntry,
        startedUnprotected = vm.engine.blocker.startedUnprotected,
    )

    Surface(
        modifier = modifier.fillMaxWidth().height(ToolbarHeight),
        color = MaterialTheme.colorScheme.surfaceContainer,
    ) {
        Row(
            Modifier.padding(horizontal = ToolbarLayout.ROW_PADDING_DP.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (layout.showHome) {
                IconButton(onClick = goHome) {
                    Icon(Icons.Outlined.Home, contentDescription = stringResource(R.string.toolbar_home))
                }
            }
            AddressPill(
                tab = tab,
                placeholder = placeholder,
                onClick = { vm.startEditing(if (tab == null || tab.isNewTab) "" else tab.displayUrl) },
                modifier = Modifier.weight(1f).padding(horizontal = ToolbarLayout.PILL_PADDING_DP.dp),
                compact = !layout.showNotSecureWords,
                hasLeading = shield.mode != ShieldMode.HIDDEN,
                leading = {
                    if (shield.mode != ShieldMode.HIDDEN) ShieldButton(shield) { sheetOpen = true }
                },
            ) {
                if (tab != null && !tab.isNewTab && session != null) {
                    // 48 dp targets inside the 40 dp pill (see AddressPill).
                    if (page?.loading == true) {
                        IconButton(onClick = { session.stop() }, modifier = Modifier.size(ToolbarLayout.BUTTON_DP.dp)) {
                            Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.toolbar_stop))
                        }
                    } else {
                        IconButton(onClick = {
                            vm.tabs.reload()
                        }, modifier = Modifier.size(ToolbarLayout.BUTTON_DP.dp)) {
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
                    nav = navRow(page, hasSession = session != null, toolbarShowsHome = layout.showHome),
                    onHome = goHome,
                    onHistory = { forward ->
                        val s = session
                        if (s != null) {
                            val entries = s.historyEntries.map { org.krunk.ffi.NavEntry(it.url, it.title) }
                            historyList = navMenu(entries, s.historyIndex.toUInt(), forward).takeIf { it.isNotEmpty() }
                        }
                    },
                    onMessage = onMessage,
                )
            }
        }
    }

    if (sheetOpen && tab != null) {
        ShieldSheet(
            ui = shield,
            site = AllowList.siteOf(pageUrl).orEmpty(),
            privateTab = tab.private,
            onAllow = { allowed -> vm.setSiteAllowed(tab.id, pageUrl, allowed) },
            onReload = { vm.tabs.reload(tab.id) },
            onSettings = { vm.push(org.krunk.browser.ui.Screen.Settings) },
            onDismiss = { sheetOpen = false },
        )
    }
    historyList?.let { items ->
        HistoryListDialog(
            items = items,
            onPick = { item ->
                historyList = null
                session?.goToHistory(item.index.toInt())
            },
            onDismiss = { historyList = null },
        )
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
            .size(ToolbarLayout.BUTTON_DP.dp)
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
