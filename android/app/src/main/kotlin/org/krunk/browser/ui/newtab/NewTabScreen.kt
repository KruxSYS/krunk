package org.krunk.browser.ui.newtab

import android.graphics.Bitmap
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.PushPin
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.launch
import org.krunk.browser.R
import org.krunk.browser.core.Wallpapers
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.theme.KrunkColors
import org.krunk.browser.ui.theme.LocalKrunkStyle
import org.krunk.browser.ui.theme.ThemeStyles
import org.krunk.ffi.NtpBackgroundMode
import org.krunk.ffi.Tile
import org.krunk.ffi.bookmarkable
import org.krunk.ffi.inputToUrl
import org.krunk.ffi.nowMs

/**
 * Krunk's native new-tab page: a search box, the top sites (pinned shortcuts first, then the
 * most visited by frecency, from the core `tiles` table like Windows), recently closed tabs and
 * the background the user chose (none, a colour or a built-in wallpaper). Embedded in the
 * browser screen for new-tab tabs; light and dark follow the theme.
 */
@Composable
fun NewTabScreen(vm: BrowserViewModel, modifier: Modifier = Modifier) {
    val settings by vm.settings.collectAsStateWithLifecycle()
    val tabList by vm.tabs.tabs.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    // ----- background -----
    val wallpaper = remember(settings.ntpBackground, settings.ntpWallpaper) {
        if (settings.ntpBackground == NtpBackgroundMode.WALLPAPER) Wallpapers.find(settings.ntpWallpaper) else null
    }
    var picture by remember { mutableStateOf<Bitmap?>(null) }
    LaunchedEffect(wallpaper?.id) {
        picture = wallpaper?.let { Wallpapers.load(context.assets, it.id) }
    }
    val scheme = MaterialTheme.colorScheme
    val customColor = if (settings.ntpBackground ==
        NtpBackgroundMode.COLOR
    ) {
        parseHexColor(settings.ntpBackgroundColor)
    } else {
        null
    }
    val background = customColor ?: scheme.background
    val onBackground = when {
        wallpaper != null -> if (Wallpapers.lightText(wallpaper)) Color.White else KrunkColors.fgLight
        customColor != null -> if (customColor.luminance() < 0.4f) Color.White else KrunkColors.fgLight
        else -> scheme.onBackground
    }
    val muted = onBackground.copy(alpha = 0.72f)

    // ----- data -----
    var refresh by remember { mutableIntStateOf(0) }
    val tileCount = settings.ntpColumns.toInt().coerceIn(1, 12) * settings.ntpRows.toInt().coerceIn(1, 6)
    var tiles by remember { mutableStateOf<List<Tile>>(emptyList()) }
    LaunchedEffect(refresh, tileCount, settings.ntpShowTiles, tabList.size) {
        if (settings.ntpShowTiles) {
            tiles = runCatching { vm.core.io { it.topSites(tileCount.toUInt(), nowMs()) } }.getOrDefault(emptyList())
        }
    }
    val closedAll by vm.tabs.closedTabs.collectAsStateWithLifecycle()
    val closed = if (settings.ntpShowShortcuts) closedAll else emptyList()

    var editing by remember { mutableStateOf<ShortcutEdit?>(null) }

    Box(modifier.background(background)) {
        val pic = picture
        if (pic != null && wallpaper != null) {
            Image(
                bitmap = pic.asImageBitmap(),
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxSize(),
            )
            val dim = settings.ntpImageDim.toInt().coerceIn(0, 80) / 100f
            if (dim > 0f) Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = dim)))
        }
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Spacer(Modifier.height(if (settings.ntpShowSearch) 48.dp else 24.dp))
            if (settings.ntpShowSearch) {
                SearchBox(vm, engineName = vm.settingsRepo.searchName)
                Spacer(Modifier.height(28.dp))
            }
            if (settings.ntpShowTiles) {
                TilesGrid(
                    tiles = tiles,
                    columns = settings.ntpColumns.toInt().coerceIn(1, 12),
                    textColor = onBackground,
                    mutedColor = muted,
                    vm = vm,
                    onOpen = { vm.goTo(it.url) },
                    onAdd = { editing = ShortcutEdit(null, "", "") },
                    onEdit = { editing = ShortcutEdit(it.url, it.title, it.url) },
                    onPin = { t ->
                        scope.launch {
                            runCatching { vm.core.io { it.pinSite(t.url, t.title) } }
                            refresh++
                        }
                    },
                    onUnpin = { t ->
                        scope.launch {
                            runCatching { vm.core.io { it.unpinSite(t.url) } }
                            refresh++
                        }
                    },
                    onHide = { t ->
                        scope.launch {
                            runCatching { vm.core.io { if (t.pinned) it.unpinSite(t.url) else it.hideSite(t.url) } }
                            refresh++
                        }
                    },
                )
                Spacer(Modifier.height(24.dp))
            }
            if (settings.ntpShowShortcuts && closed.isNotEmpty()) {
                Text(
                    stringResource(R.string.ntp_recently_closed),
                    style = MaterialTheme.typography.labelLarge,
                    color = muted,
                    modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp),
                )
                LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.fillMaxWidth()) {
                    itemsIndexed(closed) { index, c ->
                        AssistChip(
                            onClick = { vm.tabs.reopenClosed(index) },
                            label = {
                                Text(
                                    c.title.ifBlank { c.url },
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                    modifier = Modifier.widthIn(max = 180.dp),
                                )
                            },
                            leadingIcon = { SiteIcon(c.url, vm.tabs.favicons, size = 18.dp, fallbackText = c.title) },
                        )
                    }
                }
                Spacer(Modifier.height(24.dp))
            }
        }
    }

    editing?.let { edit ->
        ShortcutDialog(
            edit = edit,
            searchTemplate = vm.settingsRepo.searchTemplate,
            onDismiss = { editing = null },
            onSave = { title, url ->
                editing = null
                scope.launch {
                    runCatching {
                        vm.core.io {
                            val old = edit.originalUrl
                            if (old == null) {
                                it.pinSite(url, title)
                            } else if (!it.editTile(old, url, title)) {
                                // Not pinned any more (or a most-visited tile): pin the new one.
                                it.pinSite(url, title)
                            }
                        }
                    }
                    refresh++
                }
            },
        )
    }
}

/** The search box: focusing it opens the address bar; typing here and Go also works. */
@Composable
private fun SearchBox(vm: BrowserViewModel, engineName: String) {
    var text by remember { mutableStateOf("") }
    val hint = if (engineName.isBlank()) {
        stringResource(R.string.ntp_search_hint_generic)
    } else {
        stringResource(R.string.ntp_search_hint, engineName)
    }
    // The search box and the tiles take the style's corners, like the address field.
    val boxShape = RoundedCornerShape(ThemeStyles.fieldCornerDp(LocalKrunkStyle.current.shape, SEARCH_BOX_DP).dp)
    Surface(
        shape = boxShape,
        color = MaterialTheme.colorScheme.surfaceContainer,
        tonalElevation = 2.dp,
        shadowElevation = 2.dp,
        modifier = Modifier.fillMaxWidth().widthIn(max = 560.dp),
    ) {
        OutlinedTextField(
            value = text,
            onValueChange = { text = it },
            singleLine = true,
            placeholder = { Text(hint, maxLines = 1, overflow = TextOverflow.Ellipsis) },
            leadingIcon = { Icon(Icons.Filled.Search, contentDescription = stringResource(R.string.action_search)) },
            shape = boxShape,
            colors = TextFieldDefaults.colors(
                focusedIndicatorColor = Color.Transparent,
                unfocusedIndicatorColor = Color.Transparent,
                focusedContainerColor = Color.Transparent,
                unfocusedContainerColor = Color.Transparent,
            ),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Go),
            keyboardActions = KeyboardActions(onGo = { if (text.isNotBlank()) vm.go(text) }),
            modifier = Modifier
                .fillMaxWidth()
                .onFocusChanged { if (it.isFocused) vm.startEditing(text) },
        )
    }
}

private const val TILE_WIDTH_DP = 88

@Composable
private fun TilesGrid(
    tiles: List<Tile>,
    columns: Int,
    textColor: Color,
    mutedColor: Color,
    vm: BrowserViewModel,
    onOpen: (Tile) -> Unit,
    onAdd: () -> Unit,
    onEdit: (Tile) -> Unit,
    onPin: (Tile) -> Unit,
    onUnpin: (Tile) -> Unit,
    onHide: (Tile) -> Unit,
) {
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val fit = (maxWidth / TILE_WIDTH_DP.dp).toInt().coerceAtLeast(2)
        val cols = columns.coerceAtMost(fit)
        val cells: List<Tile?> = tiles + listOf<Tile?>(null) // trailing "add" tile
        Column(verticalArrangement = Arrangement.spacedBy(12.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            if (tiles.isEmpty()) {
                Text(
                    stringResource(R.string.ntp_empty_tiles),
                    color = mutedColor,
                    style = MaterialTheme.typography.bodyMedium,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(bottom = 4.dp),
                )
            }
            for (row in cells.chunked(cols)) {
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    for (t in row) {
                        if (t == null) {
                            AddTile(onAdd, textColor, mutedColor)
                        } else {
                            SiteTile(t, textColor, vm, onOpen, onEdit, onPin, onUnpin, onHide)
                        }
                    }
                    // Keep the last row left-aligned with the others.
                    repeat(cols - row.size) { Spacer(Modifier.width(TILE_WIDTH_DP.dp)) }
                }
            }
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun SiteTile(
    tile: Tile,
    textColor: Color,
    vm: BrowserViewModel,
    onOpen: (Tile) -> Unit,
    onEdit: (Tile) -> Unit,
    onPin: (Tile) -> Unit,
    onUnpin: (Tile) -> Unit,
    onHide: (Tile) -> Unit,
) {
    var menu by remember { mutableStateOf(false) }
    val label = tile.title.ifBlank { tile.url }
    Box(Modifier.width(TILE_WIDTH_DP.dp)) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(12.dp))
                .combinedClickable(onClick = { onOpen(tile) }, onLongClick = { menu = true })
                .padding(vertical = 8.dp, horizontal = 4.dp)
                .semantics { contentDescription = label },
        ) {
            Box {
                Surface(
                    shape = RoundedCornerShape(ThemeStyles.tileCornerDp(LocalKrunkStyle.current.shape).dp),
                    color = MaterialTheme.colorScheme.surfaceContainer,
                    tonalElevation = 1.dp,
                    modifier = Modifier.size(56.dp),
                ) {
                    Box(contentAlignment = Alignment.Center) {
                        SiteIcon(tile.url, vm.tabs.favicons, size = 32.dp, fallbackText = tile.title)
                    }
                }
                if (tile.pinned) {
                    Icon(
                        Icons.Filled.PushPin,
                        contentDescription = stringResource(R.string.ntp_pinned),
                        tint = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.size(16.dp).align(Alignment.TopEnd),
                    )
                }
            }
            Spacer(Modifier.height(6.dp))
            Text(
                label,
                color = textColor,
                style = MaterialTheme.typography.labelMedium,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                textAlign = TextAlign.Center,
            )
        }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            if (tile.pinned) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.ntp_unpin)) },
                    onClick = {
                        menu = false
                        onUnpin(tile)
                    },
                )
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.action_edit)) },
                    onClick = {
                        menu = false
                        onEdit(tile)
                    },
                )
            } else {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.ntp_pin)) },
                    onClick = {
                        menu = false
                        onPin(tile)
                    },
                )
            }
            DropdownMenuItem(
                text = { Text(stringResource(R.string.ntp_remove_tile)) },
                onClick = {
                    menu = false
                    onHide(tile)
                },
            )
        }
    }
}

@Composable
private fun AddTile(onAdd: () -> Unit, textColor: Color, mutedColor: Color) {
    val label = stringResource(R.string.ntp_add_shortcut)
    Column(
        horizontalAlignment = Alignment.CenterHorizontally,
        modifier = Modifier
            .width(TILE_WIDTH_DP.dp)
            .clip(RoundedCornerShape(12.dp))
            .combinedClickable(onClick = onAdd)
            .padding(vertical = 8.dp, horizontal = 4.dp),
    ) {
        Surface(
            shape = RoundedCornerShape(ThemeStyles.tileCornerDp(LocalKrunkStyle.current.shape).dp),
            color = MaterialTheme.colorScheme.surfaceContainer.copy(alpha = 0.6f),
            modifier = Modifier.size(56.dp),
        ) {
            Box(contentAlignment = Alignment.Center) {
                Icon(Icons.Filled.Add, contentDescription = label, tint = mutedColor)
            }
        }
        Spacer(Modifier.height(6.dp))
        // Centred on both axes in the label's line box, also when it wraps to two lines.
        Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
            Text(
                label,
                color = textColor.copy(alpha = 0.8f),
                style = MaterialTheme.typography.labelMedium,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

/** What the shortcut dialog edits: `originalUrl == null` adds a new pinned tile. */
data class ShortcutEdit(val originalUrl: String?, val title: String, val url: String)

@Composable
private fun ShortcutDialog(
    edit: ShortcutEdit,
    searchTemplate: String,
    onDismiss: () -> Unit,
    onSave: (title: String, url: String) -> Unit,
) {
    var title by remember { mutableStateOf(edit.title) }
    var url by remember { mutableStateOf(edit.url) }
    var error by remember { mutableStateOf(false) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = {
            Text(
                stringResource(
                    if (edit.originalUrl ==
                        null
                    ) {
                        R.string.ntp_add_shortcut
                    } else {
                        R.string.ntp_edit_shortcut
                    },
                ),
            )
        },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = title,
                    onValueChange = { title = it },
                    label = { Text(stringResource(R.string.field_title)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = url,
                    onValueChange = {
                        url = it
                        error = false
                    },
                    label = { Text(stringResource(R.string.field_url)) },
                    singleLine = true,
                    isError = error,
                    supportingText = if (error) {
                        { Text(stringResource(R.string.ntp_url_invalid)) }
                    } else {
                        null
                    },
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
        confirmButton = {
            TextButton(onClick = {
                val normalised = inputToUrl(url.trim(), searchTemplate)
                if (url.isBlank() || !bookmarkable(normalised)) {
                    error = true
                } else {
                    onSave(title.trim().ifEmpty { normalised }, normalised)
                }
            }) { Text(stringResource(R.string.action_save)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

/** `#rrggbb` to a colour, or `null` for anything else (the core validated the stored value). */
fun parseHexColor(text: String): Color? {
    if (text.length != 7 || text[0] != '#') return null
    val rgb = text.substring(1).toLongOrNull(16) ?: return null
    return Color((0xFF000000L or rgb).toInt())
}

/** The search box's height (Material's text field), for its style-dependent corners. */
private const val SEARCH_BOX_DP = 56
