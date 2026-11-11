package org.krunk.browser.engine

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class PagePromptTest {
    private val answers = ArrayList<String>()

    private fun alert(tab: UInt, name: String) =
        PagePrompt.Alert(tab, "example.com", name, canBlock = false) { answers += "$name:$it" }

    @Test
    fun onlyThePromptOfTheTabOnScreenIsShown() {
        val q = PromptQueue()
        val a = alert(1u, "a")
        val b = alert(2u, "b")
        val c = alert(1u, "c")
        q.add(a)
        q.add(b)
        q.add(c)
        assertSame(a, q.current(1u))
        assertSame(b, q.current(2u))
        assertNull(q.current(3u))
        assertNull(q.current(null))
        // Answered: the next one of that tab follows.
        q.remove(a)
        assertSame(c, q.current(1u))
        assertEquals(2, q.size)
    }

    @Test
    fun aClosedTabsPromptsAreDismissed() {
        val q = PromptQueue()
        q.add(alert(1u, "a"))
        q.add(alert(2u, "b"))
        q.add(alert(1u, "c"))
        q.dropTab(1u)
        assertEquals(listOf("a:DISMISS", "c:DISMISS"), answers)
        assertNull(q.current(1u))
        assertEquals(1, q.size)
    }

    @Test
    fun promptsAnswerThroughTheirOwnFunctions() {
        var confirm: PagePrompt.Answer? = null
        PagePrompt.Confirm(1u, "s", "m", false) { confirm = it }.answer(true)
        assertEquals(PagePrompt.Answer.OK, confirm)
        PagePrompt.Confirm(1u, "s", "m", false) { confirm = it }.answer(false)
        assertEquals(PagePrompt.Answer.CANCEL, confirm)
        var text: Pair<PagePrompt.Answer, String>? = null
        PagePrompt.Text(1u, "s", "m", "default", true) { a, t -> text = a to t }.answer("typed")
        assertEquals(PagePrompt.Answer.OK to "typed", text)
        PagePrompt.Text(1u, "s", "m", "default", true) { a, t -> text = a to t }.block()
        assertEquals(PagePrompt.Answer.BLOCK to "", text)
        var picked: List<String>? = null
        var dismissed = false
        val choice = PagePrompt.Choice(1u, "s", "", false, listOf(PagePrompt.Option("0", "A")), { picked = it }, {
            dismissed =
                true
        })
        choice.answer(listOf("0"))
        assertEquals(listOf("0"), picked)
        choice.block() // lists cannot be blocked: the same as dismissing
        assertTrue(dismissed)
    }

    @Test
    fun aPageCannotKeepTheUserInDialogs() {
        val t = PromptThrottle()
        // The first dialog of a document is plain, from the second on blocking is offered.
        assertEquals(false, t.next())
        assertEquals(true, t.next())
        assertEquals(true, t.next())
        t.block()
        assertTrue(t.blocked)
        assertNull(t.next())
        assertNull(t.next())
        // Another document starts fresh.
        t.newDocument()
        assertFalse(t.blocked)
        assertEquals(false, t.next())
    }
}
