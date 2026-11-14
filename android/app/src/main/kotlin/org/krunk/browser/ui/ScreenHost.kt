package org.krunk.browser.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.key
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.clearAndSetSemantics

/**
 * Whether the screen that reads it is the one on top of the stack. Only the root screen (the
 * browser) is ever composed while it is not: it stays in composition under the screen that
 * covers it, and has to keep still meanwhile. A covered screen must not
 *
 * - handle Back (`BackHandler(enabled = onTop && ...)`),
 * - open or keep a window of its own above the other screen: dialogs, sheets and menus are
 *   composed only while on top (`state?.takeIf { onTop }?.let { ... }`); what they ask waits,
 * - set window state (system bars) that belongs to the screen on top,
 * - show an Android view that takes focus, keys or touch (hide it).
 *
 * [ScreenHost] itself takes the covered screen out of the accessibility tree and of focus
 * traversal and covers it with an opaque surface that takes all touches.
 */
val LocalScreenOnTop = compositionLocalOf { true }

/**
 * Shows the screen on top of [stack] (root first, top last) and keeps the UI state of every
 * screen in the stack: each entry is composed inside its own saveable-state provider, so what a
 * screen holds with `rememberSaveable` (scroll positions of lazy lists included) survives being
 * covered by another screen. The state of a screen is dropped when it leaves the stack
 * ([ScreenStateKeys]). Back pops the stack while anything is above the root.
 *
 * With [keepRoot] the root screen stays in composition under the screen on top (see
 * [LocalScreenOnTop]): the browser's page view is then not built again on return. Screens
 * between the root and the top are not composed.
 *
 * Saved state reaches the activity's saved instance state: screens must not put secrets there
 * (the vault keeps its own state in `remember` and clears it when it leaves).
 */
@Composable
fun <S : Any> ScreenHost(
    stack: List<S>,
    key: (S) -> String,
    onPop: () -> Unit,
    keepRoot: Boolean = true,
    content: @Composable (S) -> Unit,
) {
    val holder = rememberSaveableStateHolder()
    val kept = remember { ScreenStateKeys() }
    val top = stack.last()
    val composed = ScreenStack.composed(stack, keepRoot)
    val rootCovered = composed.size > 1
    Surface(Modifier.fillMaxSize()) {
        // Before the screens: theirs are registered later and so are asked first.
        BackHandler(enabled = stack.size > 1, onBack = onPop)
        // Whatever had the focus under the new screen (a text field, with the keyboard) lets go.
        val focus = LocalFocusManager.current
        LaunchedEffect(rootCovered) { if (rootCovered) focus.clearFocus(force = true) }
        for (entry in composed) {
            val entryKey = key(entry)
            // Keyed, so the root is the same composition whether or not something covers it.
            key(entryKey) {
                val onTop = entry == top
                val covering = onTop && rootCovered
                Box(
                    if (onTop) {
                        Modifier.fillMaxSize()
                    } else {
                        // Covered: not for TalkBack or UI dumps, not a stop for keyboard focus.
                        Modifier.fillMaxSize().clearAndSetSemantics { }.focusProperties { canFocus = false }
                    },
                ) {
                    CompositionLocalProvider(LocalScreenOnTop provides onTop) {
                        if (covering) {
                            // Opaque, and a surface takes the touches that reach it.
                            Surface(Modifier.fillMaxSize()) {
                                holder.SaveableStateProvider(entryKey) { content(entry) }
                            }
                        } else {
                            holder.SaveableStateProvider(entryKey) { content(entry) }
                        }
                    }
                }
            }
        }
    }
    // After the composition that removed a screen (its provider has saved by then).
    val keys = stack.map(key)
    LaunchedEffect(keys) { kept.update(keys).forEach { holder.removeState(it) } }
}
