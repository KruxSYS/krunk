package org.krunk.browser.vault

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * When the vault locks by itself: after [minutes] without vault use, and [graceSeconds] after
 * the app left the foreground (0 = at once).
 *
 * The shared core has this as `vaultAutolockMinutes`, but the settings record the app gets
 * from the core does not carry that field yet, so the value lives in its own `kv` row
 * ([KEY]) with the core's rule (1 to 1440 minutes, default 15). The background grace is
 * Android's own ([GRACE_KEY]; one of [GRACE_CHOICES], default 60 s). [read] and [write] are
 * the store's `kvGet` / `kvSet`; both may fail, and then the default or the last value applies.
 */
class VaultLockSetting(private val read: (String) -> String?, private val write: (String, String) -> Unit) {
    private val _minutes = MutableStateFlow(parse(runCatching { read(KEY) }.getOrNull()))
    val minutes: StateFlow<Int> = _minutes.asStateFlow()

    val current: Int get() = _minutes.value

    /** Stores [minutes] (blocking: call off the main thread). False if out of range or not stored. */
    fun set(minutes: Int): Boolean {
        if (!valid(minutes)) return false
        if (runCatching { write(KEY, minutes.toString()) }.isFailure) return false
        _minutes.value = minutes
        return true
    }

    private val _graceSeconds = MutableStateFlow(parseGrace(runCatching { read(GRACE_KEY) }.getOrNull()))
    val graceSeconds: StateFlow<Int> = _graceSeconds.asStateFlow()

    val currentGraceSeconds: Int get() = _graceSeconds.value

    /** Stores the grace (blocking). False if [seconds] is not one of [GRACE_CHOICES] or not stored. */
    fun setGraceSeconds(seconds: Int): Boolean {
        if (seconds !in GRACE_CHOICES) return false
        if (runCatching { write(GRACE_KEY, seconds.toString()) }.isFailure) return false
        _graceSeconds.value = seconds
        return true
    }

    companion object {
        const val KEY = "android.vault.lock_minutes"
        const val DEFAULT = VaultController.DEFAULT_LOCK_MINUTES
        const val MIN = 1
        const val MAX = 24 * 60

        /** What the settings screen offers, as on Windows. */
        val CHOICES = listOf(1, 5, 15, 30, 60)

        const val GRACE_KEY = "android.vault.background_grace_s"
        const val GRACE_DEFAULT = VaultController.DEFAULT_BACKGROUND_GRACE_SECONDS

        /** The only values allowed: at once, 30 s, 1 min, 5 min. */
        val GRACE_CHOICES = listOf(0, 30, 60, 300)

        /** A missing, unreadable or unlisted value is the default. */
        fun parseGrace(raw: String?): Int = raw?.trim()?.toIntOrNull()?.takeIf { it in GRACE_CHOICES } ?: GRACE_DEFAULT

        fun valid(minutes: Int): Boolean = minutes in MIN..MAX

        /** A missing, unreadable or out-of-range value is the default. */
        fun parse(raw: String?): Int = raw?.trim()?.toIntOrNull()?.takeIf(::valid) ?: DEFAULT
    }
}
