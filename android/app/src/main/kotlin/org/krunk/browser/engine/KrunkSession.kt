package org.krunk.browser.engine

import android.util.Log
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import org.krunk.browser.KrunkApp
import org.mozilla.geckoview.AllowOrDeny
import org.mozilla.geckoview.GeckoResult
import org.mozilla.geckoview.GeckoSession
import org.mozilla.geckoview.GeckoSessionSettings
import org.mozilla.geckoview.WebRequestError

/** A long-pressed element (link, image, ...). */
data class ContextTarget(
    val linkUri: String?,
    val srcUri: String?,
    val title: String?,
    val altText: String?,
    /** The link's visible text, when the element is (inside) a link. */
    val linkText: String?,
    val type: Int,
) {
    val isLink get() = !linkUri.isNullOrBlank()
    val isImage get() = type == GeckoSession.ContentDelegate.ContextElement.TYPE_IMAGE
    val isMedia get() = type == GeckoSession.ContentDelegate.ContextElement.TYPE_VIDEO ||
        type == GeckoSession.ContentDelegate.ContextElement.TYPE_AUDIO

    /** Text worth copying for a link: its text, else its title, else the image's alt text. */
    val copyableText: String?
        get() = listOf(linkText, title, altText).firstOrNull { !it.isNullOrBlank() }?.trim()
}

/** Events the tab manager reacts to (history, new tabs, menus, downloads). */
interface SessionListener {
    /** A top-level navigation finished loading (not errors): record history. */
    fun onVisited(session: KrunkSession, url: String)

    fun onTitleChanged(session: KrunkSession, title: String)

    /** The page wants a new window/tab (`target=_blank`, `window.open`); return a tab's session. */
    fun onNewSessionRequested(session: KrunkSession, uri: String): GeckoSession?

    fun onContextMenu(session: KrunkSession, target: ContextTarget)

    fun onExternalResponse(session: KrunkSession, response: org.mozilla.geckoview.WebResponse)

    fun onIcons(session: KrunkSession, pageUrl: String, icons: List<IconCandidate>)

    fun onStateChanged(session: KrunkSession)

    /** The page's window.close() for a tab the page opened. */
    fun onCloseRequested(session: KrunkSession)

    /**
     * The page's process crashed or was killed; the engine has already closed [session]. The
     * tab needs a new session before it can load anything again.
     */
    fun onCrashed(session: KrunkSession)
}

/**
 * One tab's [GeckoSession] with every delegate wired to a [PageState] flow. Created by the tab
 * manager for live tabs; closed when the tab is closed, discarded or the process trims memory.
 */
class KrunkSession(
    val id: UInt,
    val private: Boolean,
    private val engine: GeckoEngine,
    private val listener: SessionListener,
    private val errorPage: ErrorPage,
    private val recordHistory: Boolean,
    /**
     * The session of a window a page opened (`target=_blank`, `window.open`). The engine opens
     * such a session itself and refuses one that is already open, so it is handed over unopened;
     * and its address bar shows nothing until the engine commits a location (see [displayedUrl]).
     */
    val openedByPage: Boolean = false,
) {
    val gecko: GeckoSession = engine.createSession(private)
    private val _state = MutableStateFlow(PageState())
    val state: StateFlow<PageState> = _state.asStateFlow()
    val current: PageState get() = _state.value

    private fun set(transform: (PageState) -> PageState) {
        _state.update(transform)
        listener.onStateChanged(this)
    }

    // ----- commands -----

    /**
     * Loads [url]. Only the user's own input ([LoadSource.USER]) is loaded with the app's
     * privileges; an address from another app is loaded as an external link, and one a page
     * supplied on behalf of that page ([referrer], when its tab is still open). Both are limited
     * to web addresses by [LoadPolicy] before they get here; this is the second check.
     */
    fun load(url: String, source: LoadSource = LoadSource.USER, referrer: GeckoSession? = null) {
        if (!LoadPolicy.allowed(url, source)) {
            Log.w(KrunkApp.TAG, "tab $id: refused an address that is not a web address (source $source)")
            return
        }
        set { it.requested(url) }
        val loader = GeckoSession.Loader().uri(url)
        when (source) {
            LoadSource.USER -> Unit

            LoadSource.EXTERNAL_APP -> loader.flags(GeckoSession.LOAD_FLAGS_EXTERNAL)

            LoadSource.PAGE -> if (referrer !=
                null
            ) {
                loader.referrer(referrer)
            } else {
                loader.flags(GeckoSession.LOAD_FLAGS_EXTERNAL)
            }
        }
        gecko.load(loader)
    }

    /** Parks the engine on a blank page while a native screen (the new-tab page) is shown. */
    fun park() {
        set { it.parked() }
        gecko.loadUri(ABOUT_BLANK)
    }

    /** Reloads the page. A crashed tab is not reloaded here: the tab manager gives it a new session. */
    fun reload() = gecko.reload()

    fun stop() = gecko.stop()

    fun goBack() = gecko.goBack()

    fun goForward() = gecko.goForward()

    fun setActive(active: Boolean) = gecko.setActive(active)

    fun setDesktopMode(desktop: Boolean) {
        gecko.settings.userAgentMode =
            if (desktop) GeckoSessionSettings.USER_AGENT_MODE_DESKTOP else GeckoSessionSettings.USER_AGENT_MODE_MOBILE
        gecko.settings.viewportMode =
            if (desktop) GeckoSessionSettings.VIEWPORT_MODE_DESKTOP else GeckoSessionSettings.VIEWPORT_MODE_MOBILE
        set { it.copy(desktopMode = desktop) }
        gecko.reload()
    }

    fun exitFullScreen() = gecko.exitFullScreen()

    fun restoreState(serialized: String) {
        runCatching { GeckoSession.SessionState.fromString(serialized) }
            .onSuccess { st -> if (st != null) gecko.restoreState(st) }
            .onFailure { Log.w(KrunkApp.TAG, "could not restore session state", it) }
    }

    /**
     * Find in page: a new [text] starts a search (every match highlighted), `null` moves to the
     * next or previous match of the last one. Results land in [PageState.findCurrent]/[PageState.findTotal].
     */
    fun find(text: String?, forward: Boolean) {
        var flags = 0
        if (!forward) flags = flags or GeckoSession.FINDER_FIND_BACKWARDS
        gecko.finder.displayFlags = GeckoSession.FINDER_DISPLAY_HIGHLIGHT_ALL
        gecko.finder.find(text, flags).accept { r ->
            set { it.copy(findCurrent = r?.current ?: 0, findTotal = r?.total ?: 0) }
        }
    }

    fun clearFind() {
        gecko.finder.clear()
        gecko.finder.displayFlags = 0
        set { it.copy(findCurrent = 0, findTotal = 0) }
    }

    fun close() {
        gecko.close()
    }

    // ----- delegates -----

    private val navigation = object : GeckoSession.NavigationDelegate {
        override fun onLocationChange(
            session: GeckoSession,
            url: String?,
            perms: MutableList<GeckoSession.PermissionDelegate.ContentPermission>,
            hasUserGesture: Boolean,
        ) {
            set { it.locationChanged(url ?: "") }
        }

        override fun onCanGoBack(session: GeckoSession, canGoBack: Boolean) {
            set { it.copy(canGoBack = canGoBack) }
        }

        override fun onCanGoForward(session: GeckoSession, canGoForward: Boolean) {
            set { it.copy(canGoForward = canGoForward) }
        }

        override fun onLoadRequest(
            session: GeckoSession,
            request: GeckoSession.NavigationDelegate.LoadRequest,
        ): GeckoResult<AllowOrDeny>? = GeckoResult.fromValue(AllowOrDeny.ALLOW)

        override fun onNewSession(session: GeckoSession, uri: String): GeckoResult<GeckoSession>? {
            val s = listener.onNewSessionRequested(this@KrunkSession, uri) ?: return null
            return GeckoResult.fromValue(s)
        }

        override fun onLoadError(session: GeckoSession, uri: String?, error: WebRequestError): GeckoResult<String>? {
            val u = uri ?: ""
            val page = errorPage.dataUri(u, error.category, error.code)
            set { it.copy(loadError = LoadError(u, error.category, error.code, page), progress = 0f, loading = false) }
            return GeckoResult.fromValue(page)
        }
    }

    private val progress = object : GeckoSession.ProgressDelegate {
        override fun onPageStart(session: GeckoSession, url: String) {
            set { it.copy(progress = 0.05f, loading = true, loadError = null, crashed = false) }
        }

        override fun onProgressChange(session: GeckoSession, progress: Int) {
            set { it.copy(progress = progress / 100f, loading = progress < 100) }
        }

        override fun onPageStop(session: GeckoSession, success: Boolean) {
            set { it.pageStopped() }
        }

        override fun onSecurityChange(
            session: GeckoSession,
            securityInfo: GeckoSession.ProgressDelegate.SecurityInformation,
        ) {
            set { it.copy(secure = securityInfo.isSecure) }
        }

        override fun onSessionStateChange(session: GeckoSession, sessionState: GeckoSession.SessionState) {
            set { it.copy(sessionState = sessionState.toString()) }
        }
    }

    private val content = object : GeckoSession.ContentDelegate {
        override fun onTitleChange(session: GeckoSession, title: String?) {
            val t = title ?: ""
            set { it.copy(title = t) }
            listener.onTitleChanged(this@KrunkSession, t)
        }

        override fun onFullScreen(session: GeckoSession, fullScreen: Boolean) {
            set { it.copy(fullScreen = fullScreen) }
        }

        override fun onContextMenu(
            session: GeckoSession,
            screenX: Int,
            screenY: Int,
            element: GeckoSession.ContentDelegate.ContextElement,
        ) {
            listener.onContextMenu(
                this@KrunkSession,
                ContextTarget(
                    linkUri = element.linkUri,
                    srcUri = element.srcUri,
                    title = element.title,
                    altText = element.altText,
                    linkText = element.linkText,
                    type = element.type,
                ),
            )
        }

        override fun onExternalResponse(session: GeckoSession, response: org.mozilla.geckoview.WebResponse) {
            listener.onExternalResponse(this@KrunkSession, response)
        }

        override fun onCloseRequest(session: GeckoSession) {
            listener.onCloseRequested(this@KrunkSession)
        }

        override fun onCrash(session: GeckoSession) {
            Log.w(KrunkApp.TAG, "content process crashed for tab $id")
            set { it.crashed() }
            listener.onCrashed(this@KrunkSession)
        }

        override fun onKill(session: GeckoSession) {
            Log.w(KrunkApp.TAG, "content process killed for tab $id")
            set { it.crashed() }
            listener.onCrashed(this@KrunkSession)
        }
    }

    private val history = object : GeckoSession.HistoryDelegate {
        override fun onVisited(
            session: GeckoSession,
            url: String,
            lastVisitedURL: String?,
            flags: Int,
        ): GeckoResult<Boolean>? {
            val topLevel = flags and GeckoSession.HistoryDelegate.VISIT_TOP_LEVEL != 0
            val error = flags and GeckoSession.HistoryDelegate.VISIT_UNRECOVERABLE_ERROR != 0
            if (recordHistory && topLevel && !error) listener.onVisited(this@KrunkSession, url)
            return GeckoResult.fromValue(recordHistory && topLevel && !error)
        }
    }

    init {
        gecko.navigationDelegate = navigation
        gecko.progressDelegate = progress
        gecko.contentDelegate = content
        gecko.historyDelegate = history
        if (!openedByPage) gecko.open(engine.runtime)
        engine.icons.listen(gecko) { url, icons -> listener.onIcons(this, url, icons) }
    }
}
