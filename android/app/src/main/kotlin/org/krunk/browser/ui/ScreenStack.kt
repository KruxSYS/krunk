package org.krunk.browser.ui

/**
 * The screen stack's rules, free of Android and Compose classes so they can be tested on the
 * JVM. A stack is a list with the root screen (the browser) first and the screen on top last; it
 * is never empty and holds no screen twice.
 */
object ScreenStack {
    /**
     * [screen] on top. A screen that is already in the stack is moved to the top (it keeps its
     * state); asking for the root shows the root alone.
     */
    fun <S> push(stack: List<S>, screen: S): List<S> = when (screen) {
        stack.last() -> stack
        stack.first() -> root(stack)
        else -> stack.filterNot { it == screen } + screen
    }

    /** Without the screen on top; the root stays. */
    fun <S> pop(stack: List<S>): List<S> = if (stack.size <= 1) stack else stack.dropLast(1)

    /** The root alone. */
    fun <S> root(stack: List<S>): List<S> = stack.take(1)

    /**
     * The screens that are in composition, bottom first: the one on top, and under it the root
     * when [keepRoot] (the browser stays composed while covered so its page view is not built
     * again on return). Screens in between are not composed; their saved state waits.
     */
    fun <S> composed(stack: List<S>, keepRoot: Boolean): List<S> =
        if (keepRoot && stack.size > 1) listOf(stack.first(), stack.last()) else listOf(stack.last())
}

/**
 * Which screens have UI state saved for them (scroll position, open folder, section...), by
 * key. State lives as long as its screen is in the stack: being covered by another screen keeps
 * it, leaving the stack (Back, "show the browser") drops it, so a screen opened again starts
 * fresh.
 */
class ScreenStateKeys {
    private var kept: Set<String> = emptySet()

    /** The stack is now [keys]: returns the keys whose saved state is to be dropped. */
    fun update(keys: List<String>): Set<String> {
        val now = keys.toSet()
        val dropped = kept - now
        kept = now
        return dropped
    }
}
