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
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
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
import org.krunk.ffi.bookmarkable

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

    /** Whether any private tab is open (the tab switcher shows their pictures). */
    val hasPrivateTabs: StateFlow<Boolean> = tabs.tabs.map { list -> list.any { it.private } }
        .distinctUntilChanged()
        .stateIn(viewModelScope, SharingStarted.Eagerly, false)

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

    /** A suggestion's arrow writes into the editor: (sequence, text). The editor applies it once. */
    private val _fill = MutableStateFlow(0 to "")
    val fill: StateFlow<Pair<Int, String>> = _fill.asStateFlow()

    fun fillTyped(text: String) {
        _fill.value = (_fill.value.first + 1) to text
        setTyped(text)
    }
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

    // ----- find in page -----

    private val _findActive = MutableStateFlow(false)
    val findActive: StateFlow<Boolean> = _findActive.asStateFlow()
    private val _findText = MutableStateFlow("")
    val findText: StateFlow<String> = _findText.asStateFlow()

    fun openFind() {
        stopEditing()
        _findActive.value = true
    }

    /** New search text: an empty string clears the highlights. */
    fun findTyped(text: String) {
        _findText.value = text
        val s = tabs.activeSession() ?: return
        if (text.isEmpty()) s.clearFind() else s.find(text, forward = true)
    }

    fun findNext(forward: Boolean) {
        if (_findText.value.isEmpty()) return
        tabs.activeSession()?.find(null, forward)
    }

    fun closeFind() {
        if (!_findActive.value) return
        _findActive.value = false
        _findText.value = ""
        tabs.activeSession()?.clearFind()
    }

    // ----- bookmarks (star state of the active page) -----

    private val bookmarkOverride = MutableStateFlow<Pair<String, Boolean>?>(null)

    /**
     * Whether the active tab's URL is bookmarked; queried again when the URL changes and when
     * another screen (the bookmark manager may have removed it) is left.
     */
    val bookmarked: StateFlow<Boolean> = combine(activeTab, screens) { tab, stack -> (tab?.url ?: "") to stack.size }
        .distinctUntilChanged()
        .map { (url, _) ->
            bookmarkOverride.value = null
            url.isNotEmpty() && bookmarkable(url) &&
                core.io { store -> runCatching { store.isBookmarked(url) }.getOrDefault(false) }
        }
        .stateIn(viewModelScope, SharingStarted.Eagerly, false)

    /** The star state to show: the last toggle wins over the stored state for the same URL. */
    val bookmarkedShown: StateFlow<Boolean> = combine(bookmarked, bookmarkOverride, activeTab) { stored, over, tab ->
        if (over != null && over.first == tab?.url) over.second else stored
    }.stateIn(viewModelScope, SharingStarted.Eagerly, false)

    fun canBookmark(tab: TabUi?): Boolean = tab != null && tab.url.isNotEmpty() && bookmarkable(tab.url)

    /** Adds or removes the bookmark for the active page; returns the new state, null if not bookmarkable. */
    suspend fun toggleBookmark(): Boolean? {
        val tab = activeTab.value ?: return null
        if (!canBookmark(tab)) return null
        val now = core.io { it.toggleBookmark(tab.url, tab.title.ifEmpty { tab.url }) }
        bookmarkOverride.value = tab.url to now
        return now
    }

    suspend fun updateSettings(transform: (AndroidSettings) -> AndroidSettings): Result<Unit> {
        val r = settingsRepo.update(transform)
        if (r.isSuccess) engine.applySettings(settingsRepo.current)
        return r
    }
}
