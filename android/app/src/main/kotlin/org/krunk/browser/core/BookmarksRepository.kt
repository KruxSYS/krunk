package org.krunk.browser.core

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.krunk.ffi.Bookmark
import org.krunk.ffi.ImportStats
import org.krunk.ffi.KrunkStore
import org.krunk.ffi.bookmarksBarId
import org.krunk.ffi.otherBookmarksId

/** A bookmark or folder with its children (folders only). */
data class BookmarkNode(val item: Bookmark, val children: List<BookmarkNode>)

/** One folder in a flat, indented list (folder pickers). */
data class FolderChoice(val id: Long, val title: String, val depth: Int)

/**
 * The bookmark tree on the core store (same SQLite rows as Windows), off the main thread. The
 * two roots are "Bookmarks bar" ([bookmarksBarId]) and "Other bookmarks" ([otherBookmarksId]);
 * the UI shows them as the top level.
 */
class BookmarksRepository(private val store: KrunkStore, private val io: CoroutineDispatcher = Dispatchers.IO) {
    val roots: List<Long> = listOf(bookmarksBarId(), otherBookmarksId())

    fun isRoot(id: Long): Boolean = id in roots

    suspend fun get(id: Long): Bookmark? = withContext(io) { store.bookmark(id) }

    /** Direct children of a folder, in order. */
    suspend fun children(folder: Long): List<Bookmark> = withContext(io) { store.bookmarkChildren(folder) }

    /** The two roots as rows (for the top level of the manager). */
    suspend fun rootItems(): List<Bookmark> = withContext(io) { roots.mapNotNull { store.bookmark(it) } }

    suspend fun search(text: String): List<Bookmark> = withContext(io) { store.searchBookmarks(text) }

    suspend fun all(): List<Bookmark> = withContext(io) { store.allBookmarks() }

    suspend fun add(parent: Long, title: String, url: String): Long =
        withContext(io) { store.addBookmark(parent, title, url) }

    suspend fun addFolder(parent: Long, title: String): Long = withContext(io) { store.addFolder(parent, title) }

    /** New title and, for a bookmark, URL (`null` keeps it / for folders). */
    suspend fun edit(id: Long, title: String, url: String?) = withContext(io) { store.editBookmark(id, title, url) }

    /** Moves into [parent] at [index] (past the end = last). */
    suspend fun move(id: Long, parent: Long, index: UInt = UInt.MAX_VALUE) =
        withContext(io) { store.moveBookmark(id, parent, index) }

    suspend fun delete(id: Long) = withContext(io) { store.deleteBookmark(id) }

    /** Ancestors of a folder from the root down to and including it (for breadcrumbs). */
    suspend fun path(folder: Long): List<Bookmark> = withContext(io) {
        val out = ArrayList<Bookmark>()
        var cursor: Long? = folder
        while (cursor != null && out.size < 64) {
            val b = store.bookmark(cursor) ?: break
            out.add(0, b)
            cursor = b.parent
        }
        out
    }

    /** Every folder as an indented list (roots first), for "Move to" and the bookmark editor. */
    suspend fun folders(): List<FolderChoice> = folderList(all())

    suspend fun importHtml(html: String): ImportStats = withContext(io) { store.importNetscape(html) }

    suspend fun exportHtml(): String = withContext(io) { store.exportNetscape() }

    suspend fun isBookmarked(url: String): Boolean = withContext(io) { store.isBookmarked(url) }

    companion object {
        /**
         * Rebuilds the tree from the flat depth-first list `all_bookmarks()` returns. Items whose
         * parent is missing (the roots) become top-level nodes; order within a folder is kept.
         */
        fun buildTree(all: List<Bookmark>): List<BookmarkNode> {
            val byParent = HashMap<Long?, MutableList<Bookmark>>()
            val ids = all.mapTo(HashSet()) { it.id }
            for (b in all) {
                val key = if (b.parent != null && b.parent in ids) b.parent else null
                byParent.getOrPut(key) { ArrayList() } += b
            }
            fun node(b: Bookmark, depth: Int): BookmarkNode {
                val kids = if (b.isFolder && depth < 64) byParent[b.id].orEmpty().map { node(it, depth + 1) } else emptyList()
                return BookmarkNode(b, kids)
            }
            return byParent[null].orEmpty().map { node(it, 0) }
        }

        /** Folders only, depth first, with their nesting depth (roots at 0). */
        fun folderList(all: List<Bookmark>): List<FolderChoice> {
            val out = ArrayList<FolderChoice>()
            fun walk(nodes: List<BookmarkNode>, depth: Int) {
                for (n in nodes) {
                    if (!n.item.isFolder) continue
                    out += FolderChoice(n.item.id, n.item.title, depth)
                    walk(n.children, depth + 1)
                }
            }
            walk(buildTree(all), 0)
            return out
        }
    }
}
