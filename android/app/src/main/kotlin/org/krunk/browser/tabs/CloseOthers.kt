package org.krunk.browser.tabs

import org.krunk.ffi.TabEffect
import org.krunk.ffi.TabModel

/**
 * The tabs "Close other tabs" on tab [id] closes, in tab order.
 *
 * "Others" are the other tabs of the same kind: on a normal tab every other normal tab, on a
 * private tab every other private tab. The switcher lists the two kinds apart, and an entry in
 * one list never closes tabs of the other (closing private tabs ends their sign-ins and cannot
 * be undone). A search in the switcher does not narrow it: tabs the search hides are closed too.
 * Empty when there is nothing to close (the menu entry is disabled then) or [id] is unknown.
 */
fun TabModel.otherTabsOf(id: UInt): List<UInt> {
    val private = tab(id)?.private ?: return emptyList()
    val sameKind = tabs().filter { it.private == private }.map { it.id }.toSet()
    return closeManyIds(id, true).filter { it in sameKind }
}

/**
 * "Close other tabs" on tab [id] (see [otherTabsOf]); returns what to do to the sessions.
 *
 * With one kind of tab open this is the core's own operation. With both kinds open the core
 * would close across the private boundary, so its steps are done here for the same kind only,
 * in its order: when the active tab is about to close, [id] becomes active first (no tab that
 * is about to close gets loaded for a moment), then the tabs close from the last to the first,
 * so each entry in "Recently closed" keeps its real position. Closed normal tabs land in
 * "Recently closed" like any closed tab; private tabs never do (the core's rule).
 */
fun TabModel.closeOtherTabs(id: UInt, now: Long): List<TabEffect> {
    val doomed = otherTabsOf(id)
    if (doomed.isEmpty()) return emptyList()
    if (doomed.size == count().toInt() - 1) return closeMany(id, true, now)
    val effects = ArrayList<TabEffect>()
    val active = activeId()
    if (active != null && active in doomed) effects += activate(id, now)
    for (other in doomed.asReversed()) effects += close(other, now)
    return effects
}
