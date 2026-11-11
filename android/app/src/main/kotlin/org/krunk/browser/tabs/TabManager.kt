package org.krunk.browser.tabs

import android.content.Context
import android.graphics.Bitmap
import android.os.SystemClock
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
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
import org.krunk.browser.engine.LoadPolicy
import org.krunk.browser.engine.LoadSource
import org.krunk.browser.engine.PageState
import org.krunk.browser.engine.SessionListener
import org.krunk.ffi.ClosedTab
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
import org.mozilla.geckoview.GeckoView
import org.mozilla.geckoview.WebResponse

/**
 * Owns the tabs: the core [TabModel] (order, active tab, lazy restore, recently closed) and one
 * [KrunkSession] per live tab. Persists the session to the core store like Windows does
 * (`kv["session"]`), plus Android-only Gecko state per tab, the recently closed list and a
 * clean-exit flag. Lives for the process (sessions survive activity recreation).
 *
 * Private tabs never reach the disk: the core leaves them out of the session and the closed
 * list, Gecko states and history skip them, favicons are not fetched, thumbnails stay in memory.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class TabManager(context: Context, val core: KrunkCore, val engine: GeckoEngine) :
    SessionListener,
    MemoryHost {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    /** One writer for the kv rows, so an older save can never land after a newer one. */
    private val persist = Dispatchers.IO.limitedParallelism(1)
    private val model = TabModel()
    private val sessions = HashMap<UInt, KrunkSession>()
    private val errorPage = ErrorPage(context)
    private val geckoStates = HashMap<UInt, String>()

    /** Tabs whose first load is for another app or a page (kept until the session loads it). */
    private val loadSources = HashMap<UInt, LoadOrigin>()

    private data class LoadOrigin(val source: LoadSource, val opener: UInt?)

    private val _tabs = MutableStateFlow<List<TabUi>>(emptyList())
    val tabs: StateFlow<List<TabUi>> = _tabs.asStateFlow()
    private val _activeId = MutableStateFlow<UInt?>(null)
    val activeId: StateFlow<UInt?> = _activeId.asStateFlow()
    private val _closedTabs = MutableStateFlow<List<ClosedTab>>(emptyList())

    /** Recently closed tabs, oldest first (as the core keeps them). */
    val closedTabs: StateFlow<List<ClosedTab>> = _closedTabs.asStateFlow()
    private val _events = MutableSharedFlow<TabEvent>(extraBufferCapacity = 16)
    val events: SharedFlow<TabEvent> = _events.asSharedFlow()
    private val _restorePrompt = MutableStateFlow<RestorePromptState?>(null)

    /** Non-null while the crash-restore offer is open. */
    val restorePrompt: StateFlow<RestorePromptState?> = _restorePrompt.asStateFlow()

    /** Pictures for the tab switcher (memory only, newest [THUMBNAIL_CAPACITY] tabs). */
    val thumbnails = LruMap<UInt, Bitmap>(THUMBNAIL_CAPACITY)
    val favicons = Favicons(core)

    /**
     * Set by the browser screen: the GeckoView showing the active tab, so a finished page load
     * can refresh the tab's thumbnail without a user action (optional).
     */
    var thumbnailView: (() -> GeckoView?)? = null
    private var lastCaptureAt = 0L
    private var autoCaptureJob: Job? = null

    private val memory = MemoryTicker(this) { Log.i(KrunkApp.TAG, it) }

    @Volatile
    var started = false
        private set
    private var saveJob: Job? = null
    private var pendingSaveDelay = Long.MAX_VALUE
    private var pendingRestore: String? = null

    val activeTab: TabUi? get() = _activeId.value?.let { id -> _tabs.value.firstOrNull { it.id == id } }

    fun session(id: UInt): KrunkSession? = sessions[id]

    fun activeSession(): KrunkSession? = _activeId.value?.let { sessions[it] }

    // ----- thumbnails -----

    /**
     * Takes a picture of the active tab for the tab switcher (the browser screen calls this
     * before the switcher opens and before a tab switch). Scaled to a third of the view's width;
     * `onDone` always runs exactly once, also when the compositor is not ready.
     */
    fun captureThumbnail(view: GeckoView, onDone: () -> Unit = {}) {
        val id = _activeId.value
        val session = id?.let { sessions[it] }
        if (id == null || session == null || view.session !== session.gecko) {
            onDone()
            return
        }
        // capturePixels() completes with the next composited frame; when none comes (a paused
        // compositor, an emulator without a GPU) the switcher must open without a picture.
        val gate = CaptureGate(
            CAPTURE_TIMEOUT_MS,
            { ms, action -> android.os.Handler(android.os.Looper.getMainLooper()).postDelayed(action, ms) },
            onDone,
        )
        val finish = { gate.finish() }
        try {
            view.capturePixels().accept(
                { bmp ->
                    if (bmp != null) storeThumbnail(id, bmp)
                    finish()
                },
                { e ->
                    Log.d(KrunkApp.TAG, "thumbnail for tab $id skipped: ${e?.message}")
                    finish()
                },
            )
        } catch (e: Exception) {
            Log.d(KrunkApp.TAG, "thumbnail for tab $id failed: ${e.message}")
            finish()
        }
    }

    private fun storeThumbnail(id: UInt, full: Bitmap) {
        lastCaptureAt = SystemClock.uptimeMillis()
        val w = (full.width / THUMBNAIL_SCALE).coerceAtLeast(1)
        val h = (full.height / THUMBNAIL_SCALE).coerceAtLeast(1)
        val small = Bitmap.createScaledBitmap(full, w, h, true)
        if (small !== full) full.recycle()
        thumbnails[id] = small
        if (model.tab(id) != null) refresh()
    }

    /** A page finished loading in the active tab: refresh its picture (throttled, if a view is known). */
    private fun scheduleAutoCapture(id: UInt) {
        if (thumbnailView == null) return
        autoCaptureJob?.cancel()
        autoCaptureJob = scope.launch {
            delay(AUTO_CAPTURE_DELAY_MS)
            if (_activeId.value != id) return@launch
            if (SystemClock.uptimeMillis() - lastCaptureAt < AUTO_CAPTURE_MIN_GAP_MS) return@launch
            thumbnailView?.invoke()?.let { captureThumbnail(it) }
        }
    }

    // ----- startup and persistence -----

    /**
     * Restores the stored session (startup setting `RestoreSession`, or always after a crash
     * when the user accepts) or opens the first tab. Idempotent; runs on the caller's thread
     * (four small kv reads) so the first tab exists before the first frame.
     */
    fun start() {
        if (started) return
        started = true
        val settings = core.settings.current
        val store = core.store
        val cleanExit = store.kvGet(KV_CLEAN_EXIT) != "0"
        val sessionJson = store.kvGet(KV_SESSION)
        store.kvGet(KV_CLOSED)?.let { runCatching { model.restoreClosedTabsJson(it) } }
        val now = nowMs()
        var restored = false
        when (val plan = SessionPersistence.plan(settings.startup, cleanExit, sessionJson, ::countRestorable)) {
            StartupPlan.Fresh -> Unit

            is StartupPlan.Restore -> restored = restore(plan.sessionJson, now)

            is StartupPlan.Prompt -> {
                Log.i(KrunkApp.TAG, "previous run did not exit cleanly: offering ${plan.tabCount} tabs")
                pendingRestore = plan.sessionJson
                _restorePrompt.value = RestorePromptState(plan.tabCount)
                _events.tryEmit(TabEvent.RestorePrompt(plan.tabCount))
            }
        }
        if (!restored && model.count() == 0u) {
            val url = startUrl(settings.startup)
            apply(model.open(url, false, null, true, true, now).effects)
        }
        store.kvSet(KV_CLEAN_EXIT, "0")
        refresh()
        startMemoryPolicy()
    }

    /** How many tabs a stored session would bring back (the core skips unrestorable URLs). */
    private fun countRestorable(json: String): Int {
        val probe = TabModel()
        return try {
            probe.restoreSessionJson(json, 0)
            probe.tabs().count { !it.private }
        } catch (e: Exception) {
            0
        } finally {
            probe.destroy()
        }
    }

    /** The user chose "Restore" on the crash prompt. */
    fun restorePending(): Boolean {
        val json = pendingRestore ?: return false
        pendingRestore = null
        _restorePrompt.value = null
        val now = nowMs()
        val onlyNewTab = _tabs.value.size == 1 && _tabs.value[0].isNewTab
        val old = if (onlyNewTab) _tabs.value[0].id else null
        val ok = restore(json, now)
        if (ok && old != null) apply(model.close(old, now))
        refresh()
        scheduleSave()
        return ok
    }

    /** The user chose "Start fresh": the stored session is replaced by the next save. */
    fun discardPendingRestore() {
        pendingRestore = null
        _restorePrompt.value = null
        scheduleSave()
    }

    private fun startUrl(startup: StartupMode): String = when (startup) {
        StartupMode.HOMEPAGE -> core.settings.current.homepage.ifBlank { "" }
        else -> core.settings.newTabTarget ?: ""
    }

    private fun restore(json: String, now: Long): Boolean {
        val before = model.tabs().map { it.id }.toSet()
        val effects = runCatching { model.restoreSessionJson(json, now) }
            .onFailure { Log.w(KrunkApp.TAG, "session restore failed", it) }
            .getOrNull() ?: return false
        val restoredTabs = model.tabs().filter { it.id !in before && !it.private }.map { it.id to it.url }
        val saved = core.store.kvGet(KV_GECKO_STATES)?.let(::decodeStates) ?: emptyList()
        geckoStates.putAll(SessionPersistence.assignStates(restoredTabs, saved))
        apply(effects)
        return true
    }

    private fun decodeStates(raw: String): List<Pair<String, String>> = runCatching {
        val arr = JSONArray(raw)
        (0 until arr.length()).map { i ->
            when (val item = arr.opt(i)) {
                is JSONObject -> item.optString("url", "") to item.optString("state", "")

                // The first builds stored a bare array of states.
                is String -> "" to item

                else -> "" to ""
            }
        }
    }.getOrDefault(emptyList())

    /**
     * Debounced write of everything needed to come back after process death. A shorter delay
     * wins over a longer pending one; a longer request never postpones a pending shorter one.
     */
    fun scheduleSave(delayMs: Long = SAVE_DELAY_MS) {
        if (saveJob?.isActive == true && delayMs >= pendingSaveDelay) return
        saveJob?.cancel()
        pendingSaveDelay = delayMs
        saveJob = scope.launch {
            delay(delayMs)
            pendingSaveDelay = Long.MAX_VALUE
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
            states.put(JSONObject().put("url", t.url).put("state", st))
        }
        val statesJson = states.toString()
        scope.launch(persist) {
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
        pendingSaveDelay = Long.MAX_VALUE
        saveNow()
        scope.launch(persist) { runCatching { core.store.kvSet(KV_CLEAN_EXIT, "1") } }
    }

    fun markRunning() {
        scope.launch(persist) { runCatching { core.store.kvSet(KV_CLEAN_EXIT, "0") } }
    }

    // ----- memory policy -----

    private fun startMemoryPolicy() {
        scope.launch {
            core.settings.settings.collect { s ->
                val testSeconds = DebugProps.long(DebugProps.SLEEP_SECONDS)
                memory.policy = MemoryTicker.policyFor(s, testSeconds, DebugProps.flag(DebugProps.DISCARD_TEST))
                Log.i(
                    KrunkApp.TAG,
                    "memory policy: sleep ${memory.policy.sleepAfter} ms, suspend +${memory.policy.suspendDelay} ms, " +
                        "discard ${memory.policy.discardAfter} ms, tick ${memory.tickMs} ms" +
                        (if (testSeconds != null) " (debug.krunk.sleep_seconds=$testSeconds)" else ""),
                )
            }
        }
        scope.launch {
            while (isActive) {
                delay(memory.tickMs)
                memory.tick(nowMs())
            }
        }
    }

    override fun snapshot(): List<MemoryTabSnapshot> {
        val active = model.activeId()
        return model.tabs().map { t ->
            MemoryTabSnapshot(
                id = t.id,
                live = t.live,
                active = t.id == active,
                private = t.private,
                lastActive = t.lastActive,
            )
        }
    }

    override fun sleep(id: UInt) = setAsleep(id, true)

    override fun discard(id: UInt) = unload(id)

    // ----- tab operations -----

    /** Opens a tab for the user; `url` empty = the new-tab page. Returns its id. */
    fun openTab(
        url: String = "",
        private: Boolean = false,
        activate: Boolean = true,
        opener: UInt? = null,
        live: Boolean = false,
    ): UInt {
        val target = if (url.isEmpty()) (core.settings.newTabTarget ?: "") else url
        return open(target, LoadSource.USER, private, activate, opener, live)
    }

    /**
     * Opens a tab for an address that did not come from the user: another app's intent
     * ([LoadSource.EXTERNAL_APP]) or a link or image address of a page ([LoadSource.PAGE], with
     * the page's tab as [opener]). Only web addresses are opened, and the engine loads them
     * without the app's privileges. Returns null when the address is refused.
     */
    fun openUntrusted(
        url: String,
        source: LoadSource,
        private: Boolean = false,
        activate: Boolean = true,
        opener: UInt? = null,
        live: Boolean = false,
    ): UInt? {
        if (source == LoadSource.USER || !LoadPolicy.isWebUrl(url)) {
            Log.w(KrunkApp.TAG, "refused to open an address that is not a web address (source $source)")
            return null
        }
        return open(url, source, private, activate, opener, live)
    }

    private fun open(
        target: String,
        source: LoadSource,
        private: Boolean,
        activate: Boolean,
        opener: UInt?,
        live: Boolean,
    ): UInt {
        val r = model.open(target, private, opener, activate, live, nowMs())
        if (source != LoadSource.USER) loadSources[r.id] = LoadOrigin(source, opener)
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
        forget(id)
        refresh()
        scheduleSave()
    }

    /** Closes every tab, or only the normal/private ones. */
    fun closeAll(private: Boolean? = null) {
        val ids = model.tabs().filter { private == null || it.private == private }.map { it.id }
        val now = nowMs()
        for (id in ids) {
            apply(model.close(id, now))
            forget(id)
        }
        refresh()
        scheduleSave()
    }

    private fun forget(id: UInt) {
        geckoStates.remove(id)
        loadSources.remove(id)
        thumbnails.remove(id)
        lastSeen.remove(id)
        memory.wake(id)
    }

    /** Reopens a recently closed tab (`index` into [closedTabs], default the newest). */
    fun reopenClosed(index: Int? = null): UInt? {
        val now = nowMs()
        val r = if (index == null) model.reopenClosed(now) else model.reopenClosedAt(index.toUInt(), now)
        r ?: return null
        apply(r.effects)
        refresh()
        scheduleSave()
        return r.id
    }

    fun clearClosedTabs() {
        model.clearClosedTabs()
        refresh()
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
        memory.wake(id)
        refresh()
    }

    fun setAsleep(id: UInt, asleep: Boolean) {
        if (model.activeId() == id && asleep) return
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
        thumbnails.remove(id)
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
                    memory.wake(e.to)
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
        val origin = loadSources.remove(id)
        if (internalPage(info.url) == null) {
            s.load(info.url, origin?.source ?: LoadSource.USER, origin?.opener?.let { sessions[it]?.gecko })
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
                asleep = t.asleep,
                internalPage = internal,
                page = page,
                favicon = if (t.private) null else favicons.cached(url),
                thumbnail = thumbnails[t.id],
            )
        }
        _tabs.value = list
        _activeId.value = model.activeId()
        _closedTabs.value = model.closedTabs()
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

    private val lastSeen = HashMap<UInt, PageState>()

    override fun onStateChanged(session: KrunkSession) {
        val st = session.current
        val prev = lastSeen.put(session.id, st)
        model.setLoading(session.id, st.loading)
        if (st.url.isNotEmpty() && st.url != "about:blank") model.setUrl(session.id, st.url)
        // Back/forward history and scroll position must survive a crash: persist state changes
        // too, but lazily (Gecko reports them on every scroll).
        if (!session.private && st.sessionState != null && st.sessionState != prev?.sessionState) {
            scheduleSave(STATE_SAVE_DELAY_MS)
        }
        if (prev?.loading == true && !st.loading && !st.crashed && session.id == _activeId.value) {
            scheduleAutoCapture(session.id)
        }
        refresh()
    }

    override fun onCloseRequested(session: KrunkSession) {
        close(session.id)
    }

    suspend fun <T> io(block: () -> T): T = withContext(Dispatchers.IO) { block() }

    companion object {
        const val CAPTURE_TIMEOUT_MS = 700L
        const val KV_SESSION = "session"
        const val KV_CLOSED = "android.closed_tabs"
        const val KV_GECKO_STATES = "android.gecko_states"
        const val KV_CLEAN_EXIT = "android.clean_exit"

        const val THUMBNAIL_CAPACITY = 12
        const val THUMBNAIL_SCALE = 3
        const val SAVE_DELAY_MS = 500L
        const val STATE_SAVE_DELAY_MS = 3000L
        const val AUTO_CAPTURE_DELAY_MS = 700L
        const val AUTO_CAPTURE_MIN_GAP_MS = 2000L
    }
}
