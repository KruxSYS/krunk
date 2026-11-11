package org.krunk.browser.tabs

import android.content.Context
import android.graphics.Bitmap
import android.os.SystemClock
import android.util.Log
import kotlinx.coroutines.CoroutineExceptionHandler
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
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
import org.krunk.browser.engine.displayedUrl
import org.krunk.browser.engine.modelUrlAfter
import org.krunk.ffi.ClosedTab
import org.krunk.ffi.KvEntry
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
    /** A failed background task (a favicon fetch, a save) is logged; it must not end the app. */
    private val taskFailed = CoroutineExceptionHandler { _, e -> Log.e(KrunkApp.TAG, "tab manager task failed", e) }
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate + taskFailed)

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

    // A channel, not a shared flow: an event raised while another screen is on top (a download
    // answered during the tab switcher) waits for the browser screen instead of being lost.
    private val _events = Channel<TabEvent>(capacity = 16, onBufferOverflow = BufferOverflow.DROP_OLDEST)
    val events: Flow<TabEvent> = _events.receiveAsFlow()
    private val _restorePrompt = MutableStateFlow<RestorePromptState?>(null)

    /** Non-null while the crash-restore offer is open. */
    val restorePrompt: StateFlow<RestorePromptState?> = _restorePrompt.asStateFlow()

    /** Pictures for the tab switcher (memory only, newest [THUMBNAIL_CAPACITY] tabs). */
    val thumbnails = LruMap<UInt, Bitmap>(THUMBNAIL_CAPACITY)
    val favicons = Favicons(core) { engine.defaultUserAgent }

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

    /** The crashed session on offer ("Restore tabs?"), with its tabs' states as stored then. */
    private var pendingRestore: PendingRestore? = null

    private class PendingRestore(val sessionJson: String, val states: List<Pair<String, String>>)

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
     * Restores the stored session (startup setting `RestoreSession`, the system bringing the
     * task back, or after a crash when the user accepts) or opens the first tab. Idempotent;
     * runs on the caller's thread (a few small kv reads) so the first tab exists before the
     * first frame. [restoredBySystem]: the activity was created with saved instance state.
     * A store that fails here must not stop the app from starting: each read falls back.
     */
    fun start(restoredBySystem: Boolean = false) {
        if (started) return
        started = true
        val settings = core.settings.current
        fun read(key: String): String? = runCatching { core.store.kvGet(key) }
            .onFailure { Log.w(KrunkApp.TAG, "could not read $key", it) }
            .getOrNull()
        val cleanExit = read(KV_CLEAN_EXIT) != "0"
        val unclean = SessionPersistence.uncleanStarts(read(KV_UNCLEAN_STARTS)?.toIntOrNull() ?: 0, cleanExit)
        val sessionJson = read(KV_SESSION)
        read(KV_CLOSED)?.let { runCatching { model.restoreClosedTabsJson(it) } }
        val now = nowMs()
        var restored = false
        val plan = SessionPersistence.plan(
            settings.startup,
            cleanExit,
            sessionJson,
            uncleanStarts = unclean,
            restoredBySystem = restoredBySystem,
            countTabs = ::countRestorable,
        )
        when (plan) {
            StartupPlan.Fresh -> Unit

            is StartupPlan.Restore -> restored = restore(plan.sessionJson, now, null)

            is StartupPlan.Prompt -> {
                Log.i(
                    KrunkApp.TAG,
                    "previous run did not exit cleanly ($unclean in a row): offering ${plan.tabCount} tabs",
                )
                // The offer keeps the stored session and its states as they are now: while it is
                // open nothing overwrites them (see saveNow and markCleanExit).
                val states = read(KV_GECKO_STATES)?.let(::decodeStates) ?: emptyList()
                pendingRestore = PendingRestore(plan.sessionJson, states)
                _restorePrompt.value = RestorePromptState(plan.tabCount)
            }
        }
        if (!restored && model.count() == 0u) {
            val url = startUrl(settings.startup)
            apply(model.open(url, false, null, true, true, now).effects)
        }
        scope.launch(persist) {
            runCatching {
                core.store.kvSetMany(
                    listOf(KvEntry(KV_CLEAN_EXIT, "0"), KvEntry(KV_UNCLEAN_STARTS, unclean.toString())),
                )
            }
        }
        refresh()
        startMemoryPolicy()
    }

    /** How many tabs a stored session would bring back (the core skips unrestorable URLs). */
    private fun countRestorable(json: String): Int {
        val probe = TabModel()
        return try {
            probe.restoreSessionJson(json, 0)
            // The core adds a new-tab page when nothing could be restored: that is not a tab to offer.
            probe.tabs().count { !it.private && internalPage(it.url) != "newtab" }
        } catch (e: Exception) {
            0
        } finally {
            probe.destroy()
        }
    }

    /** The user chose "Restore" on the crash prompt. */
    fun restorePending(): Boolean {
        val offer = pendingRestore ?: return false
        pendingRestore = null
        _restorePrompt.value = null
        val now = nowMs()
        val onlyNewTab = _tabs.value.size == 1 && _tabs.value[0].isNewTab
        val old = if (onlyNewTab) _tabs.value[0].id else null
        val ok = restore(offer.sessionJson, now, offer.states)
        if (ok && old != null) {
            apply(model.close(old, now))
            forget(old)
        }
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

    /** [states]: the tabs' saved states when already read (a pending offer), else read from the store. */
    private fun restore(json: String, now: Long, states: List<Pair<String, String>>?): Boolean {
        val before = model.tabs().map { it.id }.toSet()
        val effects = runCatching { model.restoreSessionJson(json, now) }
            .onFailure { Log.w(KrunkApp.TAG, "session restore failed", it) }
            .getOrNull() ?: return false
        val restoredTabs = model.tabs().filter { it.id !in before && !it.private }.map { it.id to it.url }
        val saved = states
            ?: runCatching { core.store.kvGet(KV_GECKO_STATES) }.getOrNull()?.let(::decodeStates)
            ?: emptyList()
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
        // An open "Restore tabs?" offer owns the stored session: saving now would replace the
        // crashed session with the fresh one before the user has answered.
        if (pendingRestore != null) return
        val session = model.sessionJson()
        val closed = model.closedTabsJson()
        // Only references are collected here; the (possibly large) JSON is built on the writer.
        val states = model.tabs().filter { !it.private }.map { t ->
            t.url to (sessions[t.id]?.current?.sessionState ?: geckoStates[t.id] ?: "")
        }
        scope.launch(persist) {
            runCatching {
                // One transaction: the session and its states can never be stored out of step.
                core.store.kvSetMany(
                    listOf(
                        KvEntry(KV_SESSION, session),
                        KvEntry(KV_CLOSED, closed),
                        KvEntry(KV_GECKO_STATES, encodeStates(states)),
                    ),
                )
            }.onFailure { Log.w(KrunkApp.TAG, "session save failed", it) }
        }
    }

    private fun encodeStates(states: List<Pair<String, String>>): String {
        val arr = JSONArray()
        for ((url, state) in states) arr.put(JSONObject().put("url", url).put("state", state))
        return arr.toString()
    }

    /** The app is going to the background or exiting normally. */
    fun markCleanExit() {
        // With an offer open this run has not taken over the stored session: the next start must
        // offer it again, so the exit is not recorded as clean.
        if (pendingRestore != null) return
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
                // The user-agent setting reaches the open tabs too (their next load uses it).
                sessions.values.forEach { it.setUserAgent(s.userAgent) }
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
        // Tracking parameters go before the first request, also for links from other apps and
        // from the link menu (HTTPS-only is the engine's job: it knows its exceptions).
        val url = if (target.isEmpty() || internalPage(target) != null) target else cleaned(target)
        val r = model.open(url, private, opener, activate, live, nowMs())
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
        val ids = SessionPersistence.closeOrder(
            model.tabs().filter { private == null || it.private == private }.map { it.id },
            model.activeId(),
        )
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

    /**
     * "Clear browsing history": what the tabs keep about pages that are no longer open. The
     * recently closed list and the icons in memory go, and the stored session is rewritten at
     * once. Open tabs stay as they are, with their own back/forward lists.
     */
    fun forgetHistory() {
        model.clearClosedTabs()
        favicons.clear()
        refresh()
        saveJob?.cancel()
        pendingSaveDelay = Long.MAX_VALUE
        saveNow()
    }

    fun moveTab(id: UInt, index: Int) {
        if (model.moveTab(id, index.toUInt())) {
            refresh()
            scheduleSave()
        }
    }

    /** Frees a background tab's session (memory policy); it reloads on activation. */
    fun unload(id: UInt) {
        unloadQuietly(id)
        refresh()
    }

    private fun unloadQuietly(id: UInt) {
        sessions[id]?.current?.sessionState?.let { geckoStates[id] = it }
        apply(model.unload(id))
        memory.wake(id)
    }

    /**
     * The system is short of memory: background tabs give up their sessions (one private tab
     * stays live, see [tabsToUnloadUnderPressure]) and the switcher's pictures are dropped; both
     * come back on demand.
     */
    fun onMemoryPressure() {
        val ids = tabsToUnloadUnderPressure(snapshot())
        Log.i(KrunkApp.TAG, "memory pressure: unloading ${ids.size} background tabs")
        ids.forEach(::unloadQuietly)
        thumbnails.clear()
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
            _events.trySend(TabEvent.OpenScreen(page))
            return
        }
        if (page == "newtab") {
            showNewTab(id)
            return
        }
        val target = cleaned(url)
        model.setUrl(id, target)
        model.setLoading(id, true)
        ensureSession(id)?.load(target)
        refresh()
        scheduleSave()
    }

    /** [url] without tracking parameters when the setting is on. */
    private fun cleaned(url: String): String {
        if (!core.settings.current.stripTrackingParams) return url
        return navigationRewrite(url, true, false, emptyList())?.url ?: url
    }

    private fun showNewTab(id: UInt) {
        model.setUrl(id, newTabUrl())
        model.setTitle(id, "")
        sessions[id]?.park()
        thumbnails.remove(id)
        refresh()
        scheduleSave()
    }

    fun reload(id: UInt? = activeId.value) {
        id ?: return
        if (sessions[id]?.current?.crashed == true) {
            // The engine closed the crashed session: a new one restores the page and its history.
            dropCrashedSession(id)
            createSession(id)
            refresh()
        } else {
            ensureSession(id)?.reload()
        }
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

    /**
     * A new session for a tab. With [restore] it comes back as it was (saved Gecko state, the
     * tab's address); without, it is empty and the caller loads something else.
     */
    private fun createSession(id: UInt, restore: Boolean = true): KrunkSession? {
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
        val state = geckoStates.remove(id)
        val origin = loadSources.remove(id)
        if (!restore) return s
        // A saved state brings the page back by itself (address, back/forward list, scroll
        // position, form data). Loading the address as well would be a second, new navigation
        // on top of it: a duplicate history entry and the restored position lost.
        if (state != null && s.restoreState(state)) return s
        if (internalPage(info.url) == null) {
            s.load(info.url, origin?.source ?: LoadSource.USER, origin?.opener?.let { sessions[it]?.gecko })
        }
        return s
    }

    /** The tab's session for a new load, created (or replaced after a crash) when needed. */
    private fun ensureSession(id: UInt): KrunkSession? {
        sessions[id]?.let { s ->
            if (!s.current.crashed) return s
            // Dead after a crash: an empty new session takes the load the caller is about to do.
            dropCrashedSession(id)
            return createSession(id, restore = false)
        }
        if (model.tab(id) == null) return null
        val wasLive = model.tab(id)?.live == true
        if (!wasLive) apply(model.activate(id, nowMs()))
        return sessions[id] ?: createSession(id)
    }

    /** Removes a session the engine closed after a crash, keeping its last state for a restore. */
    private fun dropCrashedSession(id: UInt) {
        sessions.remove(id)?.let { s ->
            s.current.sessionState?.let { geckoStates[id] = it }
            s.close()
        }
        lastSeen.remove(id)
    }

    private fun destroySession(id: UInt) {
        sessions.remove(id)?.let { s ->
            s.current.sessionState?.let { geckoStates[id] = it }
            s.close()
        }
    }

    // ----- state -----

    /** What the core says about an address, remembered: asked for every tab on every refresh. */
    private class UrlFacts(val internalPage: String?, val display: String)

    private val urlFactsCache = LruMap<String, UrlFacts>(URL_FACTS_CAPACITY)

    private fun urlFacts(url: String): UrlFacts {
        urlFactsCache[url]?.let { return it }
        val internal = internalPage(url)
        val facts = UrlFacts(internal, if (internal != null) displayUrl(url) else url)
        // Very long addresses (data:) are not worth keeping as keys.
        if (url.length <= 2048) urlFactsCache[url] = facts
        return facts
    }

    fun refresh() {
        val list = model.tabs().map { t ->
            val session = sessions[t.id]
            val page = session?.current ?: PageState(url = t.url, title = t.title)
            val url = displayedUrl(
                modelUrl = t.url,
                modelIsInternal = urlFacts(t.url).internalPage != null,
                page = page,
                hasSession = session != null,
                openedByPage = session?.openedByPage == true,
            )
            val facts = urlFacts(url)
            val internal = facts.internalPage
            TabUi(
                id = t.id,
                url = url,
                displayUrl = facts.display,
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
        // The title is this document's only when the engine is already at this address (the
        // state's title is emptied when a new document starts); otherwise the visit is stored
        // without one and the title event fills it in.
        val title = if (session.current.url == url) session.current.title else ""
        val ts = nowMs()
        // One writer for history rows: a visit and the title that follows land in that order.
        scope.launch(persist) {
            runCatching { core.store.recordVisit(stored, title, ts) }
                .onFailure { Log.w(KrunkApp.TAG, "a visit could not be recorded", it) }
        }
    }

    override fun onTitleChanged(session: KrunkSession, title: String) {
        model.setTitle(session.id, title)
        if (!session.private) {
            val url = session.current.url
            visitUrl(url, false)?.let { stored ->
                scope.launch(persist) {
                    runCatching { core.store.setTitle(stored, title) }
                        .onFailure { Log.w(KrunkApp.TAG, "a page title could not be stored", it) }
                }
            }
        }
        scheduleSave()
    }

    override fun onNewSessionRequested(session: KrunkSession, uri: String): GeckoSession? {
        val r = model.open(uri, session.private, session.id, true, true, nowMs())
        // The engine loads the new session itself: create it without a load.
        val info = model.tab(r.id) ?: return null
        // Unopened: the engine opens the session of a new window itself.
        val s = KrunkSession(r.id, info.private, engine, this, errorPage, !info.private, openedByPage = true)
        sessions[r.id] = s
        apply(r.effects.filterNot { it is TabEffect.CreateView })
        refresh()
        scheduleSave()
        return s.gecko
    }

    override fun onContextMenu(session: KrunkSession, target: ContextTarget) {
        _events.trySend(TabEvent.ContextMenu(session.id, target))
    }

    override fun onExternalResponse(session: KrunkSession, response: WebResponse) {
        _events.trySend(TabEvent.Download(session.id, response))
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
        // The model follows the engine's committed location only (never a pending address, and
        // never an address on the internal origin a page navigated to).
        val modelUrl = model.tab(session.id)?.url
        if (modelUrl != null) {
            val next = modelUrlAfter(
                st,
                urlFacts(modelUrl).internalPage != null,
                urlFacts(st.url).internalPage != null,
            )
            if (next != null && next != modelUrl) model.setUrl(session.id, next)
        }
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

    override fun onCrashed(session: KrunkSession) {
        val id = session.id
        if (sessions[id] !== session) return
        if (id == _activeId.value) {
            // The tab on screen: say so and wait for the user (a page that crashes on load must
            // not be reloaded in a loop). reload() and navigate() replace the dead session.
            refresh()
        } else {
            // A background tab becomes an unloaded tab: it comes back with its history when the
            // user returns to it.
            unload(id)
        }
        scheduleSave()
    }

    suspend fun <T> io(block: () -> T): T = withContext(Dispatchers.IO) { block() }

    companion object {
        const val CAPTURE_TIMEOUT_MS = 700L
        const val KV_SESSION = "session"
        const val KV_CLOSED = "android.closed_tabs"
        const val KV_GECKO_STATES = "android.gecko_states"
        const val KV_CLEAN_EXIT = "android.clean_exit"
        const val KV_UNCLEAN_STARTS = "android.unclean_starts"

        const val THUMBNAIL_CAPACITY = 12
        const val URL_FACTS_CAPACITY = 128
        const val THUMBNAIL_SCALE = 3
        const val SAVE_DELAY_MS = 500L
        const val STATE_SAVE_DELAY_MS = 3000L
        const val AUTO_CAPTURE_DELAY_MS = 700L
        const val AUTO_CAPTURE_MIN_GAP_MS = 2000L
    }
}
