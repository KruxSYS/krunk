package org.krunk.browser.ui.bookmarks

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import org.krunk.browser.ui.BrowserViewModel

/** Bookmark manager (folders, search, edit, import/export). Owned by work package C. */
@Composable
fun BookmarksScreen(vm: BrowserViewModel) {
    Box(Modifier.fillMaxSize()) { Text("Bookmarks") }
}
