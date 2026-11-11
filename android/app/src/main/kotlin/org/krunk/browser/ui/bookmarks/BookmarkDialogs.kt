package org.krunk.browser.ui.bookmarks

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.icons.filled.Folder
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.krunk.browser.R
import org.krunk.browser.core.BookmarksRepository
import org.krunk.browser.core.FolderChoice
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.ffi.Bookmark
import org.krunk.ffi.bookmarkable
import org.krunk.ffi.bookmarksBarId
import org.krunk.ffi.inputToUrl
import org.krunk.ffi.otherBookmarksId

/** The display title of a folder: the roots have fixed names, like Windows. */
@Composable
fun folderTitle(b: Bookmark): String = when (b.id) {
    bookmarksBarId() -> stringResource(R.string.bookmarks_bar)
    otherBookmarksId() -> stringResource(R.string.bookmarks_other)
    else -> b.title
}

@Composable
fun folderTitle(f: FolderChoice): String = when (f.id) {
    bookmarksBarId() -> stringResource(R.string.bookmarks_bar)
    otherBookmarksId() -> stringResource(R.string.bookmarks_other)
    else -> f.title
}

/**
 * Edits one bookmark's title, address and folder (also used by the menu's "Bookmark this page"
 * after `toggleBookmark` created it). For a folder only the title is editable. [onChanged] runs
 * after a save or delete so lists can refresh.
 */
@Composable
fun EditBookmarkDialog(vm: BrowserViewModel, bookmarkId: Long, onDismiss: () -> Unit, onChanged: () -> Unit = {}) {
    val repo = remember(vm) { BookmarksRepository(vm.core.store) }
    val scope = rememberCoroutineScope()
    var loaded by remember { mutableStateOf<Bookmark?>(null) }
    var folders by remember { mutableStateOf<List<FolderChoice>>(emptyList()) }
    var title by remember { mutableStateOf("") }
    var url by remember { mutableStateOf("") }
    var folder by remember { mutableStateOf<Long?>(null) }
    var urlError by remember { mutableStateOf(false) }

    LaunchedEffect(bookmarkId) {
        val b = runCatching { repo.get(bookmarkId) }.getOrNull()
        if (b == null) {
            onDismiss()
            return@LaunchedEffect
        }
        loaded = b
        title = b.title
        url = b.url.orEmpty()
        folder = b.parent
        folders = runCatching { repo.folders() }.getOrDefault(emptyList())
            // A folder cannot be moved into itself.
            .filterNot { it.id == b.id }
    }

    val b = loaded ?: return
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(if (b.isFolder) R.string.bookmarks_edit_folder else R.string.bookmarks_edit)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = title,
                    onValueChange = { title = it },
                    label = { Text(stringResource(R.string.field_title)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                if (!b.isFolder) {
                    OutlinedTextField(
                        value = url,
                        onValueChange = {
                            url = it
                            urlError = false
                        },
                        label = { Text(stringResource(R.string.field_url)) },
                        singleLine = true,
                        isError = urlError,
                        supportingText = if (urlError) {
                            { Text(stringResource(R.string.bookmarks_url_invalid)) }
                        } else {
                            null
                        },
                        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
                if (!repo.isRoot(b.id)) {
                    FolderPicker(folders = folders, selected = folder, onSelect = { folder = it })
                }
            }
        },
        confirmButton = {
            TextButton(onClick = {
                val target = if (b.isFolder) null else inputToUrl(url.trim(), vm.settingsRepo.searchTemplate)
                if (target != null && (url.isBlank() || !bookmarkable(target))) {
                    urlError = true
                    return@TextButton
                }
                scope.launch {
                    runCatching {
                        repo.edit(b.id, title.trim().ifEmpty { target ?: b.title }, target)
                        val f = folder
                        if (f != null && f != b.parent) repo.move(b.id, f)
                    }
                    onChanged()
                    onDismiss()
                }
            }) { Text(stringResource(R.string.action_save)) }
        },
        dismissButton = {
            Row {
                if (!repo.isRoot(b.id)) {
                    TextButton(onClick = {
                        scope.launch {
                            runCatching { repo.delete(b.id) }
                            onChanged()
                            onDismiss()
                        }
                    }) { Text(stringResource(R.string.action_remove), color = MaterialTheme.colorScheme.error) }
                }
                TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) }
            }
        },
    )
}

/** A read-only field that drops down the folder list (indented by depth). */
@Composable
fun FolderPicker(folders: List<FolderChoice>, selected: Long?, onSelect: (Long) -> Unit) {
    var open by remember { mutableStateOf(false) }
    val current = folders.firstOrNull { it.id == selected }
    Box(Modifier.fillMaxWidth()) {
        OutlinedTextField(
            value = current?.let { folderTitle(it) } ?: "",
            onValueChange = {},
            readOnly = true,
            enabled = false,
            label = { Text(stringResource(R.string.field_folder)) },
            trailingIcon = { Icon(Icons.Filled.ArrowDropDown, contentDescription = null) },
            leadingIcon = { Icon(Icons.Filled.Folder, contentDescription = null) },
            colors = androidx.compose.material3.OutlinedTextFieldDefaults.colors(
                disabledTextColor = MaterialTheme.colorScheme.onSurface,
                disabledBorderColor = MaterialTheme.colorScheme.outline,
                disabledLabelColor = MaterialTheme.colorScheme.onSurfaceVariant,
                disabledLeadingIconColor = MaterialTheme.colorScheme.onSurfaceVariant,
                disabledTrailingIconColor = MaterialTheme.colorScheme.onSurfaceVariant,
            ),
            modifier = Modifier.fillMaxWidth().clickable { open = true },
        )
        DropdownMenu(expanded = open, onDismissRequest = { open = false }, modifier = Modifier.heightIn(max = 320.dp)) {
            for (f in folders) {
                DropdownMenuItem(
                    text = {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Box(Modifier.width((f.depth * 16).dp))
                            Text(folderTitle(f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    },
                    leadingIcon = { Icon(Icons.Filled.Folder, contentDescription = null) },
                    onClick = {
                        onSelect(f.id)
                        open = false
                    },
                )
            }
        }
    }
}

/** New folder (`existing == null`) or rename. */
@Composable
fun FolderNameDialog(existing: String?, onDismiss: () -> Unit, onSave: (String) -> Unit) {
    var name by remember { mutableStateOf(existing.orEmpty()) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = {
            Text(
                stringResource(
                    if (existing ==
                        null
                    ) {
                        R.string.bookmarks_new_folder
                    } else {
                        R.string.bookmarks_edit_folder
                    },
                ),
            )
        },
        text = {
            OutlinedTextField(
                value = name,
                onValueChange = { name = it },
                label = { Text(stringResource(R.string.bookmarks_folder_name)) },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { if (name.isNotBlank()) onSave(name.trim()) }),
                modifier = Modifier.fillMaxWidth(),
            )
        },
        confirmButton = {
            TextButton(onClick = { onSave(name.trim()) }, enabled = name.isNotBlank()) {
                Text(stringResource(R.string.action_save))
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

/** "Move to": a radio list of every folder. */
@Composable
fun MoveDialog(folders: List<FolderChoice>, current: Long?, onDismiss: () -> Unit, onMove: (Long) -> Unit) {
    var chosen by remember { mutableStateOf(current) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.bookmarks_move_to)) },
        text = {
            Column(Modifier.heightIn(max = 360.dp).verticalScroll(rememberScrollState())) {
                for (f in folders) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        modifier = Modifier
                            .fillMaxWidth()
                            .heightIn(min = 48.dp)
                            .clickable { chosen = f.id }
                            .padding(start = (f.depth * 16).dp),
                    ) {
                        RadioButton(selected = chosen == f.id, onClick = { chosen = f.id })
                        Icon(Icons.Filled.Folder, contentDescription = null, modifier = Modifier.padding(end = 8.dp))
                        Text(folderTitle(f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = { chosen?.let(onMove) }, enabled = chosen != null && chosen != current) {
                Text(stringResource(R.string.action_move))
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}
