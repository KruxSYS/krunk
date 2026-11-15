package org.krunk.browser.core

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
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
    private val _settings = MutableStateFlow(loadMigrated())
    val settings: StateFlow<AndroidSettings> = _settings.asStateFlow()

    val current: AndroidSettings get() = _settings.value

    /** The search URL template in effect (engine preset or the custom URL). */
    val searchTemplate: String get() = settingsSearchTemplate(current)

    /** "Google", "DuckDuckGo", ... or the custom engine's host. */
    val searchName: String get() = settingsSearchName(current)

    /** `null` for Krunk's new-tab page, else the URL a new tab opens. */
    val newTabTarget: String? get() = settingsNewTabTarget(current)

    /**
     * Validates and saves. On a validation failure nothing is stored and the exception carries
     * the offending fields (`SettingsException.Validation`).
     */
    suspend fun update(transform: (AndroidSettings) -> AndroidSettings): Result<Unit> = writeLock.withLock {
        // One change at a time, each starting from the result of the one before: two quick
        // changes (a slider, two switches) can neither lose a field nor land out of order.
        val next = transform(current)
        withContext(Dispatchers.IO) {
            try {
                store.saveSettings(next)
                _settings.value = next
                Result.success(Unit)
            } catch (e: SettingsException) {
                Result.failure(e)
            }
        }
    }

    private val writeLock = Mutex()

    /**
     * The stored settings, moved off old defaults once ([DefaultsMigration]). A new install has
     * no settings row and gets the defaults the same way. Unreadable settings are not migrated
     * (and not overwritten): the defaults apply and the marker stays unset for the next start.
     */
    private fun loadMigrated(): AndroidSettings {
        val loaded = runCatching { store.loadSettings() }.getOrElse { return defaultSettings() }
        return runCatching {
            val from = store.kvGet(DefaultsMigration.KEY)?.toIntOrNull() ?: 0
            if (from >= DefaultsMigration.CURRENT) return loaded
            val next = DefaultsMigration.migrate(loaded, from)
            if (next != loaded) store.saveSettings(next)
            store.kvSet(DefaultsMigration.KEY, DefaultsMigration.CURRENT.toString())
            next
        }.getOrElse { loaded }
    }

    suspend fun reload() {
        _settings.value = withContext(Dispatchers.IO) { store.loadSettings() }
    }
}
