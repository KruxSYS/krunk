package org.krunk.browser.tabs

import android.content.Context
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import org.krunk.browser.KrunkApp
import org.krunk.browser.core.KrunkCore
import org.krunk.browser.engine.ContextTarget
import org.krunk.browser.engine.ErrorPage
import org.krunk.browser.engine.GeckoEngine
import org.krunk.browser.engine.IconCandidate
import org.krunk.browser.engine.KrunkSession
import org.krunk.browser.engine.PageState
import org.krunk.browser.engine.SessionListener
import org.krunk.ffi.StartupMode
import org.krunk.ffi.TabEffect
import org.krunk.ffi.TabModel
import org.krunk.ffi.displayUrl
import org.krunk.ffi.inputToUrl
import org.krunk.ffi.internalPage
import org.krunk.ffi.navigationRewrite
import org.krunk.ffi.newTabUrl
import org.krunk.ffi.nowMs
import org.krunk.ffi.visitUrl
import org.mozilla.geckoview.GeckoSession
import org.mozilla.geckoview.WebResponse

/**
 * Owns the tabs: the core [TabModel] (order, active tab, lazy restore, recently closed) and one
 * [KrunkSession] per live tab. Persists the session to the core store like Windows does
 * (`kv["session"]`), plus Android-only Gecko state per tab, the recently closed list and a
 * clean-exit flag. Lives for the process (sessions survive activity recreation).
 */
class TabManager(context: Context, val core: KrunkCore, val engine: GeckoEngine) : SessionListener {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val model = TabModel()
    private val sessions = HashMap<UInt, KrunkSession>()
    private val errorPage = ErrorPage(context)
    private val geckoStates = HashMap<UInt, String>()

    private val _tabs = MutableStateFlow<List<TabUi>>(emptyList())
    val tabs: StateFlow<List<TabUi>> = _tabs.asStateFlow()
    private val _activeId = MutableStateFlow<UInt?>(null)
    val activeId: StateFlow<UInt?> = _activeId.asStateFlow()
    private val _events = MutableSharedFlow<TabEvent>(extraBufferCapacity = 16)
    val events: SharedFlow<TabEvent> = _events.asSharedFlow()

    /** Set by the memory/thumbnail layer: pictures for the tab switcher. */
    val thumbnails = HashMap<UInt, android.graphics.Bitmap>()
    val favicons = Favicons(core)

    @Volatile
    var started = false
        private set
    private var saveJob: Job? = null

    val activeTab: TabUi? get() = _activeId.value?.let { id -> _tabs.value.firstOrNull { it.id == id } }

    fun session(id: UInt): KrunkSession? = sessions[id]

    fun activeSession(): KrunkSession? = _activeId.value?.let { sessions[it] }

    // ----- startup and persistence -----

    /**
     * Restores the stored session (startup setting `RestoreSession`, or always after a crash
     * when the user accepts) or opens the first tab. Idempotent.
     */
    fun start() {
        if (started) return
        started = true
        val settings = core.settings.current
        val store = core.store
        val cleanExit = store.kvGet(KV_CLEAN_EXIT) != "0"
        val sessionJson = store.kvGet(KV_SESSION)
        val wantsRestore = settings.startup == StartupMode.RESTORE_SESSION
        store.kvGet(KV_CLOSED)?.let { runCatching { model.restoreClosedTabsJson(it) } }
        val now = nowMs()
        var restored = false
        if (sessionJson != null && (wantsRestore || !cleanExit)) {
            if (!cleanExit && !wantsRestore) {
                val count = runCatching { JSONObject(sessionJson).optJSONArray("tabs")?.length() ?: 0 }.getOrDefault(0)
                if (count > 0) {
                    pendingRestore = sessionJson
                    _events.tryEmit(TabEvent.RestorePrompt(count))
                }
            } else {
                restored = restore(sessionJson, now)
            }
        }
        if (!restored && model.count() == 0u) {
            val url = startUrl(settings.startup)
            apply(model.open(url, false, null, true, true, now).effects)
        }
        store.kvSet(KV_CLEAN_EXIT, "0")
        refresh()
    }

    private var pendingRestore: String? = null

    /** The user chose "Restore" on the crash prompt. */
    fun restorePending(): Boolean {
        val json = pendingRestore ?: return false
        pendingRestore = null
        val now = nowMs()
        val onlyNewTab = _tabs.value.size == 1 && _tabs.value[0].isNewTab
        val old = if (onlyNewTab) _tabs.value[0].id else null
        val ok = restore(json, now)
        if (ok && old != null) apply(model.close(old, now))
        refresh()
        return ok
    }

    fun discardPendingRestore() {
        pendingRestore = null
    }

    private fun startUrl(startup: StartupMode): String = when (startup) {
        StartupMode.HOMEPAGE -> core.settings.current.homepage.ifBlank { "" }
        else -> core.settings.newTabTarget ?: ""
    }

    private fun restore(json: String, now: Long): Boolean {
        val effects = runCatching { model.restoreSessionJson(json, now) }
            .onFailure { Log.w(KrunkApp.TAG, "session restore failed", it) }
            .getOrNull() ?: return false
        // Gecko states are stored by position in the saved tab list.
        core.store.kvGet(KV_GECKO_STATES)?.let { raw ->
            runCatching {
                val arr = JSONArray(raw)
                val tabs = model.tabs()
                for (i in 0 until minOf(arr.length(), tabs.size)) {
                    val st = arr.optString(i, "")
                    if (st.isNotEmpty()) geckoStates[tabs[i].id] = st
                }
            }
        }
        apply(effects)
        return true
    }

    /** Debounced write of everything needed to come back after process death. */
    fun scheduleSave(delayMs: Long = 500) {
        saveJob?.cancel()
        saveJob = scope.launch {
            delay(delayMs)
            saveNow()
        }
    }

    fun saveNow() {
        val session = model.sessionJson()
        val closed = model.closedTabsJson()
        val states = JSONArray()
        for (t in model.tabs()) {
            if (t.private) continue
            val st = sessions[t.id]?.current?.sessionState ?: geckoStates[t.id] ?: ""
            states.put(st)
        }
        val statesJson = states.toString()
        scope.launch(Dispatchers.IO) {
            runCatching {
                core.store.kvSet(KV_SESSION, session)
                core.store.kvSet(KV_CLOSED, closed)
                core.store.kvSet(KV_GECKO_STATES, statesJson)
            }.onFailure { Log.w(KrunkApp.TAG, "session save failed", it) }
        }
    }

    /** The app is going to the background or exiting normally. */
    fun markCleanExit() {
        saveJob?.cancel()
        saveNow()
        scope.launch(Dispatchers.IO) { runCatching { core.store.kvSet(KV_CLEAN_EXIT, "1") } }
    }

    fun markRunning() {
        scope.launch(Dispatchers.IO) { runCatching { core.store.kvSet(KV_CLEAN_EXIT, "0") } }
    }

    // ----- tab operations -----

    /** Opens a tab; `url` empty = the new-tab page. Returns its id. */
    fun openTab(
        url: String = "",
        private: Boolean = false,
        activate: Boolean = true,
        opener: UInt? = null,
        live: Boolean = false,
    ): UInt {
        val target = if (url.isEmpty()) (core.settings.newTabTarget ?: "") else url
        val r = model.open(target, private, opener, activate, live, nowMs())
        apply(r.effects)
        refresh()
        scheduleSave()
        return r.id
    }

    fun activate(id: UInt) {
        apply(model.activate(id, nowMs()))
        refresh()
        scheduleSave()
    }

    fun close(id: UInt) {
        apply(model.close(id, nowMs()))
        geckoStates.remove(id)
        thumbnails.remove(id)
        refresh()
        scheduleSave()
    }

    fun closeAll(private: Boolean? = null) {
        val ids = model.tabs().filter { private == null || it.private == private }.map { it.id }
        val now = nowMs()
        for (id in ids) apply(model.close(id, now))
        ids.forEach {
            geckoStates.remove(it)
            thumbnails.remove(it)
        }
        refresh()
        scheduleSave()
    }

    fun reopenClosed(index: Int? = null): UInt? {
        val now = nowMs()
        val r = if (index == null) model.reopenClosed(now) else model.reopenClosedAt(index.toUInt(), now)
        r ?: return null
        apply(r.effects)
        refresh()
        scheduleSave()
        return r.id
    }

    fun closedTabs() = model.closedTabs()

    fun clearClosedTabs() {
        model.clearClosedTabs()
        scheduleSave()
    }

    fun moveTab(id: UInt, index: Int) {
        if (model.moveTab(id, index.toUInt())) {
            refresh()
            scheduleSave()
        }
    }

    /** Frees a background tab's session (memory policy); it reloads on activation. */
    fun unload(id: UInt) {
        sessions[id]?.current?.sessionState?.let { geckoStates[id] = it }
        apply(model.unload(id))
        refresh()
    }

    fun setAsleep(id: UInt, asleep: Boolean) {
        sessions[id]?.setActive(!asleep)
        model.setAsleep(id, asleep)
        refresh()
    }

    /** Typed address-bar text on the active tab: URL or search (core rules), then load. */
    fun navigateTyped(text: String) {
        val template = core.settings.searchTemplate
        val url = inputToUrl(text, template)
        navigate(url)
    }

    /** Loads a URL in the active tab (internal pages become native screens). */
    fun navigate(url: String, tabId: UInt? = activeId.value) {
        val id = tabId ?: return
        val page = internalPage(url)
        if (page != null && page != "newtab") {
            _events.tryEmit(TabEvent.OpenScreen(page))
            return
        }
        if (page == "newtab") {
            showNewTab(id)
            return
        }
        val s = core.settings.current
        val target = navigationRewrite(url, s.stripTrackingParams, s.httpsOnly, emptyList())?.url ?: url
        model.setUrl(id, target)
        model.setLoading(id, true)
        ensureSession(id)?.load(target)
        refresh()
        scheduleSave()
    }

    private fun showNewTab(id: UInt) {
        model.setUrl(id, newTabUrl())
        model.setTitle(id, "")
        sessions[id]?.load("about:blank")
        refresh()
        scheduleSave()
    }

    fun reload(id: UInt? = activeId.value) {
        id?.let { ensureSession(it)?.reload() }
    }

    fun setDesktopMode(id: UInt, desktop: Boolean) {
        sessions[id]?.setDesktopMode(desktop)
    }

    // ----- effects -----

    private fun apply(effects: List<TabEffect>) {
        for (e in effects) {
            when (e) {
                is TabEffect.CreateView -> createSession(e.id)

                is TabEffect.DestroyView -> destroySession(e.id)

                is TabEffect.Activate -> {
                    e.from?.let { sessions[it]?.setActive(false) }
                    sessions[e.to]?.setActive(true)
                    _activeId.value = e.to
                }
            }
        }
    }

    private fun createSession(id: UInt): KrunkSession? {
        val info = model.tab(id) ?: return null
        sessions[id]?.let { return it }
        val s = KrunkSession(
            id = id,
            private = info.private,
            engine = engine,
            listener = this,
            errorPage = errorPage,
            recordHistory = !info.private,
        )
        sessions[id] = s
        geckoStates.remove(id)?.let { s.restoreState(it) }
        if (internalPage(info.url) == null) {
            s.load(info.url)
        }
        return s
    }

    private fun ensureSession(id: UInt): KrunkSession? {
        sessions[id]?.let { return it }
        if (model.tab(id) == null) return null
        val wasLive = model.tab(id)?.live == true
        if (!wasLive) apply(model.activate(id, nowMs()))
        return sessions[id] ?: createSession(id)
    }

    private fun destroySession(id: UInt) {
        sessions.remove(id)?.let { s ->
            s.current.sessionState?.let { geckoStates[id] = it }
            s.close()
        }
    }

    // ----- state -----

    fun refresh() {
        val list = model.tabs().map { t ->
            val page = sessions[t.id]?.current ?: PageState(url = t.url, title = t.title)
            val url = if (sessions[t.id] != null && page.url.isNotEmpty() &&
                page.url != "about:blank"
            ) {
                page.url
            } else {
                t.url
            }
            val internal = internalPage(url)
            TabUi(
                id = t.id,
                url = url,
                displayUrl = if (internal != null) displayUrl(url) else url,
                title = t.title.ifEmpty { page.title },
                private = t.private,
                live = t.live,
                internalPage = internal,
                page = page,
                favicon = favicons.cached(url),
                thumbnail = thumbnails[t.id],
            )
        }
        _tabs.value = list
        _activeId.value = model.activeId()
    }

    // ----- SessionListener -----

    override fun onVisited(session: KrunkSession, url: String) {
        if (session.private) return
        val stored = visitUrl(url, false) ?: return
        val title = session.current.title
        model.setUrl(session.id, url)
        scope.launch(Dispatchers.IO) {
            runCatching { core.store.recordVisit(stored, title, nowMs()) }
        }
        scheduleSave()
    }

    override fun onTitleChanged(session: KrunkSession, title: String) {
        model.setTitle(session.id, title)
        if (!session.private) {
            val url = session.current.url
            visitUrl(url, false)?.let { stored ->
                scope.launch(Dispatchers.IO) { runCatching { core.store.setTitle(stored, title) } }
            }
        }
        refresh()
        scheduleSave()
    }

    override fun onNewSessionRequested(session: KrunkSession, uri: String): GeckoSession? {
        val r = model.open(uri, session.private, session.id, true, true, nowMs())
        // The engine loads the new session itself: create it without a load.
        val info = model.tab(r.id) ?: return null
        val s = KrunkSession(r.id, info.private, engine, this, errorPage, !info.private)
        sessions[r.id] = s
        apply(r.effects.filterNot { it is TabEffect.CreateView })
        refresh()
        scheduleSave()
        return s.gecko
    }

    override fun onContextMenu(session: KrunkSession, target: ContextTarget) {
        _events.tryEmit(TabEvent.ContextMenu(session.id, target))
    }

    override fun onExternalResponse(session: KrunkSession, response: WebResponse) {
        _events.tryEmit(TabEvent.Download(session.id, response))
    }

    override fun onIcons(session: KrunkSession, pageUrl: String, icons: List<IconCandidate>) {
        if (session.private) return
        scope.launch { favicons.fetch(pageUrl, icons) { refresh() } }
    }

    override fun onStateChanged(session: KrunkSession) {
        val st = session.current
        model.setLoading(session.id, st.loading)
        if (st.url.isNotEmpty() && st.url != "about:blank") model.setUrl(session.id, st.url)
        refresh()
    }

    override fun onCloseRequested(session: KrunkSession) {
        close(session.id)
    }

    suspend fun <T> io(block: () -> T): T = withContext(Dispatchers.IO) { block() }

    companion object {
        const val KV_SESSION = "session"
        const val KV_CLOSED = "android.closed_tabs"
        const val KV_GECKO_STATES = "android.gecko_states"
        const val KV_CLEAN_EXIT = "android.clean_exit"
    }
}
