package org.krunk.browser.ui.tabs

import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.unit.DpOffset
import androidx.compose.ui.unit.dp
import org.krunk.browser.R

/** One entry of a tab card's long-press menu. [run] is null when the entry has nothing to do. */
class TabCardAction(val label: String, val run: (() -> Unit)?)

/**
 * What a long press on a tab card offers: Duplicate, Close other tabs, Close tab.
 * "Other tabs" are the other tabs of the card's kind (normal or private, see `otherTabsOf`);
 * the entry says so for private tabs and is disabled when the card's tab is the only one.
 */
@Composable
fun tabCardActions(
    private: Boolean,
    canCloseOthers: Boolean,
    onDuplicate: () -> Unit,
    onCloseOthers: () -> Unit,
    onClose: () -> Unit,
): List<TabCardAction> = listOf(
    TabCardAction(stringResource(R.string.tabs_menu_duplicate), onDuplicate),
    TabCardAction(
        stringResource(if (private) R.string.tabs_menu_close_others_private else R.string.tabs_menu_close_others),
        onCloseOthers.takeIf { canCloseOthers },
    ),
    TabCardAction(stringResource(R.string.tabs_menu_close), onClose),
)

/**
 * The same entries as actions for TalkBack (its actions menu on the card), where a long press
 * is hard to do. Entries with nothing to do are left out.
 */
fun List<TabCardAction>.forAccessibility(): List<CustomAccessibilityAction> = mapNotNull { action ->
    action.run?.let { run ->
        CustomAccessibilityAction(action.label) {
            run()
            true
        }
    }
}

/** The long-press menu of a tab card; it opens over the lower part of the card. */
@Composable
fun TabCardMenu(expanded: Boolean, actions: List<TabCardAction>, onDismiss: () -> Unit) {
    DropdownMenu(expanded = expanded, onDismissRequest = onDismiss, offset = DpOffset(12.dp, (-72).dp)) {
        for (action in actions) {
            DropdownMenuItem(
                text = { Text(action.label) },
                enabled = action.run != null,
                onClick = {
                    onDismiss()
                    action.run?.invoke()
                },
            )
        }
    }
}
