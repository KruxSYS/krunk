package org.krunk.browser.ui

import android.app.Application
import android.util.Log
import android.widget.Toast
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
import org.krunk.browser.R
import org.krunk.browser.core.KrunkCore
import org.krunk.browser.core.SettingsRepository
import org.krunk.browser.engine.GeckoEngine
import org.krunk.browser.engine.await
import org.krunk.browser.engine.blocker.AllowList
import org.krunk.browser.engine.blocker.BlockerStatus
import org.krunk.browser.engine.blocker.Shield
import org.krunk.browser.engine.blocker.UpdateState
import org.krunk.browser.tabs.TabManager
import org.krunk.browser.tabs.TabUi
import org.krunk.ffi.AddressSuggestion
import org.krunk.ffi.AndroidSettings
import org.krunk.ffi.FfiException
import org.krunk.ffi.bookmarkable

/**
 * Text a suggestion's arrow writes into the address editor. [sequence] is 0 while nothing is
 * pending; the editor applies a fill once, when the sequence changes to a value above 0.
 */
data class AddressFill(val sequence: Int, val text: String) {
    /** The next fill of this editing session. */
    fun next(text: String) = AddressFill(sequence + 1, text)

    /** Whether an editor showing [current] has to take this fill. */
    fun appliesTo(current: String): Boolean = sequence > 0 && text != current

    companion object {
        val NONE = AddressFill(0, "")
    }
}

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

    // ----- ad and tracker blocking -----

    val blockerStatus: StateFlow<BlockerStatus> get() = engine.blocker.status

    /** The blocker's reading per tab (count, off). */
    val shields: StateFlow<Map<UInt, Shield>> get() = engine.blocker.shields
    val filterUpdate: StateFlow<UpdateState> get() = engine.blocker.update

    private val _allowHosts = MutableStateFlow<List<String>>(emptyList())

    /** The sites where blocking is switched off (Krunk's allow list), sorted. */
    val allowHosts: StateFlow<List<String>> = _allowHosts.asStateFlow()
    private val _listsUpdated = MutableStateFlow<Long?>(null)

    /** When a user-triggered update last brought newer filter lists (Unix ms), if ever. */
    val listsUpdated: StateFlow<Long?> = _listsUpdated.asStateFlow()

    init {
        viewModelScope.launch { refreshBlocking() }
        viewModelScope.launch {
            filterUpdate.collect { if (it is UpdateState.Done) _listsUpdated.value = it.at }
        }
    }

    /** Reads the allow list and the last update time again (the Privacy settings do on opening). */
    fun reloadBlocking() {
        viewModelScope.launch { refreshBlocking() }
    }

    private suspend fun refreshBlocking() {
        _allowHosts.value = core.io { AllowList(it).hosts() }
        _listsUpdated.value = core.io { store ->
            runCatching { store.kvGet(AllowList.KV_LISTS_UPDATED)?.toLongOrNull() }.getOrNull()
        }
    }

    /**
     * The per-site switch of the shield's sheet: blocking off ([allowed]) or on again for the
     * site of [url]. The blocker gets the new list, then the tab is reloaded so the page shows
     * the change. Not offered in private tabs.
     */
    fun setSiteAllowed(tabId: UInt, url: String, allowed: Boolean) {
        viewModelScope.launch {
            core.io { store ->
                val list = AllowList(store)
                if (allowed) list.allow(url) else list.entryFor(url)?.let(list::remove)
            }
            refreshBlocking()
            engine.blocker.allowListChanged { tabs.reload(tabId) }
        }
    }

    /** Settings: a site is taken off the allow list (it is blocked again from its next load). */
    fun removeAllowedSite(host: String) {
        viewModelScope.launch {
            core.io { AllowList(it).remove(host) }
            refreshBlocking()
            engine.blocker.allowListChanged()
        }
    }

    /** Settings: "Update filter lists". */
    fun updateFilterLists() = engine.blocker.updateNow()

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
    private val _fill = MutableStateFlow(AddressFill.NONE)
    val fill: StateFlow<AddressFill> = _fill.asStateFlow()

    fun fillTyped(text: String) {
        _fill.value = _fill.value.next(text)
        setTyped(text)
    }
    private val _suggestions = MutableStateFlow<List<AddressSuggestion>>(emptyList())
    val suggestions: StateFlow<List<AddressSuggestion>> = _suggestions.asStateFlow()
    private var suggestJob: Job? = null
    val omnibox = Omnibox(krunk.core) { krunk.engine.defaultUserAgent }

    fun startEditing(initial: String) {
        // A new editing session starts from the address, not from the last suggestion that was
        // filled in (the editor applies any pending fill once when it appears).
        _fill.value = AddressFill.NONE
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

    /** The tab the find bar searches: the one that was active when it opened. */
    private var findTab: UInt? = null

    private fun findSession() = findTab?.let { tabs.session(it) }

    fun openFind() {
        stopEditing()
        findTab = activeTab.value?.id
        _findActive.value = true
    }

    /**
     * New search text: an empty string clears the highlights. The search runs after a short
     * pause in typing: the engine counts matches in the background, and a search started while
     * the previous one is still counting came back as "0/0" although the page showed the
     * matches (seen on the release build, M7).
     */
    fun findTyped(text: String) {
        _findText.value = text
        findJob?.cancel()
        val s = findSession() ?: return
        if (text.isEmpty()) {
            s.clearFind()
            return
        }
        findJob = viewModelScope.launch {
            delay(FIND_TYPING_PAUSE_MS)
            s.find(text, forward = true)
        }
    }

    private var findJob: Job? = null

    fun findNext(forward: Boolean) {
        val text = _findText.value
        if (text.isEmpty()) return
        val s = findSession() ?: return
        // A search still waiting for the typing pause runs now (Search on the keyboard).
        if (findJob?.isActive == true) {
            findJob?.cancel()
            s.find(text, forward)
        } else {
            s.find(null, forward)
        }
    }

    /** Closes the bar and clears the highlights in the tab it searched (not in another one). */
    fun closeFind() {
        if (!_findActive.value) return
        findJob?.cancel()
        _findActive.value = false
        _findText.value = ""
        findSession()?.clearFind()
        findTab = null
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
        // A database failure is "nothing changed", not a crash.
        val now = try {
            core.io { it.toggleBookmark(tab.url, tab.title.ifEmpty { tab.url }) }
        } catch (e: FfiException) {
            Log.w(KrunkApp.TAG, "bookmark could not be changed", e)
            return null
        }
        bookmarkOverride.value = tab.url to now
        return now
    }

    /**
     * Settings > Clear browsing data, and History > Clear all. History means everything Krunk
     * keeps about visited pages: visits, the favicon table (one row per visited host), the
     * recently closed list, remembered suggestions; open tabs stay. Cookies, cache and site data
     * are the engine's, and the call returns when it has finished. A failure is returned, not
     * thrown.
     */
    suspend fun clearBrowsingData(
        history: Boolean,
        cookies: Boolean = false,
        cache: Boolean = false,
        siteData: Boolean = false,
    ): Result<Unit> = try {
        if (siteData) tabs.closeAll()
        if (history) {
            core.io { store ->
                store.clearHistory()
                store.clearFavicons()
                // Rewrites the file, so the deleted rows are not left in free pages.
                store.vacuum()
            }
            tabs.forgetHistory()
            omnibox.clearCache()
        }
        if (cookies || cache || siteData) engine.clearData(cookies, cache, siteData).await()
        Result.success(Unit)
    } catch (e: kotlinx.coroutines.CancellationException) {
        throw e
    } catch (e: Throwable) {
        Log.w(KrunkApp.TAG, "clearing browsing data failed", e)
        Result.failure(e)
    }

    /** [clearBrowsingData] for the settings dialog: outlives the dialog and says how it went. */
    fun clearBrowsingDataAndReport(history: Boolean, cookies: Boolean, cache: Boolean, siteData: Boolean) {
        viewModelScope.launch {
            val ok = clearBrowsingData(history, cookies, cache, siteData).isSuccess
            val text = if (ok) R.string.settings_cleared else R.string.settings_clear_failed
            Toast.makeText(getApplication(), text, Toast.LENGTH_SHORT).show()
        }
    }

    suspend fun updateSettings(transform: (AndroidSettings) -> AndroidSettings): Result<Unit> {
        val r = settingsRepo.update(transform)
        if (r.isSuccess) engine.applySettings(settingsRepo.current)
        return r
    }

    private companion object {
        /** Long enough for a fast typist's next key, short enough to feel immediate. */
        const val FIND_TYPING_PAUSE_MS = 180L
    }
}
