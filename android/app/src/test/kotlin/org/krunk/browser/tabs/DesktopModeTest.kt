package org.krunk.browser.tabs

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.ffi.TabEffect
import org.krunk.ffi.TabModel

/** Desktop-mode persistence: the mode is the tab's and outlives the tab's engine sessions. */
class DesktopModeTest {
    /** What the tab manager does with the model's effects: each new session starts in its tab's mode. */
    private class Sessions(val desktop: DesktopModes) {
        val live = HashMap<UInt, Boolean>()

        fun apply(effects: List<TabEffect>) {
            for (e in effects) {
                when (e) {
                    is TabEffect.CreateView -> live[e.id] = desktop[e.id]
                    is TabEffect.DestroyView -> live.remove(e.id)
                    is TabEffect.Activate -> Unit
                }
            }
        }
    }

    @Test
    fun aTabThatGetsANewSessionKeepsItsDesktopMode() {
        val m = TabModel()
        val desktop = DesktopModes()
        val sessions = Sessions(desktop)
        val a = m.open("https://a.example/", false, null, true, true, 1)
        sessions.apply(a.effects)
        val b = m.open("https://b.example/", false, null, true, true, 2)
        sessions.apply(b.effects)
        assertEquals(mapOf(a.id to false, b.id to false), sessions.live)

        // The user asks for the desktop site in tab A (the live session is switched in place).
        desktop[a.id] = true
        sessions.live[a.id] = true

        // A is discarded to save memory and comes back when the user returns to it.
        sessions.apply(m.unload(a.id))
        assertFalse(a.id in sessions.live)
        assertTrue("the mode stays with the tab while it has no session", desktop[a.id])
        sessions.apply(m.activate(a.id, 3))
        assertEquals("the new session starts as a desktop one", true, sessions.live[a.id])
        assertEquals("other tabs are not touched", false, sessions.live[b.id])

        // Switched off again, it stays off for the next session.
        desktop[a.id] = false
        sessions.apply(m.activate(b.id, 4))
        sessions.apply(m.unload(a.id))
        sessions.apply(m.activate(a.id, 5))
        assertEquals(false, sessions.live[a.id])
        m.destroy()
    }

    @Test
    fun theModeEndsWithTheTabAndADuplicateStartsWithIt() {
        val m = TabModel()
        val desktop = DesktopModes()
        val sessions = Sessions(desktop)
        val a = m.open("https://a.example/", false, null, true, true, 1)
        sessions.apply(a.effects)
        desktop[a.id] = true

        val copy = m.duplicate(a.id, 2)!!
        desktop.copy(a.id, copy.id)
        sessions.apply(copy.effects)
        assertEquals(true, sessions.live[copy.id])

        // The copy has its own mode from then on.
        desktop[copy.id] = false
        assertTrue(desktop[a.id])

        // A closed tab's mode is forgotten; the tab that comes back from "Recently closed" is new.
        sessions.apply(m.close(a.id, 3))
        desktop.forget(a.id)
        assertFalse(desktop[a.id])
        val reopened = m.reopenClosed(4)!!
        sessions.apply(reopened.effects)
        assertEquals(false, sessions.live[reopened.id])
        m.destroy()
    }

    /** The restart: the rows saved next to `kv["session"]` give the restored tabs their mode back. */
    @Test
    fun theModeComesBackWithTheSessionAfterARestart() {
        val m = TabModel()
        val before = DesktopModes()
        val a = m.open("https://a.example/", false, null, true, true, 1)
        val b = m.open("https://b.example/", false, null, true, true, 2)
        val c = m.open("https://c.example/", false, null, true, true, 3)
        val p = m.open("https://private.example/", true, null, false, false, 4)
        before[a.id] = true
        before[c.id] = true
        before[p.id] = true
        // What the tab manager saves: one row per normal tab, in tab order. A has a state, C none.
        val states = mapOf(a.id to "state-a", b.id to "state-b")
        val saved = m.tabs().filter { !it.private }.map { SavedTabState(it.url, states[it.id] ?: "", before[it.id]) }
        assertEquals(listOf(true, false, true), saved.map { it.desktop })
        val json = m.sessionJson()
        m.destroy()

        val n = TabModel()
        val after = DesktopModes()
        val sessions = Sessions(after)
        val effects = n.restoreSessionJson(json, 10)
        val tabs = n.tabs().map { it.id to it.url }
        assertEquals(3, tabs.size)
        // The modes are in place before the first session is created.
        val rows = assignSavedTabs(tabs, saved)
        for ((id, row) in rows) after[id] = row.desktop
        sessions.apply(effects)
        assertEquals(listOf(true, false, true), tabs.map { after[it.first] })
        assertEquals("the active tab's session starts as a desktop one", mapOf(tabs[2].first to true), sessions.live)
        // A lazily restored tab gets its mode when its session is made.
        sessions.apply(n.activate(tabs[0].first, 11))
        assertEquals(true, sessions.live[tabs[0].first])
        // States still follow their tabs; a row with only the mode is worth restoring too.
        assertEquals("state-a", rows[tabs[0].first]?.state)
        assertEquals("state-b", rows[tabs[1].first]?.state)
        assertEquals(SavedTabState("https://c.example/", "", true), rows[tabs[2].first])
        n.destroy()
    }

    @Test
    fun savedRowsArePairedLikeTheStates() {
        val tabs = listOf(1u to "https://a.example/", 2u to "https://a.example/", 3u to "https://b.example/")
        // Rows written before the mode existed: every tab comes back with the mobile site.
        val old = listOf(
            SavedTabState("https://a.example/", "s1"),
            SavedTabState("https://a.example/", "s2"),
            SavedTabState("https://b.example/", ""),
        )
        val fromOld = assignSavedTabs(tabs, old)
        assertEquals(mapOf(1u to "s1", 2u to "s2"), fromOld.mapValues { it.value.state })
        assertTrue(fromOld.values.none { it.desktop })
        // Two tabs on the same address: each takes its own row, state and mode together.
        val rows = listOf(
            SavedTabState("https://a.example/", "s1", desktop = false),
            SavedTabState("https://a.example/", "s2", desktop = true),
            SavedTabState("https://b.example/", "", desktop = false),
        )
        val got = assignSavedTabs(tabs, rows)
        assertEquals(mapOf(1u to rows[0], 2u to rows[1]), got)
        // A tab the core did not restore leaves its row unused; the others keep theirs.
        val skipped = listOf(
            SavedTabState("data:text/plain,skipped", "sx", desktop = true),
            SavedTabState("https://b.example/", "", desktop = true),
        )
        assertEquals(mapOf(3u to skipped[1]), assignSavedTabs(tabs, skipped))
        assertTrue(assignSavedTabs(tabs, emptyList()).isEmpty())
    }
}
