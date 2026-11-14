package org.krunk.browser.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.ui.Modifier

/**
 * Shows the screen on top of [stack] (root first, top last) and keeps the UI state of every
 * screen in the stack: each entry is composed inside its own saveable-state provider, so what a
 * screen holds with `rememberSaveable` (scroll positions of lazy lists included) survives being
 * covered by another screen. The state of a screen is dropped when it leaves the stack
 * ([ScreenStateKeys]). Back pops the stack while anything is above the root.
 *
 * Saved state reaches the activity's saved instance state: screens must not put secrets there
 * (the vault keeps its own state in `remember` and clears it when it leaves).
 */
@Composable
fun <S : Any> ScreenHost(stack: List<S>, key: (S) -> String, onPop: () -> Unit, content: @Composable (S) -> Unit) {
    val holder = rememberSaveableStateHolder()
    val kept = remember { ScreenStateKeys() }
    Surface(Modifier.fillMaxSize()) {
        // Before the screens: theirs are registered later and so are asked first.
        BackHandler(enabled = stack.size > 1, onBack = onPop)
        val top = stack.last()
        holder.SaveableStateProvider(key(top)) { content(top) }
    }
    // After the composition that removed a screen (its provider has saved by then).
    val keys = stack.map(key)
    LaunchedEffect(keys) { kept.update(keys).forEach { holder.removeState(it) } }
}
