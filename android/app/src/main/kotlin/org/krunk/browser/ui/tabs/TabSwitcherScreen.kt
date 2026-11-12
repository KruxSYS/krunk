package org.krunk.browser.ui.tabs

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Bedtime
import androidx.compose.material.icons.filled.ClearAll
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.History
import androidx.compose.material.icons.filled.Public
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Tab
import androidx.compose.material3.TabRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.krunk.browser.R
import org.krunk.browser.tabs.TabUi
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.theme.CardMark
import org.krunk.browser.ui.theme.KrunkTheme
import org.krunk.browser.ui.theme.LocalKrunkStyle
import org.krunk.browser.ui.theme.ThemeStyles
import org.krunk.ffi.hostOf

/** Grid columns for the switcher: phones two, sw600dp tablets three, wide tablets four. */
fun switcherColumns(widthDp: Int): Int = when {
    widthDp >= 840 -> 4
    widthDp >= 600 -> 3
    else -> 2
}

/** Case-insensitive match of the search text against title and URL. */
fun tabMatches(tab: TabUi, query: String): Boolean {
    val q = query.trim()
    if (q.isEmpty()) return true
    return tab.title.contains(q, ignoreCase = true) || tab.displayUrl.contains(q, ignoreCase = true) ||
        tab.url.contains(q, ignoreCase = true)
}

/**
 * The tab switcher: a grid of cards (thumbnail or favicon placeholder, title, close), two
 * sections (normal and Private, the latter in the private theme), search, close all, recently
 * closed and a new-tab button. Swiping a card sideways closes it; a long press opens its menu
 * (Duplicate, Close other tabs, Close tab).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TabSwitcherScreen(vm: BrowserViewModel) {
    val tabs by vm.tabs.tabs.collectAsStateWithLifecycle()
    val activeId by vm.tabs.activeId.collectAsStateWithLifecycle()
    val settings by vm.settings.collectAsStateWithLifecycle()
    val haptics = LocalHapticFeedback.current

    var privateSection by rememberSaveable { mutableStateOf(tabs.firstOrNull { it.id == activeId }?.private == true) }
    var query by rememberSaveable { mutableStateOf("") }
    var showClosed by rememberSaveable { mutableStateOf(false) }
    var confirmCloseAll by remember { mutableStateOf(false) }

    val normalCount = tabs.count { !it.private }
    val privateCount = tabs.size - normalCount
    val section = tabs.filter { it.private == privateSection }
    val shown = if (query.isBlank()) section else section.filter { tabMatches(it, query) }
    val columns = switcherColumns(LocalConfiguration.current.screenWidthDp)

    val select: (TabUi) -> Unit = { tab ->
        vm.tabs.activate(tab.id)
        vm.showBrowser()
    }
    val close: (TabUi) -> Unit = { tab ->
        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
        vm.tabs.close(tab.id)
    }

    KrunkTheme(mode = settings.theme, privateMode = privateSection, appearance = settings.appearance) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            Scaffold(
                containerColor = MaterialTheme.colorScheme.background,
                topBar = {
                    TopAppBar(
                        title = { Text(stringResource(R.string.tabs_title)) },
                        navigationIcon = {
                            IconButton(onClick = { vm.pop() }) {
                                Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.tabs_back))
                            }
                        },
                        actions = {
                            IconButton(onClick = { showClosed = true }) {
                                Icon(Icons.Filled.History, stringResource(R.string.tabs_recently_closed))
                            }
                            IconButton(onClick = { confirmCloseAll = true }, enabled = section.isNotEmpty()) {
                                Icon(
                                    Icons.Filled.ClearAll,
                                    stringResource(
                                        if (privateSection) {
                                            R.string.tabs_close_all_private
                                        } else {
                                            R.string.tabs_close_all
                                        },
                                    ),
                                )
                            }
                        },
                    )
                },
                floatingActionButton = {
                    val label = stringResource(
                        if (privateSection) R.string.tabs_new_private_tab else R.string.tabs_new_tab,
                    )
                    ExtendedFloatingActionButton(
                        onClick = {
                            vm.tabs.openTab(private = privateSection)
                            vm.showBrowser()
                        },
                        icon = { Icon(Icons.Filled.Add, contentDescription = null) },
                        text = { Text(label) },
                        modifier = Modifier.semantics { contentDescription = label },
                    )
                },
            ) { padding ->
                Column(Modifier.fillMaxSize().padding(padding)) {
                    TabRow(selectedTabIndex = if (privateSection) 1 else 0) {
                        Tab(
                            selected = !privateSection,
                            onClick = { privateSection = false },
                            text = {
                                Text(
                                    stringResource(
                                        R.string.tabs_section_count,
                                        stringResource(R.string.tabs_section_normal),
                                        normalCount,
                                    ),
                                )
                            },
                        )
                        Tab(
                            selected = privateSection,
                            onClick = { privateSection = true },
                            text = {
                                Text(
                                    stringResource(
                                        R.string.tabs_section_count,
                                        stringResource(R.string.tabs_section_private),
                                        privateCount,
                                    ),
                                )
                            },
                        )
                    }
                    OutlinedTextField(
                        value = query,
                        onValueChange = { query = it },
                        singleLine = true,
                        placeholder = { Text(stringResource(R.string.tabs_search_hint)) },
                        leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null) },
                        trailingIcon = {
                            if (query.isNotEmpty()) {
                                IconButton(onClick = { query = "" }) {
                                    Icon(Icons.Filled.Close, stringResource(R.string.tabs_search_clear))
                                }
                            }
                        },
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
                    )
                    if (shown.isEmpty()) {
                        EmptyState(privateSection, query)
                    } else {
                        LazyVerticalGrid(
                            columns = GridCells.Fixed(columns),
                            contentPadding = PaddingValues(start = 12.dp, end = 12.dp, top = 4.dp, bottom = 96.dp),
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                            verticalArrangement = Arrangement.spacedBy(12.dp),
                            modifier = Modifier.fillMaxSize(),
                        ) {
                            items(shown, key = { it.id.toLong() }) { tab ->
                                TabCard(
                                    tab,
                                    active = tab.id == activeId,
                                    // "Other tabs" are the others of this list (normal or private).
                                    canCloseOthers = section.size > 1,
                                    onSelect = { select(tab) },
                                    onClose = { close(tab) },
                                    onDuplicate = { vm.tabs.duplicate(tab.id) },
                                    onCloseOthers = {
                                        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                                        vm.tabs.closeOthers(tab.id)
                                    },
                                )
                            }
                        }
                    }
                }
            }
            if (showClosed) RecentlyClosedSheet(vm) { showClosed = false }
            if (confirmCloseAll) {
                CloseAllDialog(privateSection, section.size, onDismiss = { confirmCloseAll = false }) {
                    confirmCloseAll = false
                    haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                    vm.tabs.closeAll(private = privateSection)
                }
            }
        }
    }
}

@Composable
private fun EmptyState(private: Boolean, query: String) {
    val text = when {
        query.isNotBlank() -> stringResource(R.string.tabs_empty_search, query.trim())
        private -> stringResource(R.string.tabs_empty_private)
        else -> stringResource(R.string.tabs_empty)
    }
    Box(Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
        Text(
            text,
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = androidx.compose.ui.text.style.TextAlign.Center,
        )
    }
}

@Composable
private fun CloseAllDialog(private: Boolean, count: Int, onDismiss: () -> Unit, onConfirm: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.tabs_close_all_title)) },
        text = {
            Text(
                if (private) {
                    pluralStringResource(R.plurals.tabs_close_all_private_text, count, count)
                } else {
                    pluralStringResource(R.plurals.tabs_close_all_text, count, count)
                },
            )
        },
        confirmButton = { TextButton(onClick = onConfirm) { Text(stringResource(R.string.tabs_close)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.tabs_cancel)) } },
    )
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun TabCard(
    tab: TabUi,
    active: Boolean,
    canCloseOthers: Boolean,
    onSelect: () -> Unit,
    onClose: () -> Unit,
    onDuplicate: () -> Unit,
    onCloseOthers: () -> Unit,
) {
    val title = tab.title.ifBlank {
        if (tab.isNewTab || tab.displayUrl.isBlank()) {
            stringResource(R.string.tabs_untitled)
        } else {
            hostOf(tab.url) ?: tab.displayUrl
        }
    }
    val description = stringResource(
        when {
            active -> R.string.tabs_card_description_active
            tab.asleep -> R.string.tabs_card_description_asleep
            else -> R.string.tabs_card_description
        },
        title,
    )
    var menu by remember { mutableStateOf(false) }
    val actions = tabCardActions(tab.private, canCloseOthers, onDuplicate, onCloseOthers, onClose)
    val menuLabel = stringResource(R.string.tabs_menu_open)
    val style = LocalKrunkStyle.current
    val shape = style.cardShape
    // How the theme's style marks the active tab's card (fill, underline, outline or the title).
    val mark = ThemeStyles.cardMark(style.highlight, active)
    val dismissState = rememberSwipeToDismissBoxState()
    SwipeToDismissBox(
        state = dismissState,
        backgroundContent = {},
        onDismiss = { value -> if (value != SwipeToDismissBoxValue.Settled) onClose() },
    ) {
        Card(
            modifier = Modifier
                .fillMaxWidth()
                .aspectRatio(0.72f)
                // A tap opens the tab; a long press opens the card's menu. TalkBack gets the
                // menu's entries as actions on the card as well.
                .clip(shape)
                .combinedClickable(
                    onLongClickLabel = menuLabel,
                    onLongClick = { menu = true },
                    onClick = onSelect,
                )
                .semantics {
                    contentDescription = description
                    customActions = actions.forAccessibility()
                },
            shape = shape,
            border = if (mark == CardMark.OUTLINE) BorderStroke(2.dp, MaterialTheme.colorScheme.primary) else null,
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
        ) {
            Column(Modifier.fillMaxSize()) {
                val titleColor = when (mark) {
                    CardMark.FILL -> style.onAccent
                    CardMark.TITLE, CardMark.UNDERLINE -> MaterialTheme.colorScheme.primary
                    else -> LocalContentColor.current
                }
                Row(
                    Modifier
                        .fillMaxWidth()
                        .then(if (mark == CardMark.FILL) Modifier.background(style.accent) else Modifier)
                        .padding(start = 10.dp, top = 2.dp, bottom = 2.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Favicon(tab, 16.dp)
                    Spacer(Modifier.width(8.dp))
                    Text(
                        title,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        style = MaterialTheme.typography.labelLarge,
                        color = titleColor,
                        modifier = Modifier.weight(1f),
                    )
                    if (tab.asleep) {
                        Icon(
                            Icons.Filled.Bedtime,
                            contentDescription = stringResource(R.string.tabs_asleep),
                            tint = if (mark ==
                                CardMark.FILL
                            ) {
                                style.onAccent
                            } else {
                                MaterialTheme.colorScheme.onSurfaceVariant
                            },
                            modifier = Modifier.size(14.dp),
                        )
                    }
                    IconButton(onClick = onClose) {
                        Icon(
                            Icons.Filled.Close,
                            contentDescription = stringResource(R.string.tabs_close_tab, title),
                            tint = if (mark == CardMark.FILL) style.onAccent else LocalContentColor.current,
                        )
                    }
                }
                if (mark == CardMark.UNDERLINE) {
                    Box(Modifier.fillMaxWidth().height(3.dp).background(MaterialTheme.colorScheme.primary))
                }
                Box(
                    Modifier
                        .fillMaxWidth()
                        .weight(1f)
                        .background(MaterialTheme.colorScheme.surfaceContainerLowest),
                ) {
                    val thumb = tab.thumbnail
                    if (thumb != null && !thumb.isRecycled) {
                        Image(
                            bitmap = thumb.asImageBitmap(),
                            contentDescription = null,
                            contentScale = ContentScale.Crop,
                            alignment = Alignment.TopCenter,
                            modifier = Modifier.fillMaxSize(),
                        )
                    } else {
                        Column(
                            Modifier.fillMaxSize().padding(12.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.Center,
                        ) {
                            Favicon(tab, 40.dp)
                            Spacer(Modifier.height(8.dp))
                            Text(
                                if (tab.isNewTab) {
                                    stringResource(R.string.tabs_untitled)
                                } else {
                                    (
                                        hostOf(tab.url)
                                            ?: tab.displayUrl
                                        )
                                },
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                            )
                        }
                    }
                }
            }
        }
        TabCardMenu(menu, actions) { menu = false }
    }
}

@Composable
private fun Favicon(tab: TabUi, size: androidx.compose.ui.unit.Dp) {
    val icon = tab.favicon
    if (icon != null && !icon.isRecycled) {
        Image(icon.asImageBitmap(), contentDescription = null, modifier = Modifier.size(size))
    } else {
        Icon(
            Icons.Filled.Public,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(size),
        )
    }
}
