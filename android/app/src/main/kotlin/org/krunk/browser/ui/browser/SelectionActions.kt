package org.krunk.browser.ui.browser

import org.krunk.ffi.selectionAddress
import org.krunk.ffi.selectionSearch

/** One entry Krunk adds to the menu over selected text. */
sealed interface SelectionAction {
    /** The menu's text. */
    val label: String

    /** The address the entry opens (in a new tab next to the current one, same privacy). */
    val url: String

    /** "Search DuckDuckGo for …". */
    data class Search(override val label: String, override val url: String) : SelectionAction

    /** The selection is an address: open it. */
    data class Open(override val label: String, override val url: String) : SelectionAction
}

object SelectionActions {
    /** Selections longer than this get no entries (and are not handed to the core). */
    const val MAX_SELECTION = 4096

    /**
     * The entries for [selection], decided by the shared core like on Windows: an address (one
     * word that reads as one, http or https only) can be opened, and any text can be searched
     * with the engine in effect. An editable field's selection gets the same entries.
     */
    fun of(
        selection: String,
        engineName: String,
        searchTemplate: String,
        openLabel: (String) -> String,
    ): List<SelectionAction> {
        if (selection.isBlank() || selection.length > MAX_SELECTION) return emptyList()
        val out = ArrayList<SelectionAction>(2)
        selectionAddress(selection)?.let { out += SelectionAction.Open(openLabel(it.label), it.url) }
        selectionSearch(selection, engineName, searchTemplate)?.let { out += SelectionAction.Search(it.label, it.url) }
        return out
    }

    /** The engine's menu actions with Krunk's [mine] placed before [before] (at the end without it). */
    fun placed(engine: List<String>, mine: List<String>, before: String): List<String> {
        val at = engine.indexOf(before).let { if (it < 0) engine.size else it }
        return engine.take(at) + mine + engine.drop(at)
    }
}
