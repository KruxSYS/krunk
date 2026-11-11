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
        assertEquals(StartupPlan.Fresh, SessionPersistence.plan(StartupMode.NEW_TAB, true, null, countTabs = count))
        assertEquals(StartupPlan.Fresh, SessionPersistence.plan(StartupMode.NEW_TAB, true, session, countTabs = count))
        assertEquals(StartupPlan.Fresh, SessionPersistence.plan(StartupMode.HOMEPAGE, true, session, countTabs = count))
        assertEquals(
            StartupPlan.Prompt(session, 2),
            SessionPersistence.plan(StartupMode.NEW_TAB, false, session, countTabs = count),
        )
        assertEquals(
            StartupPlan.Prompt(session, 2),
            SessionPersistence.plan(StartupMode.HOMEPAGE, false, session, countTabs = count),
        )
        // Restore-session users get their tabs back silently, also after one crash.
        assertEquals(
            StartupPlan.Restore(session),
            SessionPersistence.plan(StartupMode.RESTORE_SESSION, false, session, countTabs = count),
        )
        assertEquals(
            StartupPlan.Restore(session),
            SessionPersistence.plan(StartupMode.RESTORE_SESSION, true, session, countTabs = count),
        )
        // An empty stored session is not worth a prompt.
        assertEquals(StartupPlan.Fresh, SessionPersistence.plan(StartupMode.NEW_TAB, false, "{}") { 0 })
    }

    @Test
    fun aSecondCrashInARowOffersTheSessionInsteadOfRestoringIt() {
        val count = { _: String -> 2 }
        fun plan(unclean: Int, clean: Boolean = false) = SessionPersistence.plan(
            StartupMode.RESTORE_SESSION,
            clean,
            session,
            uncleanStarts = unclean,
            countTabs = count,
        )
        assertEquals(StartupPlan.Restore(session), plan(1))
        // A page that takes the app down on load must not be reloaded on every start.
        assertEquals(StartupPlan.Prompt(session, 2), plan(2))
        assertEquals(StartupPlan.Prompt(session, 2), plan(5))
        // A clean exit in between ends the series.
        assertEquals(StartupPlan.Restore(session), plan(0, clean = true))
        // The counter: reset by a clean exit, else one more.
        assertEquals(0, SessionPersistence.uncleanStarts(3, cleanExit = true))
        assertEquals(1, SessionPersistence.uncleanStarts(0, cleanExit = false))
        assertEquals(4, SessionPersistence.uncleanStarts(3, cleanExit = false))
        assertEquals(1, SessionPersistence.uncleanStarts(-7, cleanExit = false))
    }

    @Test
    fun aTaskBroughtBackByTheSystemKeepsItsTabsWhateverTheStartupSetting() {
        val count = { _: String -> 2 }
        for (startup in listOf(StartupMode.NEW_TAB, StartupMode.HOMEPAGE, StartupMode.RESTORE_SESSION)) {
            // Killed in the background after a clean stop: the user never left.
            assertEquals(
                StartupPlan.Restore(session),
                SessionPersistence.plan(startup, true, session, restoredBySystem = true, countTabs = count),
            )
        }
        // A fresh launch with the same setting still starts fresh.
        assertEquals(
            StartupPlan.Fresh,
            SessionPersistence.plan(StartupMode.NEW_TAB, true, session, restoredBySystem = false, countTabs = count),
        )
        // Nothing stored, nothing to bring back.
        assertEquals(
            StartupPlan.Fresh,
            SessionPersistence.plan(StartupMode.NEW_TAB, true, null, restoredBySystem = true, countTabs = count),
        )
        // In a crash loop even a system restore asks first.
        assertEquals(
            StartupPlan.Prompt(session, 2),
            SessionPersistence.plan(
                StartupMode.NEW_TAB,
                false,
                session,
                uncleanStarts = 2,
                restoredBySystem = true,
                countTabs = count,
            ),
        )
    }

    /** "Close all" on the real tab model: no unloaded tab may get a session on the way. */
    @Test
    fun closeAllClosesBackgroundTabsFirstSoNoneIsLoadedOnTheWay() {
        val four = """{"tabs":[{"url":"https://a.example/","title":"A"},{"url":"https://b.example/","title":"B"},""" +
            """{"url":"https://c.example/","title":"C"},{"url":"https://d.example/","title":"D"}],"active":1}"""

        fun closeAll(order: (List<UInt>, UInt?) -> List<UInt>): List<UInt> {
            val m = TabModel()
            m.restoreSessionJson(four, 1)
            val before = m.tabs().map { it.id }
            val loaded = ArrayList<UInt>()
            for (id in order(before, m.activeId())) {
                for (e in m.close(id, 2)) if (e is TabEffect.CreateView && e.id in before) loaded += e.id
            }
            // What is left is the fresh new-tab page the core opens after the last close.
            assertEquals(1, m.tabs().size)
            assertEquals("newtab", internalPage(m.tabs()[0].url))
            m.destroy()
            return loaded
        }
        assertEquals(emptyList<UInt>(), closeAll(SessionPersistence::closeOrder))
        // In strip order (what 0.1.0 did) the unloaded neighbours are loaded one after the other.
        assertTrue(closeAll { ids, _ -> ids }.isNotEmpty())

        assertEquals(listOf(1u, 3u, 2u), SessionPersistence.closeOrder(listOf(1u, 2u, 3u), 2u))
        assertEquals(listOf(1u, 2u), SessionPersistence.closeOrder(listOf(1u, 2u), null))
        assertEquals(listOf(1u, 2u), SessionPersistence.closeOrder(listOf(1u, 2u), 9u))
    }

    @Test
    fun aRestoredStateThatIsBehindTheTabGetsTheTabsAddressLoaded() {
        val hub = "http://10.0.0.1/h.html"
        // The state and the tab agree: the state alone restores the page (scroll, forms).
        assertFalse(SessionPersistence.restoreNeedsLoad(hub, hub))
        // Saved a moment after going back: the state still stands on the page before.
        assertTrue(SessionPersistence.restoreNeedsLoad("https://example.com/", hub))
        // A state without a current entry restores nothing.
        assertTrue(SessionPersistence.restoreNeedsLoad("", hub))
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
        // A private active tab is not saved, so the session's active index points at B.
        m.activate(b.id, 4)
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
