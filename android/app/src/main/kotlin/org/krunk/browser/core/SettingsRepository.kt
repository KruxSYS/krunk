package org.krunk.browser.core

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext
import org.krunk.ffi.AndroidSettings
import org.krunk.ffi.KrunkStore
import org.krunk.ffi.SettingsException
import org.krunk.ffi.defaultSettings
import org.krunk.ffi.settingsNewTabTarget
import org.krunk.ffi.settingsSearchName
import org.krunk.ffi.settingsSearchTemplate

/**
 * Settings as a [StateFlow]. Values come from `core::settings` (validated and stored by the
 * core, same JSON as Windows); Android-only fields ride along in the same record.
 */
class SettingsRepository(private val store: KrunkStore) {
    // Unreadable settings must not stop the app from starting: the defaults apply until a save.
    private val _settings = MutableStateFlow(runCatching { store.loadSettings() }.getOrElse { defaultSettings() })
    val settings: StateFlow<AndroidSettings> = _settings.asStateFlow()

    val current: AndroidSettings get() = _settings.value

    /** The search URL template in effect (engine preset or the custom URL). */
    val searchTemplate: String get() = settingsSearchTemplate(current)

    /** "DuckDuckGo", "Google", ... or the custom engine's host. */
    val searchName: String get() = settingsSearchName(current)

    /** `null` for Krunk's new-tab page, else the URL a new tab opens. */
    val newTabTarget: String? get() = settingsNewTabTarget(current)

    /**
     * Validates and saves. On a validation failure nothing is stored and the exception carries
     * the offending fields (`SettingsException.Validation`).
     */
    suspend fun update(transform: (AndroidSettings) -> AndroidSettings): Result<Unit> {
        val next = transform(current)
        return withContext(Dispatchers.IO) {
            try {
                store.saveSettings(next)
                _settings.value = next
                Result.success(Unit)
            } catch (e: SettingsException) {
                Result.failure(e)
            }
        }
    }

    suspend fun reload() {
        _settings.value = withContext(Dispatchers.IO) { store.loadSettings() }
    }
}
