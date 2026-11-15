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
    /** `null` until the engine has reported on the page's connection (blank page, a load under way). */
    val secure: Boolean? = null,
    /**
     * The lock state of the committed page, set aside while a load to another site is under way
     * (it comes back if that load never commits).
     */
    val secureBeforeLoad: Boolean? = null,
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
    /** The address of the top-level load that started last (what the blocker's verdict is matched to). */
    val loadingUrl: String? = null,
    /**
     * The blocker stopped a top-level load to this address (it is on a filter list). Krunk shows
     * its own "blocked" panel instead of the page until the tab goes somewhere else.
     */
    val blocked: String? = null,
)

/**
 * A failed load: the address that failed and the error. [pageUri] is the `data:` address of the
 * error page Krunk handed to the engine for it ("" when unknown).
 */
data class LoadError(
    val uri: String,
    val category: Int,
    val code: Int,
    val pageUri: String = "",
    /** The load failed on the site's certificate: Krunk's certificate screen is shown over the page. */
    val cert: CertError? = null,
)

/** One entry of a tab's back/forward list. */
data class HistoryPage(val url: String, val title: String)

const val ABOUT_BLANK = "about:blank"

/** Scheme and authority (up to the first `/`, `?`, `#` or backslash) of a hierarchical address. */
private val ORIGIN = Regex("""^([A-Za-z][A-Za-z0-9+.-]*)://([^/?#\\]*)""")

// ----- transitions (pure; KrunkSession applies them from the engine's callbacks) -----

/** The app asked the engine to load [url]. */
fun PageState.requested(url: String): PageState =
    copy(pendingUrl = url, loadError = null, crashed = false, blocked = null)

/** The app parked the engine on a blank page (a native screen such as the new-tab page is shown). */
fun PageState.parked(): PageState = copy(pendingUrl = null, loadError = null, crashed = false, blocked = null)

/**
 * `scheme://host:port` of a hierarchical address, lower case and without user:password, or null
 * (`about:`, `data:`, ...). Two addresses with the same value are the same site for the lock.
 */
fun originOf(url: String): String? {
    val m = ORIGIN.find(url) ?: return null
    val host = m.groupValues[2].substringAfterLast('@')
    if (host.isEmpty()) return null
    return (m.groupValues[1] + "://" + host).lowercase()
}

/**
 * A top-level load started. The lock belongs to the committed page: when the load goes to
 * another site it is taken down until the engine reports on the new connection, so a new
 * address can never be shown with the previous page's lock.
 */
fun PageState.pageStarted(startedUrl: String): PageState {
    val from = originOf(url)
    val sameSite = from != null && from == originOf(startedUrl)
    return copy(
        // A new document: the previous page's title is not its title (history must not record
        // the visit under it); the page reports its own.
        title = "",
        progress = 0.05f,
        loading = true,
        // Krunk's own error page starting to show is still the failed load, not a new one.
        loadError = loadError?.takeIf { it.pageUri.isNotEmpty() && it.pageUri == startedUrl },
        crashed = false,
        loadingUrl = startedUrl,
        blocked = null,
        secure = if (sameSite) secure else null,
        secureBeforeLoad = if (sameSite) secureBeforeLoad else secure ?: secureBeforeLoad,
    )
}

/** The engine reports on the connection of the page it shows. */
fun PageState.securityChanged(isSecure: Boolean): PageState = copy(secure = isSecure)

/** The engine reports a new top-level location. */
fun PageState.locationChanged(newUrl: String): PageState {
    // Every session starts by reporting a blank page before anything was loaded: not a location.
    if (newUrl == ABOUT_BLANK && !committed && !initialBlankSeen) return copy(initialBlankSeen = true)
    val err = loadError
    return when {
        // Krunk's own error page (exactly the one handed over) or the failed address itself:
        // the bar keeps the address that failed.
        err != null && (newUrl == err.uri || (err.pageUri.isNotEmpty() && newUrl == err.pageUri)) ->
            copy(url = err.uri, pendingUrl = null, committed = true, secureBeforeLoad = null)

        else -> copy(url = newUrl, pendingUrl = null, loadError = null, committed = true, secureBeforeLoad = null)
    }
}

/**
 * The page's process crashed or was killed by the system. The engine has closed the session:
 * nothing is loading any more, full screen is over, and the tab needs a new session to continue.
 */
fun PageState.crashed(): PageState =
    copy(crashed = true, loading = false, progress = 0f, fullScreen = false, pendingUrl = null)

/**
 * The blocker stopped the top-level load to [url]: nothing was requested and nothing committed.
 * The tab shows Krunk's "blocked" panel for that address; the page under it (if any) stays.
 */
fun PageState.blockedByBlocker(url: String): PageState =
    copy(blocked = url, loading = false, progress = 0f, pendingUrl = null, loadError = null)

/** The user left the "blocked" panel for the page that was there before. */
fun PageState.blockedDismissed(): PageState = copy(blocked = null)

/** Whether a tab showing the "blocked" panel has a page to go back to (else it is closed). */
fun PageState.hasPageUnderBlocked(): Boolean = committed && url.isNotEmpty() && url != ABOUT_BLANK

/** What "back" means for the page on screen (the menu's Back and the system back alike). */
enum class BackAction {
    /** Nothing to go back to inside the tab. */
    NONE,

    /** Leave Krunk's "blocked" panel for the page under it (or close a tab that never had one). */
    LEAVE_BLOCKED,

    /** One step back in the page's history. */
    HISTORY,
}

fun backAction(page: PageState?): BackAction = when {
    page == null -> BackAction.NONE
    page.blocked != null -> BackAction.LEAVE_BLOCKED
    page.canGoBack -> BackAction.HISTORY
    else -> BackAction.NONE
}

/** A load ended. One that never committed (a download, "204 No Content", stop) leaves the old page. */
fun PageState.pageStopped(): PageState = copy(
    progress = 0f,
    loading = false,
    pendingUrl = null,
    // Nothing committed since the lock was set aside: the old page is still shown, with its lock.
    secure = secure ?: secureBeforeLoad,
    secureBeforeLoad = null,
)

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

    // The address the blocker stopped: the panel is about it, and the user must see which.
    page.blocked != null -> page.blocked

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

/** The engine's visit flags (`GeckoSession.HistoryDelegate.VISIT_*`), as numbers for the JVM tests. */
object VisitFlags {
    const val TOP_LEVEL = 1
    const val REDIRECT_SOURCE = 8
    const val REDIRECT_SOURCE_PERMANENT = 16
    const val UNRECOVERABLE_ERROR = 32
}

/**
 * Whether a visit the engine reports goes into the history: top-level pages that loaded, and
 * not the addresses that only redirected somewhere else (the user never saw those).
 */
fun isRecordableVisit(flags: Int): Boolean = flags and VisitFlags.TOP_LEVEL != 0 &&
    flags and VisitFlags.UNRECOVERABLE_ERROR == 0 &&
    flags and (VisitFlags.REDIRECT_SOURCE or VisitFlags.REDIRECT_SOURCE_PERMANENT) == 0
