package org.krunk.browser.ui.bookmarks

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.CreateNewFolder
import androidx.compose.material.icons.filled.Folder
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.krunk.browser.R
import org.krunk.browser.core.BookmarksRepository
import org.krunk.browser.core.FolderChoice
import org.krunk.browser.core.readCapped
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.newtab.SiteIcon
import org.krunk.ffi.Bookmark

/** The folder ids from the top level down to the open folder, as saved state. */
private val FolderStackSaver = listSaver<List<Long>, Long>(save = { it.toList() }, restore = { it })

/**
 * The bookmark manager: the two roots at the top level, folders below, search across all
 * bookmarks, edit/move/delete, new folders, and Netscape HTML import/export through the Android
 * file picker (the same format the Windows build reads and writes).
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
fun BookmarksScreen(vm: BrowserViewModel) {
    val repo = remember(vm) { BookmarksRepository(vm.core.store) }
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val res = LocalResources.current
    val snackbar = remember { SnackbarHostState() }

    // null = the top level (the two roots); otherwise the open folder and the way back to it.
    // The open folder and the search are kept while another screen covers this one.
    var stack by rememberSaveable(stateSaver = FolderStackSaver) { mutableStateOf<List<Long>>(emptyList()) }
    val folderId = stack.lastOrNull()
    var folderItem by remember { mutableStateOf<Bookmark?>(null) }
    var items by remember { mutableStateOf<List<Bookmark>>(emptyList()) }
    var searching by rememberSaveable { mutableStateOf(false) }
    var query by rememberSaveable { mutableStateOf("") }
    var refresh by remember { mutableIntStateOf(0) }

    var editing by remember { mutableStateOf<Long?>(null) }
    var moving by remember { mutableStateOf<Bookmark?>(null) }
    var folders by remember { mutableStateOf<List<FolderChoice>>(emptyList()) }
    var newFolder by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf<Bookmark?>(null) }
    var menu by remember { mutableStateOf(false) }

    LaunchedEffect(folderId, query, refresh, searching) {
        val result = runCatching {
            when {
                searching && query.isNotBlank() -> repo.search(query.trim())
                folderId == null -> repo.rootItems()
                else -> repo.children(folderId)
            }
        }
        items = result.getOrDefault(emptyList())
        folderItem = folderId?.let { runCatching { repo.get(it) }.getOrNull() }
    }

    fun goUp(): Boolean {
        if (searching) {
            searching = false
            query = ""
            return true
        }
        if (stack.isEmpty()) return false
        stack = stack.dropLast(1)
        return true
    }
    BackHandler(enabled = searching || stack.isNotEmpty()) { goUp() }

    fun open(b: Bookmark) {
        val url = b.url ?: return
        vm.goTo(url)
        vm.showBrowser()
    }

    fun openInTab(b: Bookmark, private: Boolean) {
        val url = b.url ?: return
        vm.tabs.openTab(url, private = private, activate = true, live = true)
        vm.showBrowser()
    }

    val importLauncher = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri ?: return@rememberLauncherForActivityResult
        scope.launch {
            val html = withContext(Dispatchers.IO) {
                runCatching {
                    // Bounded: a mistaken pick (a video) must not be read into memory whole.
                    context.contentResolver.openInputStream(uri)?.use { input ->
                        input.readCapped(MAX_IMPORT_BYTES, maxMillis = 60_000)?.toString(Charsets.UTF_8)
                    }
                }
                    .getOrNull()
            }
            if (html == null) {
                snackbar.showSnackbar(res.getString(R.string.bookmarks_import_failed))
                return@launch
            }
            val stats = runCatching { repo.importHtml(html) }.getOrNull()
            refresh++
            if (stats == null) {
                snackbar.showSnackbar(res.getString(R.string.bookmarks_import_failed))
            } else {
                snackbar.showSnackbar(
                    res.getString(
                        R.string.bookmarks_import_result,
                        stats.bookmarks.toInt(),
                        stats.folders.toInt(),
                        stats.skipped.toInt(),
                    ),
                )
            }
        }
    }
    val exportLauncher = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("text/html")) { uri ->
        uri ?: return@rememberLauncherForActivityResult
        scope.launch {
            val ok = runCatching {
                val html = repo.exportHtml()
                withContext(Dispatchers.IO) {
                    context.contentResolver.openOutputStream(uri)?.use { it.write(html.toByteArray(Charsets.UTF_8)) }
                        ?: error("no stream")
                }
            }.isSuccess
            snackbar.showSnackbar(
                res.getString(if (ok) R.string.bookmarks_export_done else R.string.bookmarks_export_failed),
            )
        }
    }

    val title = folderItem?.let { folderTitle(it) } ?: stringResource(R.string.bookmarks_title)
    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    if (searching) {
                        SearchField(query, { query = it }, stringResource(R.string.bookmarks_search_hint))
                    } else {
                        Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                },
                navigationIcon = {
                    IconButton(onClick = { if (!goUp()) vm.pop() }) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = stringResource(R.string.action_back),
                        )
                    }
                },
                actions = {
                    if (searching) {
                        IconButton(onClick = { goUp() }) {
                            Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.action_close))
                        }
                    } else {
                        IconButton(onClick = { searching = true }) {
                            Icon(Icons.Filled.Search, contentDescription = stringResource(R.string.action_search))
                        }
                        IconButton(onClick = { newFolder = true }) {
                            Icon(
                                Icons.Filled.CreateNewFolder,
                                contentDescription = stringResource(R.string.bookmarks_new_folder),
                            )
                        }
                        Box {
                            IconButton(onClick = { menu = true }) {
                                Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.action_more))
                            }
                            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                                DropdownMenuItem(
                                    text = { Text(stringResource(R.string.bookmarks_import)) },
                                    onClick = {
                                        menu = false
                                        importLauncher.launch(
                                            arrayOf("text/html", "application/xhtml+xml", "text/plain"),
                                        )
                                    },
                                )
                                DropdownMenuItem(
                                    text = { Text(stringResource(R.string.bookmarks_export)) },
                                    onClick = {
                                        menu = false
                                        exportLauncher.launch(res.getString(R.string.bookmarks_export_name))
                                    },
                                )
                            }
                        }
                    }
                },
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        if (items.isEmpty()) {
            Box(Modifier.fillMaxSize().padding(padding).padding(32.dp), contentAlignment = Alignment.Center) {
                Text(
                    if (searching && query.isNotBlank()) {
                        stringResource(R.string.bookmarks_no_results, query.trim())
                    } else {
                        stringResource(R.string.bookmarks_empty_folder)
                    },
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyLarge,
                )
            }
        } else {
            LazyColumn(Modifier.fillMaxSize().padding(padding)) {
                items(items, key = { it.id }) { b ->
                    BookmarkRow(
                        b = b,
                        vm = vm,
                        isRoot = repo.isRoot(b.id),
                        onClick = {
                            if (b.isFolder) {
                                searching = false
                                query = ""
                                stack = stack + b.id
                            } else {
                                open(b)
                            }
                        },
                        onOpenNewTab = { openInTab(b, false) },
                        onOpenPrivate = { openInTab(b, true) },
                        onCopy = {
                            b.url?.let { copyToClipboard(context, it) }
                            scope.launch { snackbar.showSnackbar(res.getString(R.string.copied)) }
                        },
                        onEdit = { editing = b.id },
                        onMove = {
                            scope.launch {
                                folders =
                                    runCatching { repo.folders() }.getOrDefault(emptyList()).filterNot { it.id == b.id }
                                moving = b
                            }
                        },
                        onDelete = {
                            if (b.isFolder) {
                                deleting = b
                            } else {
                                scope.launch {
                                    runCatching { repo.delete(b.id) }
                                    refresh++
                                    snackbar.showSnackbar(res.getString(R.string.bookmarks_deleted))
                                }
                            }
                        },
                    )
                }
            }
        }
    }

    editing?.let { id ->
        EditBookmarkDialog(vm, id, onDismiss = { editing = null }, onChanged = { refresh++ })
    }
    moving?.let { b ->
        MoveDialog(
            folders = folders,
            current = b.parent,
            onDismiss = { moving = null },
            onMove = { target ->
                moving = null
                scope.launch {
                    runCatching { repo.move(b.id, target) }
                    refresh++
                }
            },
        )
    }
    if (newFolder) {
        FolderNameDialog(
            existing = null,
            onDismiss = { newFolder = false },
            onSave = { name ->
                newFolder = false
                scope.launch {
                    runCatching { repo.addFolder(folderId ?: repo.roots.first(), name) }
                    refresh++
                }
            },
        )
    }
    deleting?.let { f ->
        AlertDialog(
            onDismissRequest = { deleting = null },
            title = { Text(stringResource(R.string.action_delete)) },
            text = { Text(stringResource(R.string.bookmarks_delete_folder_confirm, f.title)) },
            confirmButton = {
                TextButton(onClick = {
                    deleting = null
                    scope.launch {
                        runCatching { repo.delete(f.id) }
                        refresh++
                        snackbar.showSnackbar(res.getString(R.string.bookmarks_folder_deleted))
                    }
                }) { Text(stringResource(R.string.action_delete), color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = {
                TextButton(onClick = { deleting = null }) { Text(stringResource(R.string.action_cancel)) }
            },
        )
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun BookmarkRow(
    b: Bookmark,
    vm: BrowserViewModel,
    isRoot: Boolean,
    onClick: () -> Unit,
    onOpenNewTab: () -> Unit,
    onOpenPrivate: () -> Unit,
    onCopy: () -> Unit,
    onEdit: () -> Unit,
    onMove: () -> Unit,
    onDelete: () -> Unit,
) {
    var menu by remember { mutableStateOf(false) }
    Box {
        ListItem(
            headlineContent = {
                Text(
                    if (b.isFolder) folderTitle(b) else b.title.ifBlank { b.url.orEmpty() },
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            },
            supportingContent = b.url?.let { url -> { Text(url, maxLines = 1, overflow = TextOverflow.Ellipsis) } },
            leadingContent = {
                if (b.isFolder) {
                    Icon(
                        Icons.Filled.Folder,
                        contentDescription = stringResource(R.string.bookmarks_folder_icon),
                        tint = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.size(32.dp),
                    )
                } else {
                    SiteIcon(b.url.orEmpty(), vm.tabs.favicons, size = 32.dp, fallbackText = b.title)
                }
            },
            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
            modifier = Modifier
                .fillMaxWidth()
                .combinedClickable(onClick = onClick, onLongClick = { if (!isRoot) menu = true }),
        )
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            if (!b.isFolder) {
                DropdownMenuItem(text = { Text(stringResource(R.string.action_open_new_tab)) }, onClick = {
                    menu = false
                    onOpenNewTab()
                })
                DropdownMenuItem(text = { Text(stringResource(R.string.action_open_private_tab)) }, onClick = {
                    menu = false
                    onOpenPrivate()
                })
                DropdownMenuItem(text = { Text(stringResource(R.string.action_copy_link)) }, onClick = {
                    menu = false
                    onCopy()
                })
            }
            DropdownMenuItem(text = { Text(stringResource(R.string.action_edit)) }, onClick = {
                menu = false
                onEdit()
            })
            DropdownMenuItem(text = { Text(stringResource(R.string.action_move)) }, onClick = {
                menu = false
                onMove()
            })
            DropdownMenuItem(text = { Text(stringResource(R.string.action_delete)) }, onClick = {
                menu = false
                onDelete()
            })
        }
    }
}

/** A plain search field for an app bar title slot. */
@Composable
fun SearchField(value: String, onChange: (String) -> Unit, hint: String) {
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    TextField(
        value = value,
        onValueChange = onChange,
        singleLine = true,
        placeholder = { Text(hint) },
        colors = TextFieldDefaults.colors(
            focusedContainerColor = Color.Transparent,
            unfocusedContainerColor = Color.Transparent,
            focusedIndicatorColor = Color.Transparent,
            unfocusedIndicatorColor = Color.Transparent,
        ),
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
        keyboardActions = KeyboardActions(onSearch = {}),
        modifier = Modifier.fillMaxWidth().focusRequester(focus),
    )
}

fun copyToClipboard(context: Context, text: String) {
    val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return
    cm.setPrimaryClip(ClipData.newPlainText("URL", text))
}

/** A bookmarks file larger than this is not a bookmarks file (the core refuses 64 MB anyway). */
private const val MAX_IMPORT_BYTES = 16 * 1024 * 1024
