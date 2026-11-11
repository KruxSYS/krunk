package org.krunk.browser.ui.browser

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Bookmarks
import androidx.compose.material.icons.outlined.Computer
import androidx.compose.material.icons.outlined.FindInPage
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.Masks
import androidx.compose.material.icons.outlined.Restore
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.icons.outlined.Share
import androidx.compose.material.icons.outlined.StarBorder
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.launch
import org.krunk.browser.R
import org.krunk.browser.tabs.TabUi
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.Screen
import org.krunk.browser.ui.theme.KrunkColors

/** The ⋮ menu of the toolbar. */
@Composable
fun BrowserMenu(
    expanded: Boolean,
    onDismiss: () -> Unit,
    vm: BrowserViewModel,
    tab: TabUi?,
    onMessage: (String) -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val bookmarked by vm.bookmarkedShown.collectAsStateWithLifecycle()
    val page = tab?.page
    val hasPage = tab != null && !tab.isNewTab && tab.internalPage == null && tab.url.isNotEmpty()
    val canBookmark = vm.canBookmark(tab)
    val closedCount = if (expanded) vm.tabs.closedTabs.value.size else 0
    val added = stringResource(R.string.bookmark_added)
    val removed = stringResource(R.string.bookmark_removed)
    val shareTitle = stringResource(R.string.share_chooser)

    DropdownMenu(expanded = expanded, onDismissRequest = onDismiss) {
        Item(stringResource(R.string.menu_new_tab), Icons.Outlined.Add) {
            onDismiss()
            vm.tabs.openTab("")
        }
        Item(stringResource(R.string.menu_new_private_tab), Icons.Outlined.Masks) {
            onDismiss()
            vm.tabs.openTab("", private = true)
        }
        HorizontalDivider()
        DropdownMenuItem(
            text = {
                Text(stringResource(if (bookmarked) R.string.menu_bookmark_remove else R.string.menu_bookmark_add))
            },
            leadingIcon = {
                Icon(
                    if (bookmarked) Icons.Filled.Star else Icons.Outlined.StarBorder,
                    contentDescription = null,
                    tint = if (bookmarked) KrunkColors.bookmarkStar else MaterialTheme.colorScheme.onSurfaceVariant,
                )
            },
            enabled = canBookmark,
            onClick = {
                onDismiss()
                scope.launch {
                    when (vm.toggleBookmark()) {
                        true -> onMessage(added)
                        false -> onMessage(removed)
                        null -> Unit
                    }
                }
            },
        )
        Item(stringResource(R.string.menu_share), Icons.Outlined.Share, enabled = hasPage) {
            onDismiss()
            if (tab != null) Share.text(context, tab.url, tab.title, shareTitle)
        }
        Item(stringResource(R.string.menu_find), Icons.Outlined.FindInPage, enabled = hasPage) {
            onDismiss()
            vm.openFind()
        }
        DropdownMenuItem(
            text = { Text(stringResource(R.string.menu_desktop_site)) },
            leadingIcon = { Icon(Icons.Outlined.Computer, contentDescription = null) },
            trailingIcon = {
                Checkbox(checked = page?.desktopMode == true, onCheckedChange = null, enabled = hasPage)
            },
            enabled = hasPage,
            onClick = {
                onDismiss()
                if (tab != null) vm.tabs.setDesktopMode(tab.id, page?.desktopMode != true)
            },
        )
        Item(stringResource(R.string.menu_recently_closed), Icons.Outlined.Restore, enabled = closedCount > 0) {
            onDismiss()
            vm.tabs.reopenClosed()
        }
        HorizontalDivider()
        Item(stringResource(R.string.menu_bookmarks), Icons.Outlined.Bookmarks) {
            onDismiss()
            vm.push(Screen.Bookmarks)
        }
        Item(stringResource(R.string.menu_history), Icons.Outlined.History) {
            onDismiss()
            vm.push(Screen.History)
        }
        Item(stringResource(R.string.menu_settings), Icons.Outlined.Settings) {
            onDismiss()
            vm.push(Screen.Settings)
        }
    }
}

@Composable
private fun Item(text: String, icon: ImageVector, enabled: Boolean = true, onClick: () -> Unit) {
    DropdownMenuItem(
        text = { Text(text) },
        leadingIcon = { Icon(icon, contentDescription = null) },
        enabled = enabled,
        onClick = onClick,
        modifier = Modifier,
    )
}
