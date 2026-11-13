package org.krunk.browser.ui.vault

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

/**
 * Counts the vault surfaces on screen (the vault screen, an unlock dialog, a picker, a save
 * prompt). While there is at least one, the activity's window is marked secure: no screenshot,
 * no screen recording, no Recents thumbnail. A count and not a flag, because a dialog can open
 * over the screen and either can go first.
 */
class SecureSurfaceCounter {
    private val count = MutableStateFlow(0)
    private val _secure = MutableStateFlow(false)

    /** True while at least one surface is showing. */
    val secure: StateFlow<Boolean> = _secure.asStateFlow()

    fun acquire() {
        count.update { it + 1 }
        _secure.value = count.value > 0
    }

    /** A release without an acquire is ignored: the count never goes below zero. */
    fun release() {
        count.update { (it - 1).coerceAtLeast(0) }
        _secure.value = count.value > 0
    }

    val surfaces: Int get() = count.value
}

/** The app's one counter; `KrunkRoot` reads it for `SecureWindowEffect`. */
val VaultSecureFlag = SecureSurfaceCounter()

/** Put into every composable that shows vault content or asks for the vault password. */
@Composable
fun SecureVaultSurface() {
    DisposableEffect(Unit) {
        VaultSecureFlag.acquire()
        onDispose { VaultSecureFlag.release() }
    }
}
