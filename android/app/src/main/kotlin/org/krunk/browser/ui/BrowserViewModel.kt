package org.krunk.browser.ui

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import org.krunk.browser.KrunkApp
import org.krunk.browser.core.KrunkCore
import org.krunk.browser.core.SettingsRepository
import org.krunk.browser.engine.GeckoEngine
import org.krunk.browser.tabs.TabManager
import org.krunk.browser.tabs.TabUi
import org.krunk.ffi.AddressSuggestion
import org.krunk.ffi.AndroidSettings

/** Which native screen is on top of the browser. */
sealed class Screen {
    data object Browser : Screen()

    data object TabSwitcher : Screen()

    data object Bookmarks : Screen()

    data object History : Screen()

    data object Settings : Screen()

    data object About : Screen()

    companion object {
        /** `krunk://name` -> screen. */
        fun forPage(name: String): Screen? = when (name) {
            "bookmarks" -> Bookmarks
            "history" -> History
            "settings" -> Settings
            "about" -> About
            else -> null
        }
    }
}

/**
 * UI state for the whole app: tabs (from the process-wide [TabManager]), settings, the screen
 * stack and the address bar's editing state. One instance per activity.
 */
class BrowserViewModel(app: Application) : AndroidViewModel(app) {
    private val krunk = app as KrunkApp
    val core: KrunkCore get() = krunk.core
    val engine: GeckoEngine get() = krunk.engine
    val tabs: TabManager get() = krunk.tabs
    val settingsRepo: SettingsRepository get() = krunk.core.settings

    val settings: StateFlow<AndroidSettings> get() = settingsRepo.settings

    val activeTab: StateFlow<TabUi?> = combine(tabs.tabs, tabs.activeId) { list, id ->
        list.firstOrNull { it.id == id }
    }
        .stateIn(viewModelScope, SharingStarted.Eagerly, null)

    private val _screens = MutableStateFlow<List<Screen>>(listOf(Screen.Browser))
    val screens: StateFlow<List<Screen>> = _screens.asStateFlow()
    val screen: Screen get() = _screens.value.last()

    fun push(screen: Screen) {
        if (_screens.value.last() == screen) return
        _screens.value = _screens.value.filterNot { it == screen } + screen
    }

    fun pop(): Boolean {
        if (_screens.value.size <= 1) return false
        _screens.value = _screens.value.dropLast(1)
        return true
    }

    fun showBrowser() {
        _screens.value = listOf(Screen.Browser)
    }

    // ----- address bar -----

    private val _editing = MutableStateFlow(false)
    val editing: StateFlow<Boolean> = _editing.asStateFlow()
    private val _typed = MutableStateFlow("")
    val typed: StateFlow<String> = _typed.asStateFlow()
    private val _suggestions = MutableStateFlow<List<AddressSuggestion>>(emptyList())
    val suggestions: StateFlow<List<AddressSuggestion>> = _suggestions.asStateFlow()
    private var suggestJob: Job? = null
    val omnibox = Omnibox(krunk.core)

    fun startEditing(initial: String) {
        _typed.value = initial
        _editing.value = true
        querySuggestions(initial)
    }

    fun setTyped(text: String) {
        _typed.value = text
        querySuggestions(text)
    }

    fun stopEditing() {
        _editing.value = false
        _typed.value = ""
        _suggestions.value = emptyList()
        suggestJob?.cancel()
    }

    private fun querySuggestions(text: String) {
        suggestJob?.cancel()
        if (text.isBlank()) {
            _suggestions.value = emptyList()
            return
        }
        val private = activeTab.value?.private == true
        suggestJob = viewModelScope.launch {
            val local = omnibox.local(text)
            _suggestions.value = local.rows
            if (!private && settings.value.searchSuggestions) {
                delay(150)
                val merged = omnibox.withRemote(local.rows, text, settings.value.searchEngine)
                if (merged != null && _typed.value == text) _suggestions.value = merged
            }
        }
    }

    /** Enter in the address bar / a suggestion row chosen. */
    fun go(text: String) {
        stopEditing()
        if (text.isBlank()) return
        tabs.navigateTyped(text)
    }

    fun goTo(url: String) {
        stopEditing()
        tabs.navigate(url)
    }

    suspend fun updateSettings(transform: (AndroidSettings) -> AndroidSettings): Result<Unit> {
        val r = settingsRepo.update(transform)
        if (r.isSuccess) engine.applySettings(settingsRepo.current)
        return r
    }
}
