package org.krunk.browser.tabs

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.ffi.TabEffect
import org.krunk.ffi.TabModel

/** The tab card's menu on the real tab model: "Duplicate" and "Close other tabs". */
class TabCardActionsTest {
    private fun TabModel.urls() = tabs().map { it.url }

    private fun TabModel.add(url: String, private: Boolean = false, now: Long = 1): UInt =
        open(url, private, null, true, true, now).id

    @Test
    fun aDuplicateOpensNextToTheOriginalWithTheSameAddressAndPrivacy() {
        val m = TabModel()
        val a = m.add("https://a.example/")
        val b = m.add("https://b.example/")
        val p = m.add("https://private.example/", private = true)
        m.setTitle(a, "A")

        val copy = m.duplicate(a, 5)!!
        assertEquals(
            listOf("https://a.example/", "https://a.example/", "https://b.example/", "https://private.example/"),
            m.urls(),
        )
        assertEquals("right after the original", m.indexOf(a)!! + 1u, m.indexOf(copy.id))
        val info = m.tab(copy.id)!!
        assertEquals("A", info.title)
        assertFalse(info.private)
        assertTrue("the copy gets a session and loads", info.live && TabEffect.CreateView(copy.id) in copy.effects)
        assertEquals("the copy is the tab in front", copy.id, m.activeId())

        val privateCopy = m.duplicate(p, 6)!!
        assertTrue("a private tab's copy is private", m.tab(privateCopy.id)!!.private)
        assertEquals(m.indexOf(p)!! + 1u, m.indexOf(privateCopy.id))
        assertEquals("https://private.example/", m.tab(privateCopy.id)!!.url)

        // Closed copies are ordinary closed tabs: the normal one can be reopened, the private one not.
        m.close(copy.id, 7)
        m.close(privateCopy.id, 8)
        assertEquals(listOf("https://a.example/"), m.closedTabs().map { it.url })
        assertNull(m.duplicate(999u, 9))
        assertEquals(listOf(a, b, p), m.tabs().map { it.id })
        m.destroy()
    }

    @Test
    fun closeOtherTabsNeverCrossesThePrivateBoundary() {
        val m = TabModel()
        // The two kinds are mixed in the strip; the switcher shows them as two lists.
        val a = m.add("https://a.example/", now = 1)
        val p = m.add("https://p.example/", private = true, now = 2)
        val b = m.add("https://b.example/", now = 3)
        val q = m.add("https://q.example/", private = true, now = 4)
        val c = m.add("https://c.example/", now = 5)
        assertEquals(listOf(a, c), m.otherTabsOf(b))
        assertEquals(listOf(q), m.otherTabsOf(p))
        assertEquals(emptyList<UInt>(), m.otherTabsOf(999u))

        // On a normal tab: the other normal tabs close, the private ones stay. C was in front,
        // so B comes to the front first.
        assertEquals(c, m.activeId())
        val effects = m.closeOtherTabs(b, 10)
        assertEquals(listOf(p, b, q), m.tabs().map { it.id })
        assertEquals(b, m.activeId())
        assertEquals(
            setOf(TabEffect.DestroyView(a), TabEffect.DestroyView(c)),
            effects.filterIsInstance<TabEffect.DestroyView>().toSet(),
        )
        assertTrue(effects.none { it is TabEffect.CreateView })
        // They are in "Recently closed" like any closed tab and come back where they were.
        assertEquals(setOf("https://a.example/", "https://c.example/"), m.closedTabs().map { it.url }.toSet())
        m.reopenClosed(11)
        m.reopenClosed(12)
        assertEquals(
            listOf(
                "https://a.example/",
                "https://p.example/",
                "https://b.example/",
                "https://q.example/",
                "https://c.example/",
            ),
            m.urls(),
        )
        assertTrue(m.closedTabs().isEmpty())

        // On a private tab: only the other private tab closes, and it is not remembered. The
        // normal tab in front stays in front; nothing is activated or loaded.
        val front = m.activeId()
        val privateEffects = m.closeOtherTabs(p, 13)
        assertEquals(listOf<TabEffect>(TabEffect.DestroyView(q)), privateEffects)
        assertEquals(
            listOf("https://a.example/", "https://p.example/", "https://b.example/", "https://c.example/"),
            m.urls(),
        )
        assertEquals(front, m.activeId())
        assertTrue("private tabs never land in Recently closed", m.closedTabs().isEmpty())

        // Nothing to close: the only private tab (the menu entry is disabled then).
        assertTrue(m.otherTabsOf(p).isEmpty())
        assertTrue(m.closeOtherTabs(p, 14).isEmpty())
        assertEquals(4u, m.count())
        m.destroy()
    }

    @Test
    fun closingTheOthersLoadsNoTabThatIsAboutToClose() {
        val four = """{"tabs":[{"url":"https://a.example/","title":"A"},{"url":"https://b.example/","title":"B"},""" +
            """{"url":"https://c.example/","title":"C"},{"url":"https://d.example/","title":"D"}],"active":1}"""
        val m = TabModel()
        m.restoreSessionJson(four, 1)
        val ids = m.tabs().map { it.id }
        // A private tab in the background, so the same-kind steps are done on this side.
        val p = m.open("https://p.example/", true, null, false, false, 2).id
        // The card of D (unloaded) is asked to close the others; B, in front, is one of them.
        val effects = m.closeOtherTabs(ids[3], 3)
        assertEquals(listOf(ids[3], p), m.tabs().map { it.id })
        assertEquals(ids[3], m.activeId())
        assertEquals(listOf(TabEffect.CreateView(ids[3])), effects.filterIsInstance<TabEffect.CreateView>())
        assertEquals(listOf(TabEffect.DestroyView(ids[1])), effects.filterIsInstance<TabEffect.DestroyView>())
        m.destroy()
    }

    /** With one kind of tab open, "Close other tabs" is the core's own operation, step for step. */
    @Test
    fun closeOtherTabsMatchesTheCoresOperation() {
        fun model(withPrivate: Boolean): Pair<TabModel, List<UInt>> {
            val m = TabModel()
            val ids = listOf("a", "b", "c", "d").mapIndexed { i, n -> m.add("https://$n.example/", now = i + 1L) }
            if (withPrivate) m.open("https://p.example/", true, null, false, false, 9)
            return m to ids
        }
        for (keep in 0..3) {
            val (core, coreIds) = model(withPrivate = false)
            val coreEffects = core.closeMany(coreIds[keep], true, 20)
            // Only normal tabs: the same call.
            val (same, sameIds) = model(withPrivate = false)
            assertEquals(coreEffects, same.closeOtherTabs(sameIds[keep], 20))
            // A private tab as well: the same steps for the normal tabs, done one by one.
            val (mixed, mixedIds) = model(withPrivate = true)
            assertEquals(coreIds, mixedIds)
            assertEquals("keeping tab $keep", coreEffects, mixed.closeOtherTabs(mixedIds[keep], 20))
            assertEquals(core.closedTabs(), mixed.closedTabs())
            assertEquals(core.activeId(), mixed.activeId())
            assertEquals(core.urls() + "https://p.example/", mixed.urls())
            listOf(core, same, mixed).forEach { it.destroy() }
        }
    }
}
