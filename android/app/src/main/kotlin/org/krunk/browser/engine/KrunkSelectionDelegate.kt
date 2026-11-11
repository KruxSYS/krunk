package org.krunk.browser.engine

import android.app.Activity
import android.view.MenuItem
import org.krunk.browser.ui.browser.SelectionAction
import org.mozilla.geckoview.BasicSelectionActionDelegate
import org.mozilla.geckoview.GeckoSession

/**
 * The menu over selected text: the engine's own entries (copy, select all, ...) plus Krunk's two,
 * "Open <address>" when the selection is an address and "Search <engine> for …". What the two
 * offer is decided by [entries] (the shared core's rules); [open] opens the chosen one.
 * A selection in a password field gets neither.
 */
class KrunkSelectionDelegate(
    activity: Activity,
    private val entries: (String) -> List<SelectionAction>,
    private val open: (SelectionAction) -> Unit,
) : BasicSelectionActionDelegate(activity) {
    private var current: List<SelectionAction> = emptyList()

    private fun entry(id: String): SelectionAction? = when (id) {
        ACTION_OPEN -> current.firstOrNull { it is SelectionAction.Open }
        ACTION_SEARCH -> current.firstOrNull { it is SelectionAction.Search }
        else -> null
    }

    override fun onShowActionRequest(session: GeckoSession, selection: GeckoSession.SelectionActionDelegate.Selection) {
        val password = selection.flags and GeckoSession.SelectionActionDelegate.FLAG_IS_PASSWORD != 0
        current = if (password) emptyList() else entries(selection.text.orEmpty())
        super.onShowActionRequest(session, selection)
    }

    override fun getAllActions(): Array<String> = super.getAllActions() + arrayOf(ACTION_OPEN, ACTION_SEARCH)

    override fun isActionAvailable(id: String): Boolean = when (id) {
        ACTION_OPEN, ACTION_SEARCH -> entry(id) != null
        else -> super.isActionAvailable(id)
    }

    override fun prepareAction(id: String, item: MenuItem) {
        val mine = entry(id)
        if (mine != null) item.title = mine.label else super.prepareAction(id, item)
    }

    override fun performAction(id: String, item: MenuItem): Boolean {
        val mine = entry(id) ?: return super.performAction(id, item)
        open(mine)
        clearSelection()
        mActionMode?.finish()
        return true
    }

    private companion object {
        const val ACTION_OPEN = "org.krunk.browser.selection.OPEN"
        const val ACTION_SEARCH = "org.krunk.browser.selection.SEARCH"
    }
}
