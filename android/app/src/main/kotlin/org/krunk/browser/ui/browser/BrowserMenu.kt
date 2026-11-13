package org.krunk.browser.ui.browser

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.ArrowForward
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Bookmarks
import androidx.compose.material.icons.outlined.Computer
import androidx.compose.material.icons.outlined.FindInPage
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.Home
import androidx.compose.material.icons.outlined.Key
import androidx.compose.material.icons.outlined.Masks
import androidx.compose.material.icons.outlined.Password
import androidx.compose.material.icons.outlined.Restore
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.icons.outlined.Share
import androidx.compose.material.icons.outlined.StarBorder
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.toggleableState
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.launch
import org.krunk.browser.R
import org.krunk.browser.engine.logins.LoginVault
import org.krunk.browser.tabs.TabUi
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.Screen
import org.krunk.browser.ui.theme.KrunkColors
import org.krunk.ffi.NavItem

/** The ⋮ menu of the toolbar. */
@Composable
fun BrowserMenu(
    expanded: Boolean,
    onDismiss: () -> Unit,
    vm: BrowserViewModel,
    tab: TabUi?,
    nav: NavRow,
    onHome: () -> Unit,
    onHistory: (forward: Boolean) -> Unit,
    onMessage: (String) -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val bookmarked by vm.bookmarkedShown.collectAsStateWithLifecycle()
    val page = tab?.page
    val hasPage = tab != null && !tab.isNewTab && tab.internalPage == null && tab.url.isNotEmpty()
    val canBookmark = vm.canBookmark(tab)
    val closedTabs by vm.tabs.closedTabs.collectAsStateWithLifecycle()
    val closedCount = closedTabs.size
    val added = stringResource(R.string.bookmark_added)
    val removed = stringResource(R.string.bookmark_removed)
    val shareTitle = stringResource(R.string.share_chooser)

    DropdownMenu(expanded = expanded, onDismissRequest = onDismiss) {
        // Back, Forward and (on phones) Home: icons, enabled from the page's history state.
        // A long press on Back or Forward opens the list of pages to jump to.
        Row(Modifier.padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            NavIcon(
                Icons.AutoMirrored.Filled.ArrowBack,
                stringResource(R.string.toolbar_back),
                enabled = nav.backEnabled,
                onLongClick = if (nav.backHasHistory) {
                    {
                        onDismiss()
                        onHistory(false)
                    }
                } else {
                    null
                },
            ) {
                onDismiss()
                vm.tabs.back()
            }
            NavIcon(
                Icons.AutoMirrored.Filled.ArrowForward,
                stringResource(R.string.toolbar_forward),
                enabled = nav.forwardEnabled,
                onLongClick = if (nav.forwardHasHistory) {
                    {
                        onDismiss()
                        onHistory(true)
                    }
                } else {
                    null
                },
            ) {
                onDismiss()
                tab?.let { vm.tabs.session(it.id)?.goForward() }
            }
            if (nav.showHome) {
                NavIcon(
                    Icons.Outlined.Home,
                    stringResource(R.string.toolbar_home),
                    enabled = true,
                    onLongClick = null,
                ) {
                    onDismiss()
                    onHome()
                }
            }
        }
        HorizontalDivider()
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
            // The row is the check box: TalkBack (and a UI dump) get "checked" / "not checked".
            modifier = Modifier.semantics {
                role = Role.Checkbox
                toggleableState = ToggleableState(page?.desktopMode == true)
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
        Item(stringResource(R.string.menu_passwords), Icons.Outlined.Key) {
            onDismiss()
            vm.push(Screen.Vault)
        }
        // The way to a saved login where Krunk does not offer one by itself: a private tab, or
        // a page whose picker was closed. The next tap on a sign-in field brings the picker.
        val loginUi = LocalLoginUi.current
        val vaultState = loginUi?.vault?.state()
        if (tab != null && (vaultState == LoginVault.State.UNLOCKED || vaultState == LoginVault.State.LOCKED)) {
            val armed = stringResource(R.string.login_fill_armed)
            val notHere = stringResource(R.string.login_fill_not_here)
            Item(stringResource(R.string.menu_fill_from_vault), Icons.Outlined.Password) {
                onDismiss()
                onMessage(if (vm.tabs.session(tab.id)?.armFill() == true) armed else notHere)
            }
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

/** One icon of the menu's first row: a 48 dp target, dimmed and inert when disabled. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun NavIcon(
    icon: ImageVector,
    label: String,
    enabled: Boolean,
    onLongClick: (() -> Unit)?,
    onClick: () -> Unit,
) {
    val haptic = LocalHapticFeedback.current
    Box(
        Modifier
            .padding(horizontal = 4.dp)
            .size(48.dp)
            .clip(CircleShape)
            .combinedClickable(
                enabled = enabled,
                role = Role.Button,
                onClick = onClick,
                onLongClick = onLongClick?.let { open ->
                    {
                        haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                        open()
                    }
                },
            )
            .semantics { contentDescription = label },
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            icon,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurface.copy(alpha = if (enabled) 1f else DISABLED_ALPHA),
        )
    }
}

private const val DISABLED_ALPHA = 0.38f

/** The pages Back or Forward can jump to (long press), nearest first. */
@Composable
fun HistoryListDialog(items: List<NavItem>, onPick: (NavItem) -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.history_list_title)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                for (item in items) {
                    ListItem(
                        headlineContent = { Text(item.label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                        supportingContent = if (item.label != item.url) {
                            { Text(item.url, maxLines = 1, overflow = TextOverflow.StartEllipsis) }
                        } else {
                            null
                        },
                        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                        modifier = Modifier.clickable { onPick(item) },
                    )
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text(stringResource(android.R.string.cancel)) } },
    )
}
