package org.krunk.browser.ui.browser

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SelectionActionsTest {
    private val google = "https://www.google.com/search?q={searchTerms}"
    private fun of(selection: String) = SelectionActions.of(selection, "Google", google) { "Open $it" }

    @Test
    fun selectedTextCanBeSearchedWithTheEngineInEffect() {
        assertEquals(
            listOf<SelectionAction>(
                SelectionAction.Search(
                    "Search Google for “krunk browser”",
                    "https://www.google.com/search?q=krunk+browser",
                ),
            ),
            of("krunk  browser\n"),
        )
        val custom = SelectionActions.of("privacy", "example.org", "https://example.org/find?term=%s") { "Open $it" }
        assertEquals(
            SelectionAction.Search("Search example.org for “privacy”", "https://example.org/find?term=privacy"),
            custom.single(),
        )
    }

    @Test
    fun aSelectedAddressCanBeOpenedAndIsListedFirst() {
        val actions = of("(example.com/docs).")
        assertEquals(SelectionAction.Open("Open example.com/docs", "https://example.com/docs"), actions[0])
        assertTrue(actions[1] is SelectionAction.Search)
        assertEquals(2, actions.size)
        assertEquals("https://sub.example.org/a?b=1", of("https://sub.example.org/a?b=1")[0].url)
    }

    @Test
    fun onlyWebAddressesAreOffered() {
        for (text in listOf("file:///etc/passwd", "javascript:alert(1)", "two words", "hello")) {
            assertTrue(text, of(text).none { it is SelectionAction.Open })
        }
    }

    @Test
    fun nothingForAnEmptyOrHugeSelection() {
        assertTrue(of("").isEmpty())
        assertTrue(of("  \n\t ").isEmpty())
        assertTrue(of("a".repeat(SelectionActions.MAX_SELECTION + 1)).isEmpty())
        assertEquals(1, of("a".repeat(SelectionActions.MAX_SELECTION)).size)
    }

    @Test
    fun krunksEntriesComeBeforeSelectAllSoTheyAreNotBehindMoreOptions() {
        val engine = listOf("PROCESS_TEXT", "CUT", "COPY", "PASTE", "SELECT_ALL", "UNSELECT")
        assertEquals(
            listOf("PROCESS_TEXT", "CUT", "COPY", "PASTE", "OPEN", "SEARCH", "SELECT_ALL", "UNSELECT"),
            SelectionActions.placed(engine, listOf("OPEN", "SEARCH"), "SELECT_ALL"),
        )
        assertEquals(
            listOf("COPY", "OPEN", "SEARCH"),
            SelectionActions.placed(listOf("COPY"), listOf("OPEN", "SEARCH"), "SELECT_ALL"),
        )
    }
}
