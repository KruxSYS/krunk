package org.krunk.browser.engine

/** What the UI shows for one tab's page: everything the engine reports. */
data class PageState(
    /** The location the engine last committed ("" before the first one). */
    val url: String = "",
    /**
     * An address the app asked the engine to load that has not committed yet. The address bar
     * shows it while it loads; it never counts as the page's address (no lock, no history).
     */
    val pendingUrl: String? = null,
    /** The engine has committed a real location (anything after the session's first blank page). */
    val committed: Boolean = false,
    /** The blank page every new session starts with has been reported. */
    val initialBlankSeen: Boolean = false,
    val title: String = "",
    /** 0..1 while loading, 0 when idle. */
    val progress: Float = 0f,
    val loading: Boolean = false,
    /** `null` until the first security callback (about:blank, new tab). */
    val secure: Boolean? = null,
    val canGoBack: Boolean = false,
    val canGoForward: Boolean = false,
    val fullScreen: Boolean = false,
    val desktopMode: Boolean = false,
    /** The last load failed with this error page shown. */
    val loadError: LoadError? = null,
    /** Find in page: current match index (1-based) and total. */
    val findCurrent: Int = 0,
    val findTotal: Int = 0,
    /** Serialised Gecko state (back/forward list, scroll, form data) for restore. */
    val sessionState: String? = null,
    /** The page crashed or was killed by the OS; a reload recreates it. */
    val crashed: Boolean = false,
)

/**
 * A failed load: the address that failed and the error. [pageUri] is the `data:` address of the
 * error page Krunk handed to the engine for it ("" when unknown).
 */
data class LoadError(val uri: String, val category: Int, val code: Int, val pageUri: String = "")

const val ABOUT_BLANK = "about:blank"

// ----- transitions (pure; KrunkSession applies them from the engine's callbacks) -----

/** The app asked the engine to load [url]. */
fun PageState.requested(url: String): PageState = copy(pendingUrl = url, loadError = null, crashed = false)

/** The app parked the engine on a blank page (a native screen such as the new-tab page is shown). */
fun PageState.parked(): PageState = copy(pendingUrl = null, loadError = null, crashed = false)

/** The engine reports a new top-level location. */
fun PageState.locationChanged(newUrl: String): PageState {
    // Every session starts by reporting a blank page before anything was loaded: not a location.
    if (newUrl == ABOUT_BLANK && !committed && !initialBlankSeen) return copy(initialBlankSeen = true)
    val err = loadError
    return when {
        // Krunk's own error page (exactly the one handed over) or the failed address itself:
        // the bar keeps the address that failed.
        err != null && (newUrl == err.uri || (err.pageUri.isNotEmpty() && newUrl == err.pageUri)) ->
            copy(url = err.uri, pendingUrl = null, committed = true)

        else -> copy(url = newUrl, pendingUrl = null, loadError = null, committed = true)
    }
}

/**
 * The page's process crashed or was killed by the system. The engine has closed the session:
 * nothing is loading any more, full screen is over, and the tab needs a new session to continue.
 */
fun PageState.crashed(): PageState =
    copy(crashed = true, loading = false, progress = 0f, fullScreen = false, pendingUrl = null)

/** A load ended. One that never committed (a download, "204 No Content", stop) leaves the old page. */
fun PageState.pageStopped(): PageState = copy(progress = 0f, loading = false, pendingUrl = null)

/**
 * The address the address bar shows for a tab.
 *
 * [modelUrl] is what the tab model holds (the address a tab was opened or restored with);
 * [modelIsInternal] says it names a native Krunk page; [page] is the engine's state when the tab
 * has a session ([hasSession]); [openedByPage] marks a window a web page opened.
 *
 * The engine's committed location always wins once there is one. Before that, an address is
 * shown only when Krunk itself asked for it: a window opened by a page shows nothing until it
 * commits, because its opener can draw into it while the requested address never loads.
 */
fun displayedUrl(
    modelUrl: String,
    modelIsInternal: Boolean,
    page: PageState,
    hasSession: Boolean,
    openedByPage: Boolean,
): String = when {
    !hasSession -> modelUrl
    page.pendingUrl != null -> page.pendingUrl
    modelIsInternal && (page.url.isEmpty() || page.url == ABOUT_BLANK) -> modelUrl
    page.committed -> if (page.url == ABOUT_BLANK) "" else page.url
    openedByPage -> ""
    else -> modelUrl
}

/**
 * The address to keep in the tab model (what a restart restores) after a state change, or null
 * to leave it. Only committed locations are taken over: a pending address stays the model's
 * until it commits or is dropped, a page that went blank is blank, and an address on Krunk's
 * internal origin reported by the engine ([engineUrlIsInternal]) never becomes a native page.
 */
fun modelUrlAfter(page: PageState, modelIsInternal: Boolean, engineUrlIsInternal: Boolean): String? = when {
    page.pendingUrl != null || !page.committed || page.url.isEmpty() -> null
    page.url == ABOUT_BLANK -> if (modelIsInternal) null else ABOUT_BLANK
    engineUrlIsInternal -> null
    else -> page.url
}
