package org.krunk.browser.engine

import android.os.SystemClock
import android.util.Log
import android.webkit.MimeTypeMap
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import org.krunk.browser.KrunkApp
import org.krunk.browser.engine.logins.HttpAuthOffer
import org.krunk.browser.engine.logins.LoginPage
import org.krunk.browser.engine.logins.LoginPolicy
import org.krunk.browser.engine.logins.LoginVault
import org.krunk.browser.engine.permissions.SitePermissions
import org.krunk.browser.vault.VaultFailure
import org.krunk.ffi.internalPage
import org.krunk.ffi.webOrigin
import org.mozilla.geckoview.AllowOrDeny
import org.mozilla.geckoview.Autocomplete
import org.mozilla.geckoview.GeckoResult
import org.mozilla.geckoview.GeckoSession
import org.mozilla.geckoview.GeckoSession.PromptDelegate.AuthPrompt.AuthOptions.Flags as AuthFlags
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

    /**
     * The page wants a `mailto:` or `tel:` link opened: hand it to Android, after asking the
     * user when [ask] (the page did it by itself, without a tap).
     */
    fun onExternalLink(session: KrunkSession, link: ExternalLink, ask: Boolean)
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
    /**
     * The tab asks for the desktop site ("Request desktop site"). A new session of such a tab
     * starts with the desktop user agent and viewport, so its first load is already a desktop one.
     */
    desktopMode: Boolean = false,
) {
    val gecko: GeckoSession = engine.createSession(private, desktopMode)

    /** The user-agent setting changed: it applies to this tab's next load. */
    fun setUserAgent(userAgent: String) {
        gecko.settings.userAgentOverride = userAgent.trim().ifEmpty { null }
    }
    private val _state = MutableStateFlow(PageState(desktopMode = desktopMode))
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
        engine.blocker.pages.run { if (!closed) gecko.load(loader) }
    }

    /** Parks the engine on a blank page while a native screen (the new-tab page) is shown. */
    fun park() {
        set { it.parked() }
        gecko.loadUri(ABOUT_BLANK)
    }

    /** Reloads the page. A crashed tab is not reloaded here: the tab manager gives it a new session. */
    fun reload() = engine.blocker.pages.run { if (!closed) gecko.reload() }

    /** The blocker stopped the load to [url] (see [PageState.blocked]). */
    fun blocked(url: String) = set { it.blockedByBlocker(url) }

    /** Leaves the "blocked" panel for the page under it. */
    fun dismissBlocked() = set { it.blockedDismissed() }

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

    /**
     * Leaves full screen now. Krunk's own state changes at once, so the bars and the toolbar
     * come back even when the page is slow (or too busy) to confirm; the page is told as well, so
     * it does not go on believing it is full screen.
     */
    fun leaveFullScreen() {
        set { if (it.fullScreen) it.copy(fullScreen = false) else it }
        if (!closed) gecko.exitFullScreen()
    }

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
        engine.blocker.pages.run { if (!closed) gecko.restoreState(state) }
        // Until the engine reports a newer one this is the tab's state: a save or a crash in
        // the first seconds after a restore must not find "no state" and lose the history.
        _state.value = _state.value.copy(sessionState = serialized)
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
        // Only the answer to the latest search counts: one that was overtaken comes back
        // without its match count and must not replace the newer result.
        val search = ++findSearch
        gecko.finder.find(text, flags).accept { r ->
            if (search == findSearch) set { it.copy(findCurrent = r?.current ?: 0, findTotal = r?.total ?: 0) }
        }
    }

    private var findSearch = 0

    fun clearFind() {
        findSearch++
        gecko.finder.clear()
        gecko.finder.displayFlags = 0
        set { it.copy(findCurrent = 0, findTotal = 0) }
    }

    private var closed = false

    /** When the user last lifted a finger from this tab's page (uptime ms; 0 = never). */
    private var lastTouchAt = 0L

    /** The page view reports a finished touch (see [ExternalLinks.TAP_WINDOW_MS]). */
    fun touched() {
        lastTouchAt = SystemClock.uptimeMillis()
    }

    fun close() {
        closed = true
        loginsClosed()
        engine.permissions?.tabClosed(id)
        engine.blocker.detach(id)
        engine.uploads.clearTab(id)
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
            loginsLocationChanged()
            // Site permissions end with the document, not with every load start (DocumentChange).
            if (document.locationChanged(url ?: "")) engine.permissions?.newDocument(id)
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
            // mailto: and tel: are not pages: Android's mail app or dialer gets them (asked
            // first unless the user tapped the link). The engine never loads them.
            val sinceTouch = lastTouchAt.takeIf { it > 0 }?.let { SystemClock.uptimeMillis() - it }
            val handOff = ExternalLinks.classify(request.uri, request.hasUserGesture, request.isRedirect, sinceTouch)
            when (handOff) {
                HandOff.NotExternal -> Unit

                HandOff.Refuse -> return GeckoResult.fromValue(AllowOrDeny.DENY)

                is HandOff.Open -> {
                    listener.onExternalLink(this@KrunkSession, handOff.link, ask = false)
                    return GeckoResult.fromValue(AllowOrDeny.DENY)
                }

                is HandOff.Ask -> {
                    listener.onExternalLink(this@KrunkSession, handOff.link, ask = true)
                    return GeckoResult.fromValue(AllowOrDeny.DENY)
                }
            }
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
            val cert = CertErrors.fromLoadError(u, error.code, error.certificate, private, System.currentTimeMillis())
            set {
                it.copy(
                    loadError = LoadError(u, error.category, error.code, page, cert),
                    progress = 0f,
                    loading = false,
                )
            }
            return GeckoResult.fromValue(page)
        }
    }

    private val progress = object : GeckoSession.ProgressDelegate {
        override fun onPageStart(session: GeckoSession, url: String) {
            promptThrottle.newDocument()
            document.pageStart()
            engine.blocker.newDocument(id)
            loginsNewDocument(url)
            set { it.pageStarted(url) }
        }

        override fun onProgressChange(session: GeckoSession, progress: Int) {
            set { it.copy(progress = progress / 100f, loading = progress < 100) }
        }

        override fun onPageStop(session: GeckoSession, success: Boolean) {
            document.pageStop()
            set { it.pageStopped() }
            // An HTTP sign-in that was not asked for again worked: offer to save it.
            httpAuth.onPageStop(success)?.let { offerSave(it.origin, it.username, it.password) }
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

    /** When the top-level document changed (for site permissions). */
    private val document = DocumentChange(::webOrigin)

    /** The host of the page that is asking (the dialog's title). */
    private fun promptSite(): String = promptSiteOf(current.url)

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

    // ----- saved logins (0.3.0) -----

    private val httpAuth = HttpAuthOffer()

    /** "Fill from vault" was chosen for this origin: the next tap on a sign-in field asks. */
    private var fillArmedOrigin: String? = null

    /** The picker was closed or used on this page: taps do not bring it back until the next load. */
    private var fillSuppressed = false
    private var savePrompt: PagePrompt.LoginSave? = null

    /** This tab's page as the engine's login lookup may know it. */
    val loginPage: LoginPage get() = LoginPage(current.url, private, fillArmedOrigin)

    /**
     * "Fill from vault" in the menu: the explicit request a private tab needs, and the way back
     * to the picker in any tab. False when the page is not a web page.
     */
    fun armFill(): Boolean {
        val origin = originOf(current.url)
        if (!LoginPolicy.isWeb(origin)) return false
        fillArmedOrigin = origin
        fillSuppressed = false
        return true
    }

    private fun loginsNewDocument(url: String) {
        httpAuth.onPageStart(url)
        fillSuppressed = false
        if (fillArmedOrigin != originOf(url)) fillArmedOrigin = null
    }

    private fun loginsLocationChanged() {
        if (fillArmedOrigin != null && fillArmedOrigin != originOf(current.url)) fillArmedOrigin = null
        // A save offer follows the sign-in to the next page of the site, not to another site.
        savePrompt?.let { if (!LoginPolicy.saveStillValid(id, it.origin, id, current.url)) it.close() }
    }

    /** The tab is going away: nothing of a sign-in is kept. */
    private fun loginsClosed() {
        httpAuth.clear()
        savePrompt?.close()
    }

    /** A login was sent from this tab ([origin] is the form's or the server's): ask to save it? */
    private fun offerSave(origin: String?, username: String?, password: String?) {
        val vault = engine.logins.vault ?: return
        // A private tab: nothing is asked of the vault, nothing is kept.
        if (private || origin == null || password.isNullOrEmpty()) return
        val user = username ?: ""
        val state = vault.state()
        engine.logins.scope.launch {
            val known = if (state == LoginVault.State.UNLOCKED) {
                try {
                    // peek: a form being sent is not the user using the vault.
                    vault.peek { it.saveDecision(origin, user, password) }
                } catch (e: VaultFailure) {
                    null
                }
            } else {
                null
            }
            val decision = LoginPolicy.save(
                LoginPolicy.SaveContext(
                    private = private,
                    origin = origin,
                    passwordEmpty = false,
                    vault = state,
                    neverSave = vault.neverSave(origin),
                    offersEnabled = vault.offersEnabled(),
                    known = known,
                ),
            )
            if (decision !is LoginPolicy.Save.Offer || closed) return@launch
            // The tab went elsewhere while the vault was asked: no offer on another site.
            if (!LoginPolicy.saveStillValid(id, origin, id, current.url) && originOf(current.url) != null) return@launch
            savePrompt?.close()
            val prompt = PagePrompt.LoginSave(
                tabId = id,
                site = promptSiteOf(origin),
                origin = origin,
                username = user,
                password = password,
                kind = decision.kind,
                warnInsecure = decision.warnInsecure,
                onClosed = { p ->
                    if (savePrompt === p) savePrompt = null
                    listener.onPromptClosed(this@KrunkSession, p)
                },
            )
            savePrompt = prompt
            listener.onPrompt(this@KrunkSession, prompt)
        }
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
                options.flags and AuthFlags.ONLY_PASSWORD != 0
            // The server that asks, which need not be the page's own.
            val server = options.uri?.let { originOf(it)?.substringAfter("://") } ?: promptSite()
            val challenge = HttpAuthOffer.Challenge(
                uri = options.uri,
                proxy = options.flags and AuthFlags.PROXY != 0,
                crossOriginSubResource =
                    options.flags and AuthFlags.CROSS_ORIGIN_SUB_RESOURCE !=
                        0,
                previousFailed =
                    options.flags and AuthFlags.PREVIOUS_FAILED != 0,
            )
            httpAuth.onChallenge(challenge)
            val vault = engine.logins.vault
            val saved = if (vault == null) {
                HttpAuthOffer.Saved.NONE
            } else {
                httpAuth.saved(challenge, private, vault.state(), options.uri?.let { vault.hasHint(it) } == true)
            }
            return ask(prompt) { answer ->
                PagePrompt.SignIn(
                    tabId = id,
                    site = server,
                    message = prompt.message ?: "",
                    username = options.username ?: "",
                    onlyPassword = onlyPassword,
                    insecure = options.uri?.startsWith("http://", ignoreCase = true) == true,
                    onAnswer = { user, password ->
                        httpAuth.onAnswered(
                            challenge,
                            private,
                            if (onlyPassword) options.username ?: "" else user,
                            password,
                        )
                        answer(if (onlyPassword) prompt.confirm(password) else prompt.confirm(user, password))
                    },
                    onDismiss = {
                        httpAuth.clear()
                        answer(prompt.dismiss())
                    },
                    saved = saved,
                    authUrl = options.uri ?: "",
                    private = private,
                )
            }
        }

        /**
         * A sign-in field was tapped and the engine holds stand-in entries for the page (see
         * LoginsHub). Krunk decides here, where the tab is known, and hands over one login the
         * user picks: built by Krunk, never one of the engine's entries.
         */
        override fun onLoginSelect(
            session: GeckoSession,
            request: GeckoSession.PromptDelegate.AutocompleteRequest<Autocomplete.LoginSelectOption>,
        ): GeckoResult<GeckoSession.PromptDelegate.PromptResponse> {
            val vault = engine.logins.vault
            val formOrigin = request.options.firstOrNull()?.value?.origin
            val pageUrl = current.url
            val decision = if (vault == null) {
                LoginPolicy.Fill.No("no vault")
            } else {
                LoginPolicy.fill(
                    LoginPolicy.FillContext(
                        private = private,
                        armedOrigin = fillArmedOrigin,
                        suppressed = fillSuppressed,
                        pageUrl = pageUrl,
                        formOrigin = formOrigin,
                        vault = vault.state(),
                        hint = vault.hasHint(pageUrl),
                        recentTap = LoginPolicy.recentTap(
                            lastTouchAt.takeIf { it > 0 }?.let { SystemClock.uptimeMillis() - it },
                        ),
                    ),
                )
            }
            if (decision !is LoginPolicy.Fill.Ask || formOrigin == null) {
                return GeckoResult.fromValue(request.dismiss())
            }
            return ask(request) { answer ->
                PagePrompt.LoginFill(
                    tabId = id,
                    site = promptSiteOf(pageUrl),
                    pageUrl = pageUrl,
                    unlockFirst = decision.unlockFirst,
                    warnInsecure = decision.warnInsecure,
                    onPick = { secret ->
                        // Once per page: a filled form does not ask again at the next tap.
                        fillSuppressed = true
                        fillArmedOrigin = null
                        // The tab may have gone elsewhere while the picker was open.
                        if (originOf(current.url) == originOf(pageUrl)) {
                            val login = Autocomplete.LoginEntry.Builder()
                                .origin(formOrigin)
                                .formActionOrigin(formOrigin)
                                .username(secret.username)
                                .password(secret.password)
                                .build()
                            answer(request.confirm(Autocomplete.LoginSelectOption(login)))
                        } else {
                            answer(request.dismiss())
                        }
                    },
                    onDismiss = {
                        fillSuppressed = true
                        // "Fill from vault" was for this one picker, not for every later tap.
                        fillArmedOrigin = null
                        answer(request.dismiss())
                    },
                )
            }
        }

        /**
         * A form with a password was sent. The engine's request is answered at once (nothing is
         * stored through the engine); whether Krunk offers to save is its own question.
         */
        override fun onLoginSave(
            session: GeckoSession,
            request: GeckoSession.PromptDelegate.AutocompleteRequest<Autocomplete.LoginSaveOption>,
        ): GeckoResult<GeckoSession.PromptDelegate.PromptResponse> {
            request.options.firstOrNull()?.value?.let { offerSave(it.origin, it.username, it.password) }
            return GeckoResult.fromValue(request.dismiss())
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

        override fun onFilePrompt(
            session: GeckoSession,
            prompt: GeckoSession.PromptDelegate.FilePrompt,
        ): GeckoResult<GeckoSession.PromptDelegate.PromptResponse> {
            // A whole folder (`webkitdirectory`) is not offered: the page gets no file.
            if (prompt.type == GeckoSession.PromptDelegate.FilePrompt.Type.FOLDER) {
                return GeckoResult.fromValue(prompt.dismiss())
            }
            val request = FileRequests.of(
                multiple = prompt.type == GeckoSession.PromptDelegate.FilePrompt.Type.MULTIPLE,
                accept = prompt.mimeTypes?.toList().orEmpty(),
                capture = prompt.capture != GeckoSession.PromptDelegate.FilePrompt.Capture.NONE,
            ) { MimeTypeMap.getSingleton().getMimeTypeFromExtension(it) }
            return ask(prompt) { answer ->
                PagePrompt.File(
                    tabId = id,
                    site = promptSite(),
                    request = request,
                    onAnswer = { uris ->
                        // The engine reads plain files: it gets Krunk's own copies (see Uploads).
                        engine.uploads.copy(id, uris) { copies ->
                            answer(
                                when {
                                    copies.isEmpty() || closed -> prompt.dismiss()
                                    request.multiple -> prompt.confirm(engine.appContext, copies.toTypedArray())
                                    else -> prompt.confirm(engine.appContext, copies.first())
                                },
                            )
                        }
                    },
                    onDismiss = { answer(prompt.dismiss()) },
                )
            }
        }

        override fun onDateTimePrompt(
            session: GeckoSession,
            prompt: GeckoSession.PromptDelegate.DateTimePrompt,
        ): GeckoResult<GeckoSession.PromptDelegate.PromptResponse> {
            val kind = when (prompt.type) {
                GeckoSession.PromptDelegate.DateTimePrompt.Type.DATE -> DateKind.DATE
                GeckoSession.PromptDelegate.DateTimePrompt.Type.TIME -> DateKind.TIME
                GeckoSession.PromptDelegate.DateTimePrompt.Type.DATETIME_LOCAL -> DateKind.DATE_TIME
                GeckoSession.PromptDelegate.DateTimePrompt.Type.MONTH -> DateKind.MONTH
                GeckoSession.PromptDelegate.DateTimePrompt.Type.WEEK -> DateKind.WEEK
                else -> return GeckoResult.fromValue(prompt.dismiss())
            }
            return ask(prompt) { answer ->
                PagePrompt.DateTime(
                    tabId = id,
                    site = promptSite(),
                    kind = kind,
                    value = prompt.defaultValue.orEmpty(),
                    min = prompt.minValue.orEmpty(),
                    max = prompt.maxValue.orEmpty(),
                    onAnswer = { answer(prompt.confirm(it)) },
                    onDismiss = { answer(prompt.dismiss()) },
                )
            }
        }

        override fun onColorPrompt(
            session: GeckoSession,
            prompt: GeckoSession.PromptDelegate.ColorPrompt,
        ): GeckoResult<GeckoSession.PromptDelegate.PromptResponse> = ask(prompt) { answer ->
            PagePrompt.Colour(
                tabId = id,
                site = promptSite(),
                value = prompt.defaultValue.orEmpty(),
                suggested = prompt.predefinedValues?.toList().orEmpty(),
                onAnswer = { answer(prompt.confirm(it)) },
                onDismiss = { answer(prompt.dismiss()) },
            )
        }

        override fun onBeforeUnloadPrompt(
            session: GeckoSession,
            prompt: GeckoSession.PromptDelegate.BeforeUnloadPrompt,
        ): GeckoResult<GeckoSession.PromptDelegate.PromptResponse> = ask(prompt) { answer ->
            PagePrompt.LeavePage(id, promptSite()) { leave ->
                answer(prompt.confirm(if (leave) AllowOrDeny.ALLOW else AllowOrDeny.DENY))
            }
        }

        override fun onRepostConfirmPrompt(
            session: GeckoSession,
            prompt: GeckoSession.PromptDelegate.RepostConfirmPrompt,
        ): GeckoResult<GeckoSession.PromptDelegate.PromptResponse> = ask(prompt) { answer ->
            PagePrompt.Repost(id, promptSite()) { send ->
                answer(prompt.confirm(if (send) AllowOrDeny.ALLOW else AllowOrDeny.DENY))
            }
        }

        override fun onSharePrompt(
            session: GeckoSession,
            prompt: GeckoSession.PromptDelegate.SharePrompt,
        ): GeckoResult<GeckoSession.PromptDelegate.PromptResponse> {
            val text = Shares.text(prompt.text, prompt.uri)
                ?: return GeckoResult.fromValue(prompt.confirm(GeckoSession.PromptDelegate.SharePrompt.Result.FAILURE))
            return ask(prompt) { answer ->
                PagePrompt.Share(id, promptSite(), prompt.title.orEmpty().take(MAX_SHARE_TITLE), text) { outcome ->
                    answer(
                        prompt.confirm(
                            when (outcome) {
                                ShareOutcome.SHARED -> GeckoSession.PromptDelegate.SharePrompt.Result.SUCCESS
                                ShareOutcome.ABORTED -> GeckoSession.PromptDelegate.SharePrompt.Result.ABORT
                                ShareOutcome.FAILED -> GeckoSession.PromptDelegate.SharePrompt.Result.FAILURE
                            },
                        ),
                    )
                }
            }
        }
    }

    /** The tab's back/forward list as the engine last reported it, and the shown entry's index. */
    var historyEntries: List<HistoryPage> = emptyList()
        private set
    var historyIndex: Int = 0
        private set

    /** Jumps to an entry of [historyEntries] (the long-press list of Back and Forward). */
    fun goToHistory(index: Int) {
        if (index in historyEntries.indices && index != historyIndex) gecko.gotoHistoryIndex(index)
    }

    private val history = object : GeckoSession.HistoryDelegate {
        override fun onHistoryStateChange(session: GeckoSession, state: GeckoSession.HistoryDelegate.HistoryList) {
            // The engine marks address and title as never null, but a page without a title
            // (still loading, a blank page) arrives with a null one: seen on the device.
            historyEntries = state.map { item ->
                HistoryPage((item.uri as String?).orEmpty(), (item.title as String?).orEmpty())
            }
            historyIndex = state.currentIndex.coerceIn(0, maxOf(0, historyEntries.size - 1))
        }

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
        gecko.permissionDelegate = engine.permissions?.delegate(
            SitePermissions.Tab(
                id = id,
                private = private,
                pageUrl = { current.url },
                show = { listener.onPrompt(this, it) },
                closed = { listener.onPromptClosed(this, it) },
            ),
        )
        if (!openedByPage) gecko.open(engine.runtime)
        engine.icons.listen(gecko) { url, icons -> listener.onIcons(this, url, icons) }
        engine.blocker.attach(id, gecko)
    }
}

/** A page's title for a share is a label, not content: bounded. */
private const val MAX_SHARE_TITLE = 200
