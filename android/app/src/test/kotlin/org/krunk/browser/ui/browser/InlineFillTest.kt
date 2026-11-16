package org.krunk.browser.ui.browser

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The address bar's inline completion (0.7.4). */
class InlineFillTest {
    private val yt = InlineFill(forText = "you", text = "youtube.com", url = "https://www.youtube.com/")

    @Test
    fun theTypedTextIsCompletedToTheSite() {
        assertEquals("youtube.com", completedText("you", yt))
        // What was typed stays as typed; only the added part comes from the completion.
        assertEquals("YOUtube.com", completedText("YOU", yt.copy(forText = "YOU")))
    }

    @Test
    fun nothingIsCompletedFromAnAnswerForOtherText() {
        assertNull(completedText("yo", yt))
        assertNull(completedText("you", null))
        assertNull(completedText("", yt.copy(forText = "")))
        // Nothing to add, or not a continuation of what was typed.
        assertNull(completedText("youtube.com", yt.copy(forText = "youtube.com")))
        assertNull(completedText("you", yt.copy(text = "example.com")))
    }

    @Test
    fun onlyTypingForwardCompletes() {
        assertTrue(typedForward("yo", "you"))
        assertTrue(typedForward("", "y"))
        // Backspace (the completion taken away, or a character deleted) and edits in the middle.
        assertFalse(typedForward("you", "you"))
        assertFalse(typedForward("yout", "you"))
        assertFalse(typedForward("you", "yxou"))
    }
}
