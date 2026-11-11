package org.krunk.browser.engine.blocker

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.json.JSONArray
import org.json.JSONObject
import org.krunk.browser.BuildConfig
import org.krunk.browser.KrunkApp
import org.krunk.browser.engine.EngineConfig
import org.krunk.browser.tabs.DebugProps
import org.krunk.ffi.AndroidSettings
import org.mozilla.geckoview.GeckoRuntime
import org.mozilla.geckoview.GeckoSession
import org.mozilla.geckoview.GeckoSessionSettings
import org.mozilla.geckoview.WebExtension
import org.mozilla.geckoview.WebExtensionController

/**
 * The ad and tracker blocker as the app sees it: the bundled uBlock Origin (unmodified) and
 * Krunk's own `krunk-guard` extension, which keeps the blocker's requests behind a gate and
 * passes Krunk's settings to it through the blocker's own messaging.
 *
 * The order of things at start is decided by [step] (BlockerStart.kt); this class carries the
 * actions out against GeckoView and reports back. Main thread only.
 */
class BlockerController(private val runtime: GeckoRuntime, private val host: Host) {
    /** What the controller needs from the app. */
    interface Host {
        fun settings(): AndroidSettings

        /** Krunk's allow list (the blocker gets a copy). */
        val allow: AllowList

        /** A user-triggered update brought newer lists at [at] (Unix ms). */
        fun listsUpdated(at: Long)
    }

    /** Page loads wait here until the blocker filters (or is off, unavailable, or too slow). */
    val pages = PageGate()

    private val _status = MutableStateFlow(BlockerStatus.STARTING)
    val status: StateFlow<BlockerStatus> = _status.asStateFlow()

    /** Pages were let go before the blocker was filtering (it took longer than [HOLD_MS]). */
    val startedUnprotected: Boolean get() = state.startedUnprotected

    private val _shields = MutableStateFlow<Map<UInt, Shield>>(emptyMap())

    /** The blocker's reading per tab (count, off). */
    val shields: StateFlow<Map<UInt, Shield>> = _shields.asStateFlow()

    private val _update = MutableStateFlow<UpdateState>(UpdateState.Idle)
    val update: StateFlow<UpdateState> = _update.asStateFlow()

    /** A top-level page load was stopped by the blocker (the address is on a filter list). */
    var onPageBlocked: ((url: String) -> Unit)? = null

    private val handler = Handler(Looper.getMainLooper())
    private val controller: WebExtensionController get() = runtime.webExtensionController
    private var state = StartState()
    private val shieldStore = ShieldStore()
    private val sessions = HashMap<UInt, GeckoSession>()

    private var guard: WebExtension? = null
    private var blocker: WebExtension? = null
    private var guardPort: WebExtension.Port? = null
    private var guardConnects = 0
    private var connectsAtAllow = 0

    /** The guard's script on the current port has said "ready". */
    private var readyOnPort = false
    private var bridge: GeckoSession? = null
    private var windowUntil = 0L
    private var nextCall = 1
    private val calls = HashMap<Int, (Boolean, Any?) -> Unit>()

    // Start-up timing (ms since start()), for the log line when the blocker is ready.
    private var startedAt = 0L
    private var guardReadyMs = -1L
    private var installedMs = -1L
    private var filteringMs = -1L

    // ----- start -----

    /** Brings the blocker up. Called once, right after the runtime is created. */
    fun start() {
        startedAt = SystemClock.elapsedRealtime()
        handler.postDelayed({ dispatch(StartEvent.HoldTimeout) }, HOLD_MS)
        handler.postDelayed({ if (!state.guardUp) dispatch(StartEvent.GuardFailed) }, GUARD_WATCHDOG_MS)
        dispatch(StartEvent.Start(host.settings().adblockEnabled))
    }

    private fun dispatch(event: StartEvent) {
        val (next, actions) = step(state, event)
        if (next != state || actions.isNotEmpty()) debug { "start: $event -> ${next.phase} $actions" }
        state = next
        _status.value = next.status
        for (action in actions) perform(action)
    }

    private fun perform(action: StartAction) {
        when (action) {
            StartAction.InstallGuard -> installGuard()

            StartAction.AllowGuardPrivate -> {
                val g = guard ?: return dispatch(StartEvent.GuardFailed)
                connectsAtAllow = guardConnects
                controller.setAllowedInPrivateBrowsing(g, true).accept(
                    { ext ->
                        if (ext != null) adoptGuard(ext)
                        dispatch(StartEvent.GuardPrivateAllowed)
                        // Granting it restarts the extension: has the new script connected yet?
                        settleGuard(restarted = guardConnects > connectsAtAllow)
                    },
                    { e -> failed("guard: private access", e, StartEvent.GuardFailed) },
                )
            }

            StartAction.ConfigureGuard -> {
                if (guardReadyMs < 0) guardReadyMs = sinceStart()
                configureGuard()
            }

            StartAction.InstallBlocker -> installBlocker()

            StartAction.AllowBlockerPrivate -> {
                val b = blocker ?: return dispatch(StartEvent.BlockerFailed)
                controller.setAllowedInPrivateBrowsing(b, true).accept(
                    { ext ->
                        if (ext != null) adoptBlocker(ext)
                        dispatch(StartEvent.BlockerPrivateAllowed)
                    },
                    { e -> failed("blocker: private access", e, StartEvent.BlockerFailed) },
                )
            }

            StartAction.EnableBlocker -> {
                val b = blocker ?: return dispatch(StartEvent.BlockerFailed)
                if (b.metaData.enabled) {
                    dispatch(StartEvent.BlockerEnabled)
                } else {
                    controller.enable(b, WebExtensionController.EnableSource.APP).accept(
                        { ext ->
                            if (ext != null) adoptBlocker(ext)
                            dispatch(StartEvent.BlockerEnabled)
                        },
                        { e -> failed("blocker: enable", e, StartEvent.BlockerFailed) },
                    )
                }
            }

            StartAction.DisableBlocker -> disableBlocker()

            StartAction.AwaitFilters -> {
                if (installedMs < 0) installedMs = sinceStart()
                openBridge()
                awaitFilters(SystemClock.elapsedRealtime() + FILTERS_TIMEOUT_MS)
            }

            StartAction.Configure -> {
                if (filteringMs < 0) filteringMs = sinceStart()
                configureBlocker()
            }

            is StartAction.ReleasePages -> {
                if (action.unprotected) {
                    Log.w(KrunkApp.TAG, "blocker not ready after ${HOLD_MS / 1000} s: pages load without it")
                }
                pages.open()
            }
        }
    }

    private fun failed(what: String, e: Throwable?, event: StartEvent) {
        Log.e(KrunkApp.TAG, "$what failed", e)
        dispatch(event)
    }

    private fun sinceStart(): Long = SystemClock.elapsedRealtime() - startedAt

    // ----- guard -----

    private fun installGuard() {
        // Debug builds can point at a missing folder to try a start without the guard.
        val uri = DebugProps.get(DebugProps.GUARD_URI) ?: GUARD_URI
        controller.ensureBuiltIn(uri, GUARD_ID).accept(
            { ext ->
                if (ext == null) return@accept dispatch(StartEvent.GuardFailed)
                adoptGuard(ext)
                if (ext.metaData.enabled) {
                    installedGuard(ext)
                } else {
                    // It is Krunk's own: nothing should ever disable it.
                    controller.enable(ext, WebExtensionController.EnableSource.APP).accept(
                        { enabled ->
                            val g = enabled ?: ext
                            adoptGuard(g)
                            installedGuard(g)
                        },
                        { e -> failed("guard: enable", e, StartEvent.GuardFailed) },
                    )
                }
            },
            { e -> failed("guard: install", e, StartEvent.GuardFailed) },
        )
    }

    private fun installedGuard(ext: WebExtension) {
        val private = ext.metaData.allowedInPrivateBrowsing
        dispatch(StartEvent.GuardInstalled(private))
        if (private) settleGuard(restarted = true)
    }

    /**
     * The guard is installed with private access: its "ready" is what is still missing. Its
     * script says so once when it starts, which can be before the app was waiting for it (while
     * the extension was being enabled or restarted). [restarted]: the script running now is the
     * final one (no restart is still to come).
     */
    private fun settleGuard(restarted: Boolean) {
        if (state.phase != StartPhase.GUARD_WAITING_READY) return
        if (restarted && guardPort != null && readyOnPort) {
            dispatch(StartEvent.GuardReady)
        } else {
            // It will say so by itself when it starts; ask if nothing came.
            handler.postDelayed({ hello() }, HELLO_DELAY_MS)
        }
    }

    private fun adoptGuard(ext: WebExtension) {
        guard = ext
        ext.setMessageDelegate(
            object : WebExtension.MessageDelegate {
                override fun onConnect(port: WebExtension.Port) {
                    guardPort = port
                    guardConnects++
                    readyOnPort = false
                    port.setDelegate(
                        object : WebExtension.PortDelegate {
                            override fun onPortMessage(message: Any, port: WebExtension.Port) {
                                if (port === guardPort) (message as? JSONObject)?.let(::fromGuard)
                            }

                            override fun onDisconnect(port: WebExtension.Port) {
                                if (port !== guardPort) return
                                guardPort = null
                                failCalls()
                                dispatch(StartEvent.GuardLost)
                            }
                        },
                    )
                }
            },
            NATIVE_APP,
        )
    }

    private fun toGuard(message: JSONObject): Boolean {
        val port = guardPort ?: return false
        return runCatching { port.postMessage(message) }.isSuccess
    }

    private fun hello() {
        if (state.phase == StartPhase.GUARD_WAITING_READY) toGuard(JSONObject().put("t", "hello"))
    }

    private fun configureGuard() {
        val s = host.settings()
        toGuard(JSONObject().put("t", "strip").put("on", s.stripTrackingParams))
        sendGate()
        val engine = if (s.filterAutoUpdate) "allow" else EngineConfig.ENGINE_GATE_DEFAULT
        toGuard(JSONObject().put("t", "engine").put("mode", engine))
    }

    private fun sendGate() {
        val open = FilterUpdatePolicy.gateOpen(
            host.settings().filterAutoUpdate,
            windowUntil,
            System.currentTimeMillis(),
        )
        toGuard(JSONObject().put("t", "gate").put("open", open))
    }

    private fun fromGuard(m: JSONObject) {
        when (m.optString("t")) {
            "ready" -> {
                readyOnPort = true
                dispatch(StartEvent.GuardReady)
            }

            "gate" -> debug { "gate: ${m.optJSONObject("gate") ?: m.optString("error")}" }

            "event" -> {
                val e = m.optJSONObject("e") ?: return
                val url = e.optString("url")
                when (e.optString("k")) {
                    "stopped" -> if (state.phase == StartPhase.READY) onPageBlocked?.invoke(url)

                    "blocker" -> debug {
                        "gate: blocker request ${if (e.optBoolean("cancelled")) "cancelled" else "passed"} $url"
                    }

                    "engine" -> debug {
                        "gate: engine request ${if (e.optBoolean("cancelled")) "cancelled" else "seen"} $url"
                    }
                }
            }

            "blocker-reply" -> {
                val done = calls.remove(m.optInt("id")) ?: return
                if (m.optBoolean("ok")) done(true, m.opt("result")) else done(false, m.optString("error"))
            }

            "rewrite" -> {
                val cleaned = UrlRewrite.cleaned(m.optString("url"), host.settings().stripTrackingParams)
                toGuard(JSONObject().put("t", "rewritten").put("id", m.optInt("id")).put("url", cleaned ?: ""))
            }
        }
    }

    // ----- blocker -----

    private fun installBlocker() {
        controller.ensureBuiltIn(BLOCKER_URI, BuildConfig.UBO_ID).accept(
            { ext ->
                if (ext == null) return@accept dispatch(StartEvent.BlockerFailed)
                adoptBlocker(ext)
                dispatch(StartEvent.BlockerInstalled(ext.metaData.allowedInPrivateBrowsing, ext.metaData.enabled))
            },
            { e -> failed("blocker: install", e, StartEvent.BlockerFailed) },
        )
    }

    /** The blocker's extension object changed (installed, enabled, private access): re-attach. */
    private fun adoptBlocker(ext: WebExtension) {
        blocker = ext
        ext.setActionDelegate(object : WebExtension.ActionDelegate {})
        for ((tab, session) in sessions) session.webExtensionController.setActionDelegate(ext, actionDelegate(tab))
    }

    private fun disableBlocker() {
        closeBridge()
        failCalls()
        if (shieldStore.clear()) _shields.value = emptyMap()
        val disable = { ext: WebExtension ->
            if (ext.metaData.enabled) {
                controller.disable(ext, WebExtensionController.EnableSource.APP).accept(
                    { disabled -> if (disabled != null) blocker = disabled },
                    { e -> Log.e(KrunkApp.TAG, "blocker: disable failed", e) },
                )
            }
        }
        val known = blocker
        if (known != null) {
            disable(known)
        } else {
            // Never touched in this run (blocking is off, or the guard failed): look in the profile.
            controller.list().accept(
                { all ->
                    all?.firstOrNull { it.id == BuildConfig.UBO_ID }?.let {
                        blocker = it
                        disable(it)
                    }
                },
                { e -> Log.e(KrunkApp.TAG, "blocker: list failed", e) },
            )
        }
    }

    private fun actionDelegate(tab: UInt) = object : WebExtension.ActionDelegate {
        override fun onBrowserAction(extension: WebExtension, session: GeckoSession?, action: WebExtension.Action) {
            if (session != null && shieldStore.title(tab, action.title)) _shields.value = shieldStore.snapshot()
        }
    }

    /** A tab's session exists: the blocker's per-tab reading arrives through it. */
    fun attach(tab: UInt, session: GeckoSession) {
        sessions[tab] = session
        blocker?.let { session.webExtensionController.setActionDelegate(it, actionDelegate(tab)) }
    }

    /** The tab's session is gone. */
    fun detach(tab: UInt) {
        sessions.remove(tab)
        if (shieldStore.forget(tab)) _shields.value = shieldStore.snapshot()
    }

    /** A new top-level document started in [tab]. */
    fun newDocument(tab: UInt) {
        if (shieldStore.newDocument(tab)) _shields.value = shieldStore.snapshot()
    }

    // ----- bridge (the blocker's own messaging) -----

    private fun openBridge() {
        closeBridge()
        val base = blocker?.metaData?.baseUrl ?: return
        // The blocker's address is random per install: always read from the extension.
        bridge = GeckoSession(GeckoSessionSettings.Builder().usePrivateMode(false).build()).also {
            it.open(runtime)
            it.loadUri(base + BRIDGE_PAGE)
        }
    }

    private fun closeBridge() {
        bridge?.let { runCatching { it.close() } }
        bridge = null
    }

    /** One message to the blocker; [done] gets (ok, reply or error text). Times out as failed. */
    private fun call(channel: String, message: JSONObject, done: (Boolean, Any?) -> Unit) {
        val id = nextCall++
        calls[id] = done
        handler.postDelayed({ calls.remove(id)?.invoke(false, "timeout") }, CALL_TIMEOUT_MS)
        if (!toGuard(JSONObject().put("t", "blocker").put("id", id).put("channel", channel).put("msg", message))) {
            calls.remove(id)?.invoke(false, "no guard")
        }
    }

    private fun failCalls() {
        val pending = ArrayList(calls.values)
        calls.clear()
        for (done in pending) done(false, "guard lost")
    }

    private fun dashboard(what: String, done: (Boolean, Any?) -> Unit) =
        call(DASHBOARD, JSONObject().put("what", what), done)

    private fun awaitFilters(deadline: Long) {
        if (state.phase != StartPhase.WAITING_FILTERS) return
        dashboard("readyToFilter") { ok, reply ->
            when {
                state.phase != StartPhase.WAITING_FILTERS -> Unit
                ok && reply == true -> dispatch(StartEvent.FiltersReady)
                SystemClock.elapsedRealtime() > deadline -> failed("blocker: not ready", null, StartEvent.BlockerFailed)
                else -> handler.postDelayed({ awaitFilters(deadline) }, READY_POLL_MS)
            }
        }
    }

    /** Krunk's settings and allow list go into the blocker, each read back afterwards. */
    private fun configureBlocker() {
        val automatic = host.settings().filterAutoUpdate
        setAutoUpdate(automatic) { settingsOk ->
            pushAllowList { allowOk ->
                if (!settingsOk || !allowOk) {
                    // Blocking works all the same and the gate does not depend on this.
                    Log.e(
                        KrunkApp.TAG,
                        "blocker: settings not confirmed (auto-update $settingsOk, allow list $allowOk)",
                    )
                }
                Log.i(
                    KrunkApp.TAG,
                    "blocker ready after ${sinceStart()} ms (guard $guardReadyMs, installed $installedMs, " +
                        "filtering $filteringMs)",
                )
                dispatch(StartEvent.Configured)
            }
        }
    }

    private fun setAutoUpdate(on: Boolean, done: (Boolean) -> Unit) {
        val set = JSONObject().put("what", "userSettings").put("name", "autoUpdate").put("value", on)
        call(DASHBOARD, set) { _, _ ->
            dashboard("userSettings") { ok, reply ->
                done(ok && (reply as? JSONObject)?.optBoolean("autoUpdate", !on) == on)
            }
        }
    }

    private fun pushAllowList(done: (Boolean) -> Unit) {
        val hosts = host.allow.hosts()
        dashboard("getWhitelist") { ok, reply ->
            val defaults = (reply as? JSONObject)?.optJSONArray("whitelistDefault").strings()
            if (!ok || defaults.isEmpty()) return@dashboard done(false)
            val text = AllowList.trustedSites(defaults, hosts)
            call(DASHBOARD, JSONObject().put("what", "setWhitelist").put("whitelist", text)) { _, _ ->
                dashboard("getWhitelist") { ok2, reply2 ->
                    val now = (reply2 as? JSONObject)?.optJSONArray("whitelist").strings()
                    done(ok2 && AllowList.sameSites(now, defaults, hosts))
                }
            }
        }
    }

    private fun JSONArray?.strings(): List<String> {
        this ?: return emptyList()
        return (0 until length()).map { optString(it) }
    }

    // ----- what the app asks for -----

    /** Krunk's allow list changed: the blocker gets the new copy. [done] hears whether it took it. */
    fun allowListChanged(done: (Boolean) -> Unit = {}) {
        if (state.phase != StartPhase.READY) return done(false)
        pushAllowList(done)
    }

    /** Lets the blocker's "page blocked" rule pass for [host] for two minutes, then calls [then]. */
    fun openOnce(host: String, then: () -> Unit) {
        if (state.phase != StartPhase.READY) return then()
        val message = JSONObject().put("what", "temporarilyWhitelistDocument").put("hostname", host)
        call(DOCUMENT_BLOCKED, message) { _, _ -> then() }
    }

    /** Settings changed: the blocking switch, the rewriter, the gate. */
    fun settingsChanged(settings: AndroidSettings) {
        dispatch(StartEvent.BlockingWanted(settings.adblockEnabled))
        if (state.guardUp) configureGuard()
        if (state.phase == StartPhase.READY) setAutoUpdate(settings.filterAutoUpdate) { }
    }

    /** "Update filter lists": the only way an update starts unless the user turned on automatic updates. */
    fun updateNow(cause: FetchCause = FetchCause.USER) {
        val now = System.currentTimeMillis()
        if (!FilterUpdatePolicy.mayStart(cause) || _update.value is UpdateState.Running) return
        if (state.phase != StartPhase.READY) {
            _update.value = UpdateState.Failed(now)
            return
        }
        windowUntil = FilterUpdatePolicy.windowFor(cause, now) ?: return
        _update.value = UpdateState.Running(now)
        sendGate()
        lists { before ->
            dashboard("updateNow") { _, _ -> pollUpdate(before.orEmpty(), sawUpdating = false) }
        }
    }

    private fun pollUpdate(before: List<FilterList>, sawUpdating: Boolean) {
        handler.postDelayed(
            {
                val started = (_update.value as? UpdateState.Running)?.since ?: return@postDelayed
                dashboard("getLists") { ok, reply ->
                    val now = System.currentTimeMillis()
                    val o = reply as? JSONObject
                    val updating = ok && o?.optBoolean("isUpdating") == true
                    val settled = ok && !updating && (sawUpdating || now - started > UPDATE_SETTLE_MS)
                    if (settled || now >= windowUntil || state.phase != StartPhase.READY) {
                        finishUpdate(before, parseLists(o), now)
                    } else {
                        pollUpdate(before, sawUpdating || updating)
                    }
                }
            },
            UPDATE_POLL_MS,
        )
    }

    private fun finishUpdate(before: List<FilterList>, after: List<FilterList>, now: Long) {
        windowUntil = 0
        sendGate()
        val updated = FilterLists.updated(before, after)
        if (updated > 0) {
            host.listsUpdated(now)
            _update.value = UpdateState.Done(now, updated)
        } else {
            _update.value = UpdateState.Failed(now)
        }
        debug { "update finished: $updated lists newer" }
    }

    /** The selected filter lists as the blocker reports them (null when it does not answer). */
    fun lists(done: (List<FilterList>?) -> Unit) {
        if (state.phase != StartPhase.READY) return done(null)
        dashboard("getLists") { ok, reply -> done(if (ok) parseLists(reply as? JSONObject) else null) }
    }

    private fun parseLists(reply: JSONObject?): List<FilterList> {
        val arr = reply?.optJSONArray("lists") ?: return emptyList()
        return (0 until arr.length()).mapNotNull { i ->
            val o = arr.optJSONObject(i) ?: return@mapNotNull null
            FilterList(o.optString("key"), o.optLong("written"), o.optInt("filters"), o.optBoolean("inPackage"))
        }
    }

    // ----- debug -----

    /** One message to the blocker, for the debug hook only. */
    fun debugCall(channel: String, message: JSONObject, done: (Boolean, Any?) -> Unit) {
        if (BuildConfig.DEBUG) call(channel, message, done)
    }

    /** Disables the guard, for the debug hook only (the "guard lost" test). */
    fun debugDisableGuard() {
        if (!BuildConfig.DEBUG) return
        guard?.let { controller.disable(it, WebExtensionController.EnableSource.APP) }
    }

    fun debugState(): String = "phase=${state.phase} status=${state.status} guardUp=${state.guardUp} " +
        "pagesReleased=${state.pagesReleased} unprotected=${state.startedUnprotected} held=${pages.held} " +
        "guard=${guard?.metaData?.enabled} blocker=${blocker?.metaData?.enabled} ${blocker?.metaData?.version} " +
        "shields=${shieldStore.snapshot()} update=${_update.value}"

    private inline fun debug(message: () -> String) {
        if (BuildConfig.DEBUG) Log.i(GUARD_TAG, message())
    }

    companion object {
        const val GUARD_TAG = "KrunkGuard"
        const val GUARD_ID = "guard@krunk.org"
        const val GUARD_URI = "resource://android/assets/extensions/krunk-guard/"
        const val BLOCKER_URI = "resource://android/assets/extensions/ublock0/"
        const val NATIVE_APP = "krunk"

        /** The blocker's lightest page: its messaging client and nothing else. */
        private const val BRIDGE_PAGE = "no-dashboard.html"
        private const val DASHBOARD = "dashboard"
        private const val DOCUMENT_BLOCKED = "documentBlocked"
        private const val READY_POLL_MS = 250L
        private const val FILTERS_TIMEOUT_MS = 60_000L
        private const val CALL_TIMEOUT_MS = 5_000L
        private const val HELLO_DELAY_MS = 3_000L
        private const val GUARD_WATCHDOG_MS = 20_000L
        private const val UPDATE_POLL_MS = 2_000L
        private const val UPDATE_SETTLE_MS = 8_000L
    }
}
