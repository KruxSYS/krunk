package org.krunk.browser.ui.browser

/**
 * The address bar's inline completion (0.7.4, like Firefox and Chrome): the core completes what
 * was typed to a site it knows ("you" → "youtube.com", see `core/src/omnibox.rs`), the editor
 * shows the rest selected, and Enter goes to [url]. [forText] is the typed text it was made for:
 * an answer for older text is never applied.
 */
data class InlineFill(val forText: String, val text: String, val url: String)

/**
 * The text the field shows for [typed] with [fill]: the typed characters as typed, then the rest
 * of the completion; null when [fill] is for other text or completes nothing.
 */
fun completedText(typed: String, fill: InlineFill?): String? {
    if (fill == null || typed.isEmpty() || fill.forText != typed) return null
    if (fill.text.length <= typed.length || !fill.text.startsWith(typed, ignoreCase = true)) return null
    return typed + fill.text.substring(typed.length)
}

/**
 * Whether going from [before] to [after] was typing forward (characters added at the end). Only
 * then is the text completed: after a backspace (which takes the completion away) or an edit in
 * the middle, the field keeps exactly what is there.
 */
fun typedForward(before: String, after: String): Boolean = after.length > before.length && after.startsWith(before)
