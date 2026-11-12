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

    @Test
    fun aFileFieldGetsThePickedFilesOrNone() {
        val got = ArrayList<List<String>>()
        var none = 0
        fun file(multiple: Boolean) =
            PagePrompt.File(1u, "s", FileRequest(multiple, listOf("image/png"), camera = false), {
                got +=
                    it
            }, { none++ })
        file(multiple = true).answer(listOf("content://a/1", "content://a/2"))
        assertEquals(listOf(listOf("content://a/1", "content://a/2")), got)
        // A field for one file gets one, whatever the picker returned.
        file(multiple = false).answer(listOf("content://a/1", "content://a/2"))
        assertEquals(listOf("content://a/1"), got.last())
        // A picker closed without a choice, Back, and the tab going away all give the page no file.
        file(multiple = true).answer(emptyList())
        file(multiple = false).dismiss()
        val queue = PromptQueue()
        queue.add(file(multiple = false))
        queue.dropTab(1u)
        assertEquals(3, none)
        assertEquals(2, got.size)
    }

    @Test
    fun aDateFieldIsAnsweredClearedOrLeftAsItWas() {
        val got = ArrayList<String>()
        var unchanged = 0
        fun date() = PagePrompt.DateTime(1u, "s", DateKind.DATE, "2026-11-12", "", "", { got += it }, { unchanged++ })
        date().answer("2026-12-24")
        date().clear()
        date().dismiss()
        assertEquals(listOf("2026-12-24", ""), got)
        assertEquals(1, unchanged)
    }

    @Test
    fun dismissingLeavePageStaysAndDismissingRepostDoesNotSend() {
        val leave = ArrayList<Boolean>()
        PagePrompt.LeavePage(1u, "s") { leave += it }.answer(true)
        PagePrompt.LeavePage(1u, "s") { leave += it }.answer(false)
        PagePrompt.LeavePage(1u, "s") { leave += it }.dismiss()
        assertEquals(listOf(true, false, false), leave)
        val send = ArrayList<Boolean>()
        PagePrompt.Repost(1u, "s") { send += it }.answer(true)
        PagePrompt.Repost(1u, "s") { send += it }.dismiss()
        assertEquals(listOf(true, false), send)
        // A closed tab's prompts take the same way out.
        val queue = PromptQueue()
        queue.add(PagePrompt.LeavePage(7u, "s") { leave += it })
        queue.add(PagePrompt.Repost(7u, "s") { send += it })
        queue.dropTab(7u)
        assertEquals(false, leave.last())
        assertEquals(false, send.last())
        assertEquals(0, queue.size)
    }

    @Test
    fun aDismissedShareIsAborted() {
        val got = ArrayList<ShareOutcome>()
        PagePrompt.Share(1u, "s", "Title", "text") { got += it }.result(ShareOutcome.SHARED)
        PagePrompt.Share(1u, "s", "Title", "text") { got += it }.dismiss()
        assertEquals(listOf(ShareOutcome.SHARED, ShareOutcome.ABORTED), got)
    }

    @Test
    fun aColourFieldIsAnsweredOrLeftAsItWas() {
        val got = ArrayList<String>()
        var unchanged = 0
        fun colour() = PagePrompt.Colour(1u, "s", "#336699", emptyList(), { got += it }, { unchanged++ })
        colour().answer("#ff0000")
        colour().dismiss()
        assertEquals(listOf("#ff0000"), got)
        assertEquals(1, unchanged)
    }

    @Test
    fun aPromptIsTitledWithTheSiteThatAsks() {
        assertEquals("example.com", promptSiteOf("https://example.com/path?q=1#top"))
        assertEquals("example.com", promptSiteOf("HTTPS://EXAMPLE.com"))
        assertEquals("10.250.136.3:18080", promptSiteOf("http://10.250.136.3:18080/pickers.html"))
        assertEquals("[::1]:8080", promptSiteOf("http://[::1]:8080/"))
        // A page cannot borrow another site's name with a user part or a look-alike path.
        assertEquals("evil.example", promptSiteOf("https://bank.example@evil.example/login"))
        assertEquals("evil.example", promptSiteOf("https://bank.example:pw@evil.example/"))
        assertEquals("evil.example", promptSiteOf("https://evil.example/https://bank.example/"))
        assertEquals("evil.example", promptSiteOf("https://evil.example\\@bank.example/"))
        // No site: the title says "This page".
        for (none in listOf(
            "",
            "about:blank",
            "file:///sdcard/page.html",
            "data:text/html,<p>x",
            "javascript:alert(1)",
        )) {
            assertEquals(none, "", promptSiteOf(none))
        }
    }
}
