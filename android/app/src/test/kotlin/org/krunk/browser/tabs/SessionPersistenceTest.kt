package org.krunk.browser.tabs

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.ffi.StartupMode
import org.krunk.ffi.TabEffect
import org.krunk.ffi.TabModel
import org.krunk.ffi.internalPage

class SessionPersistenceTest {
    private val session =
        """{"tabs":[{"url":"https://a.example/","title":"A"},{"url":"https://b.example/","title":"B"}],"active":1}"""

    @Test
    fun startupPlanPromptsOnlyAfterACrashWithoutTheRestoreSetting() {
        val count = { _: String -> 2 }
        assertEquals(StartupPlan.Fresh, SessionPersistence.plan(StartupMode.NEW_TAB, true, null, count))
        assertEquals(StartupPlan.Fresh, SessionPersistence.plan(StartupMode.NEW_TAB, true, session, count))
        assertEquals(StartupPlan.Fresh, SessionPersistence.plan(StartupMode.HOMEPAGE, true, session, count))
        assertEquals(
            StartupPlan.Prompt(session, 2),
            SessionPersistence.plan(StartupMode.NEW_TAB, false, session, count),
        )
        assertEquals(
            StartupPlan.Prompt(session, 2),
            SessionPersistence.plan(StartupMode.HOMEPAGE, false, session, count),
        )
        // Restore-session users get their tabs back silently, crash or not.
        assertEquals(
            StartupPlan.Restore(session),
            SessionPersistence.plan(StartupMode.RESTORE_SESSION, false, session, count),
        )
        assertEquals(
            StartupPlan.Restore(session),
            SessionPersistence.plan(StartupMode.RESTORE_SESSION, true, session, count),
        )
        // An empty stored session is not worth a prompt.
        assertEquals(StartupPlan.Fresh, SessionPersistence.plan(StartupMode.NEW_TAB, false, "{}") { 0 })
    }

    @Test
    fun geckoStatesFollowTheirUrls() {
        val tabs = listOf(1u to "https://a.example/", 2u to "https://b.example/", 3u to "https://c.example/")
        val saved = listOf(
            "https://a.example/" to "sa",
            "data:text/plain,skipped" to "sx", // the core does not restore data: URLs
            "https://b.example/" to "sb",
            "https://c.example/" to "",
        )
        assertEquals(mapOf(1u to "sa", 2u to "sb"), SessionPersistence.assignStates(tabs, saved))
        // Same count, URLs rewritten by the core (credentials stripped): positions win.
        val rewritten = listOf(1u to "https://a.example/", 2u to "https://b.example/")
        val withCreds = listOf("https://u:p@a.example/" to "sa", "https://u:p@b.example/" to "sb")
        assertEquals(mapOf(1u to "sa", 2u to "sb"), SessionPersistence.assignStates(rewritten, withCreds))
        // Duplicate URLs take distinct entries in order.
        val dupes = listOf(1u to "https://a.example/", 2u to "https://a.example/")
        val dupeSaved = listOf("https://a.example/" to "s1", "https://a.example/" to "s2")
        assertEquals(mapOf(1u to "s1", 2u to "s2"), SessionPersistence.assignStates(dupes, dupeSaved))
        assertTrue(SessionPersistence.assignStates(tabs, emptyList()).isEmpty())
    }

    /** The persistence round trip through the real core model (what `kv["session"]` holds). */
    @Test
    fun sessionJsonRoundTripThroughTheCoreModel() {
        val m = TabModel()
        val a = m.open("https://a.example/", false, null, true, true, 1)
        m.setTitle(a.id, "A")
        val b = m.open("https://b.example/", false, null, true, true, 2)
        m.setTitle(b.id, "B")
        val p = m.open("https://private.example/", true, null, true, true, 3)
        m.setTitle(p.id, "Private")
        val json = m.sessionJson()
        assertTrue(json.contains("\"active\":1"))
        assertFalse("private tabs are never written", json.contains("private.example"))

        val n = TabModel()
        val effects = n.restoreSessionJson(json, 10)
        val tabs = n.tabs()
        assertEquals(listOf("https://a.example/", "https://b.example/"), tabs.map { it.url })
        assertEquals(listOf("A", "B"), tabs.map { it.title })
        assertEquals(tabs[1].id, n.activeId())
        assertTrue("only the active tab gets a session", tabs[1].live && !tabs[0].live)
        assertTrue(effects.any { it is TabEffect.CreateView && it.id == tabs[1].id })
        assertEquals(
            listOf(tabs[0].id to "https://a.example/", tabs[1].id to "https://b.example/"),
            tabs.map {
                it.id to
                    it.url
            },
        )

        // Recently closed survive as JSON too; private closes are not remembered.
        m.close(a.id, 4)
        m.close(p.id, 5)
        val closed = m.closedTabsJson()
        val o = TabModel()
        o.open("", false, null, true, true, 6)
        o.restoreClosedTabsJson(closed)
        assertEquals(listOf("https://a.example/"), o.closedTabs().map { it.url })
        val r = o.reopenClosedAt(0u, 7)
        assertEquals("https://a.example/", r?.let { o.tab(it.id)?.url })
        assertTrue(o.closedTabs().isEmpty())
        // Closing the last tab leaves a new-tab page, which is restorable.
        val one = TabModel()
        val x = one.open("https://x.example/", false, null, true, true, 1)
        one.close(x.id, 2)
        assertEquals("newtab", internalPage(one.tabs()[0].url))
        listOf(m, n, o, one).forEach { it.destroy() }
    }
}
