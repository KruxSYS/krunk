package org.krunk.browser.engine

import android.util.Log
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.krunk.browser.KrunkApp
import org.krunk.ffi.internalPage
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

    /** The page asks something (`alert`, `confirm`, `prompt`, a `<select>` list, HTTP sign-in). */
    fun onPrompt(session: KrunkSession, prompt: PagePrompt)

    /** [prompt] has its answer (from the UI, or the engine withdrew it). */
    fun onPromptClosed(session: KrunkSession, prompt: PagePrompt)
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

    /** The user-agent setting changed: it applies to this tab's next load. */
    fun setUserAgent(userAgent: String) {
        gecko.settings.userAgentOverride = userAgent.trim().ifEmpty { null }
    }
    private val _state = MutableStateFlow(PageState())
    val state: StateFlow<PageState> = _state.asStateFlow()
    val current: PageState get() = _state.value

    /** Applies a change; listeners only hear about it when something really changed. */
    private fun set(transform: (PageState) -> PageState) {
        val before = _state.value
        val after = transform(before)
        if (after == before) return
        _state.value = after
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

    /**
     * Restores a saved state (the engine then loads its current entry). Returns the address of
     * that entry ("" when the state has none), or null when the state is unusable and nothing
     * was restored. The caller compares the address with the tab's own: the engine reports its
     * state with a delay, so a state saved right after a navigation can be one page behind.
     */
    fun restoreState(serialized: String): String? {
        if (serialized.isEmpty()) return null
        val state = runCatching { GeckoSession.SessionState.fromString(serialized) }
            .onFailure { Log.w(KrunkApp.TAG, "could not restore session state", it) }
            .getOrNull() ?: return null
        val current = runCatching { state[state.currentIndex].uri }.getOrDefault("")
        gecko.restoreState(state)
        return current
    }

    /** Asks the engine to report its state now (it normally does so with a delay). */
    fun flushState() = gecko.flushSessionState()

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
        ): GeckoResult<AllowOrDeny>? {
            // Krunk's internal origin exists only as native screens, which the app opens itself
            // (its own loads do not pass here). A page that navigates there is refused, so it
            // cannot make the bar read krunk://... or switch the tab to the new-tab page.
            val internal = internalPage(request.uri) != null
            return GeckoResult.fromValue(if (internal) AllowOrDeny.DENY else AllowOrDeny.ALLOW)
        }

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
            promptThrottle.newDocument()
            set { it.pageStarted(url) }
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
            set { it.securityChanged(securityInfo.isSecure) }
        }

        override fun onSessionStateChange(session: GeckoSession, sessionState: GeckoSession.SessionState) {
            set { it.copy(sessionState = sessionState.toString()) }
        }
    }

    private val content = object : GeckoSession.ContentDelegate {
        override fun onTitleChange(session: GeckoSession, title: String?) {
            val t = title ?: ""
            // The tab model first, so the one refresh the state change triggers shows it.
            listener.onTitleChanged(this@KrunkSession, t)
            set { it.copy(title = t) }
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

    // ----- page prompts -----

    private val promptThrottle = PromptThrottle()

    /** The host of the page that is asking (the dialog's title). */
    private fun promptSite(): String = originOf(current.url)?.substringAfter("://") ?: ""

    /**
     * Shows a prompt and completes the engine's request exactly once: with the UI's answer, or
     * with a dismissal when the engine withdraws the prompt first (the page navigated away).
     */
    private fun <P : GeckoSession.PromptDelegate.BasePrompt> ask(
        prompt: P,
        make: (answer: (GeckoSession.PromptDelegate.PromptResponse) -> Unit) -> PagePrompt,
    ): GeckoResult<GeckoSession.PromptDelegate.PromptResponse> {
        val result = GeckoResult<GeckoSession.PromptDelegate.PromptResponse>()
        var answered = false
        var shown: PagePrompt? = null
        val answer: (GeckoSession.PromptDelegate.PromptResponse) -> Unit = { response ->
            if (!answered) {
                answered = true
                result.complete(response)
                shown?.let { listener.onPromptClosed(this, it) }
            }
        }
        val page = make(answer)
        shown = page
        prompt.setDelegate(object : GeckoSession.PromptDelegate.PromptInstanceDelegate {
            override fun onPromptDismiss(prompt: GeckoSession.PromptDelegate.BasePrompt) {
                if (!answered) answer(prompt.dismiss())
            }
        })
        listener.onPrompt(this, page)
        return result
    }

    private val prompts = object : GeckoSession.PromptDelegate {
        override fun onAlertPrompt(
            session: GeckoSession,
            prompt: GeckoSession.PromptDelegate.AlertPrompt,
        ): GeckoResult<GeckoSession.PromptDelegate.PromptResponse> {
            val offerBlock = promptThrottle.next() ?: return GeckoResult.fromValue(prompt.dismiss())
            return ask(prompt) { answer ->
                PagePrompt.Alert(id, promptSite(), prompt.message ?: "", offerBlock) { a ->
                    if (a == PagePrompt.Answer.BLOCK) promptThrottle.block()
                    answer(prompt.dismiss())
                }
            }
        }

        override fun onButtonPrompt(
            session: GeckoSession,
            prompt: GeckoSession.PromptDelegate.ButtonPrompt,
        ): GeckoResult<GeckoSession.PromptDelegate.PromptResponse> {
            val offerBlock = promptThrottle.next() ?: return GeckoResult.fromValue(prompt.dismiss())
            return ask(prompt) { answer ->
                PagePrompt.Confirm(id, promptSite(), prompt.message ?: "", offerBlock) { a ->
                    when (a) {
                        PagePrompt.Answer.OK ->
                            answer(prompt.confirm(GeckoSession.PromptDelegate.ButtonPrompt.Type.POSITIVE))

                        PagePrompt.Answer.CANCEL ->
                            answer(prompt.confirm(GeckoSession.PromptDelegate.ButtonPrompt.Type.NEGATIVE))

                        PagePrompt.Answer.DISMISS -> answer(prompt.dismiss())

                        PagePrompt.Answer.BLOCK -> {
                            promptThrottle.block()
                            answer(prompt.dismiss())
                        }
                    }
                }
            }
        }

        override fun onTextPrompt(
            session: GeckoSession,
            prompt: GeckoSession.PromptDelegate.TextPrompt,
        ): GeckoResult<GeckoSession.PromptDelegate.PromptResponse> {
            val offerBlock = promptThrottle.next() ?: return GeckoResult.fromValue(prompt.dismiss())
            return ask(prompt) { answer ->
                PagePrompt.Text(
                    id,
                    promptSite(),
                    prompt.message ?: "",
                    prompt.defaultValue ?: "",
                    offerBlock,
                ) { a, text ->
                    if (a == PagePrompt.Answer.BLOCK) promptThrottle.block()
                    answer(if (a == PagePrompt.Answer.OK) prompt.confirm(text) else prompt.dismiss())
                }
            }
        }

        override fun onAuthPrompt(
            session: GeckoSession,
            prompt: GeckoSession.PromptDelegate.AuthPrompt,
        ): GeckoResult<GeckoSession.PromptDelegate.PromptResponse> {
            val options = prompt.authOptions
            val onlyPassword =
                options.flags and GeckoSession.PromptDelegate.AuthPrompt.AuthOptions.Flags.ONLY_PASSWORD != 0
            // The server that asks, which need not be the page's own.
            val server = options.uri?.let { originOf(it)?.substringAfter("://") } ?: promptSite()
            return ask(prompt) { answer ->
                PagePrompt.SignIn(
                    tabId = id,
                    site = server,
                    message = prompt.message ?: "",
                    username = options.username ?: "",
                    onlyPassword = onlyPassword,
                    insecure = options.uri?.startsWith("http://", ignoreCase = true) == true,
                    onAnswer = { user, password ->
                        answer(if (onlyPassword) prompt.confirm(password) else prompt.confirm(user, password))
                    },
                    onDismiss = { answer(prompt.dismiss()) },
                )
            }
        }

        override fun onChoicePrompt(
            session: GeckoSession,
            prompt: GeckoSession.PromptDelegate.ChoicePrompt,
        ): GeckoResult<GeckoSession.PromptDelegate.PromptResponse> {
            val multiple = prompt.type == GeckoSession.PromptDelegate.ChoicePrompt.Type.MULTIPLE
            val options = ArrayList<PagePrompt.Option>()
            fun add(c: GeckoSession.PromptDelegate.ChoicePrompt.Choice, indented: Boolean) {
                val group = c.items
                when {
                    c.separator -> options += PagePrompt.Option(c.id, "", separator = true)

                    group != null -> {
                        options += PagePrompt.Option(c.id, c.label, header = true)
                        group.forEach { add(it, indented = true) }
                    }

                    else -> options += PagePrompt.Option(c.id, c.label, c.selected, c.disabled, indented = indented)
                }
            }
            prompt.choices.forEach { add(it, indented = false) }
            return ask(prompt) { answer ->
                PagePrompt.Choice(
                    tabId = id,
                    site = promptSite(),
                    message = prompt.message ?: prompt.title ?: "",
                    multiple = multiple,
                    options = options,
                    onAnswer = { ids ->
                        answer(
                            when {
                                multiple -> prompt.confirm(ids.toTypedArray())
                                ids.isEmpty() -> prompt.dismiss()
                                else -> prompt.confirm(ids.first())
                            },
                        )
                    },
                    onDismiss = { answer(prompt.dismiss()) },
                )
            }
        }
    }

    private val history = object : GeckoSession.HistoryDelegate {
        override fun onVisited(
            session: GeckoSession,
            url: String,
            lastVisitedURL: String?,
            flags: Int,
        ): GeckoResult<Boolean>? {
            val record = recordHistory && isRecordableVisit(flags)
            if (record) listener.onVisited(this@KrunkSession, url)
            return GeckoResult.fromValue(record)
        }
    }

    init {
        gecko.navigationDelegate = navigation
        gecko.progressDelegate = progress
        gecko.contentDelegate = content
        gecko.historyDelegate = history
        gecko.promptDelegate = prompts
        if (!openedByPage) gecko.open(engine.runtime)
        engine.icons.listen(gecko) { url, icons -> listener.onIcons(this, url, icons) }
    }
}
