package org.krunk.browser.ui.browser

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.windowInsetsTopHeight
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlin.math.roundToInt
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import org.krunk.browser.R
import org.krunk.browser.engine.ContextTarget
import org.krunk.browser.engine.KrunkSession
import org.krunk.browser.engine.LoadSource
import org.krunk.browser.engine.NestedGeckoView
import org.krunk.browser.engine.originOf
import org.krunk.browser.tabs.TabEvent
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.Screen
import org.krunk.browser.ui.newtab.NewTabScreen
import org.krunk.browser.ui.tabs.RestoreTabsPrompt
import org.krunk.browser.ui.theme.LocalKrunkDark
import org.krunk.ffi.ToolbarPosition

/**
 * The browser: the page (GeckoView, or the native new-tab page), the toolbar at the top (the
 * default) or the bottom (hides while scrolling down, via the page's nested scroll), the address editor with
 * suggestions, the find bar, the link long-press sheet, downloads, full screen, back order.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun BrowserScreen(vm: BrowserViewModel) {
    val tab by vm.activeTab.collectAsStateWithLifecycle()
    val tabs by vm.tabs.tabs.collectAsStateWithLifecycle()
    val settings by vm.settings.collectAsStateWithLifecycle()
    val editing by vm.editing.collectAsStateWithLifecycle()
    val suggestions by vm.suggestions.collectAsStateWithLifecycle()
    val findActive by vm.findActive.collectAsStateWithLifecycle()
    val findText by vm.findText.collectAsStateWithLifecycle()
    val page = tab?.page
    val session = tab?.let { vm.tabs.session(it.id) }
    val fullScreen = page?.fullScreen == true
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    val dark = LocalKrunkDark.current
    val toolbarScroll = rememberToolbarScrollState()
    val toolbarHeightPx = with(LocalDensity.current) { ToolbarHeight.toPx() }
    val top = settings.toolbarPosition == ToolbarPosition.TOP
    var geckoView by remember { mutableStateOf<NestedGeckoView?>(null) }
    var contextTarget by remember { mutableStateOf<Pair<UInt, ContextTarget>?>(null) }
    var refreshing by remember { mutableStateOf(false) }
    var download by remember { mutableStateOf<PendingDownload?>(null) }

    SystemBarsEffect(fullScreen = fullScreen, darkTheme = dark)

    // Back, in order: find bar, address editing, full screen, page history; else the activity.
    BackHandler(enabled = findActive) { vm.closeFind() }
    BackHandler(enabled = !findActive && editing) { vm.stopEditing() }
    BackHandler(enabled = !findActive && !editing && fullScreen) { session?.exitFullScreen() }
    BackHandler(enabled = !findActive && !editing && !fullScreen && page?.canGoBack == true) { session?.goBack() }

    val crashed = page?.crashed == true
    val toolbarMayHide = !editing && !findActive && !fullScreen && tab?.isNewTab == false && !crashed
    SideEffect {
        toolbarScroll.heightPx = toolbarHeightPx
        toolbarScroll.enabled = toolbarMayHide
    }
    // The toolbar comes back whenever the page under it is about to be another one: a load
    // starts or ends, the site changes. A hidden address bar must not outlive a navigation, and
    // on a page that cannot scroll nothing else would bring it back.
    val loading = page?.loading == true
    val site = originOf(tab?.url ?: "")
    LaunchedEffect(tab?.id, editing, findActive, tab?.isNewTab, crashed, loading, site) { toolbarScroll.show() }
    // The find bar belongs to the tab it was opened on.
    LaunchedEffect(tab?.id) { vm.closeFind() }
    // Pull-to-refresh ends with the reload. It follows the session's own state, because a reload
    // can start and finish within one frame (an error page), which composition would never see.
    LaunchedEffect(refreshing, session) {
        if (!refreshing) return@LaunchedEffect
        if (session != null) {
            withTimeoutOrNull(REFRESH_START_MS) { session.state.first { it.loading } }
            withTimeoutOrNull(REFRESH_MAX_MS) { session.state.first { !it.loading } }
        }
        refreshing = false
    }
    // Gecko is told how far the toolbar has slid away (0 = fully shown, -height = hidden), so
    // it keeps bottom-fixed page elements inside what is really visible. Collected outside of
    // composition: the screen is not recomposed for every scrolled pixel.
    LaunchedEffect(geckoView, fullScreen) {
        val view = geckoView ?: return@LaunchedEffect
        snapshotFlow { toolbarScroll.offsetPx }.collect { hidden ->
            view.setVerticalClipping(if (fullScreen) 0 else -hidden.roundToInt())
        }
    }

    val downloadStarted = stringResource(R.string.download_started)
    val downloadUnsupported = stringResource(R.string.download_unsupported)
    val pageUnavailable = stringResource(R.string.page_unavailable)

    // The database could not be opened: say once that this session keeps nothing.
    val storageFailed = stringResource(R.string.storage_failed)
    LaunchedEffect(Unit) {
        if (vm.core.storageError != null) snackbar.showSnackbar(storageFailed, duration = SnackbarDuration.Long)
    }

    // Messages are shown from their own coroutine: the collector below must not wait the
    // seconds a snackbar stays, or the next event (a link menu) would be late.
    fun toast(msg: String) = scope.launch { snackbar.showSnackbar(msg) }
    LaunchedEffect(Unit) {
        vm.tabs.events.collect { ev ->
            when (ev) {
                is TabEvent.ContextMenu -> {
                    val private = vm.tabs.tabs.value.firstOrNull { it.id == ev.tabId }?.private == true
                    if (contextActions(ev.target, private).isNotEmpty()) contextTarget = ev.tabId to ev.target
                }

                // A page started this download: nothing is fetched or stored before the user
                // said yes. One question at a time; further ones are dropped while it is open.
                is TabEvent.Download -> {
                    val private = vm.tabs.tabs.value.firstOrNull { it.id == ev.tabId }?.private == true
                    val asked = Downloads.describe(ev.response, private)
                    if (download == null) download = asked else Downloads.discard(asked)
                }

                // A krunk:// page the Windows browser has and Android does not: say so.
                is TabEvent.OpenScreen -> Screen.forPage(ev.page)?.let { vm.push(it) } ?: toast(pageUnavailable)
            }
        }
    }

    val chromeVisible = !fullScreen

    Box(Modifier.fillMaxSize()) {
        // With the toolbar at the top the status bar strip takes the toolbar's colour, so the two
        // read as one header (also while the toolbar slides away under it).
        if (top && chromeVisible) {
            Spacer(
                Modifier
                    .fillMaxWidth()
                    .windowInsetsTopHeight(WindowInsets.safeDrawing)
                    .background(MaterialTheme.colorScheme.surfaceContainer),
            )
        }
        // Everything stays inside the safe area: the page is moved down by the visible toolbar and
        // the toolbar slides out of view, and neither may draw under the system bars.
        Box(Modifier.fillMaxSize().then(if (fullScreen) Modifier else Modifier.safeDrawingPadding().clipToBounds())) {
            // ----- page -----
            if (tab?.isNewTab == true || session == null) {
                NewTabScreen(
                    vm,
                    Modifier
                        .fillMaxSize()
                        .padding(top = if (top) ToolbarHeight else 0.dp, bottom = if (!top) ToolbarHeight else 0.dp),
                )
            } else if (crashed) {
                PageCrashed(
                    onReload = { vm.tabs.reload() },
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(top = if (top) ToolbarHeight else 0.dp, bottom = if (!top) ToolbarHeight else 0.dp),
                )
            } else {
                PullToRefreshBox(
                    isRefreshing = refreshing,
                    onRefresh = {
                        refreshing = true
                        vm.tabs.reload()
                    },
                    modifier = Modifier
                        .fillMaxSize()
                        .nestedScroll(toolbarScroll)
                        // Read in the layer, not in composition: no recomposition per scrolled pixel.
                        .graphicsLayer {
                            translationY = if (top && chromeVisible) toolbarHeightPx - toolbarScroll.offsetPx else 0f
                        },
                ) {
                    PageView(
                        session = session,
                        fullScreen = fullScreen,
                        toolbarMaxPx = toolbarHeightPx,
                        onView = { geckoView = it },
                    )
                }
            }

            // ----- chrome -----
            if (chromeVisible) {
                val align = if (top) Alignment.TopCenter else Alignment.BottomCenter
                Column(
                    Modifier
                        .align(align)
                        .fillMaxWidth()
                        .graphicsLayer {
                            translationY = if (top) -toolbarScroll.offsetPx else toolbarScroll.offsetPx
                        },
                ) {
                    if (!top && page != null && page.loading) ProgressLine(page.progress)
                    when {
                        findActive -> FindBar(
                            text = findText,
                            current = page?.findCurrent ?: 0,
                            total = page?.findTotal ?: 0,
                            onText = vm::findTyped,
                            onPrevious = { vm.findNext(forward = false) },
                            onNext = { vm.findNext(forward = true) },
                            onClose = vm::closeFind,
                        )

                        else -> BrowserToolbar(
                            vm = vm,
                            tab = tab,
                            tabCount = tabs.size,
                            onOpenTabs = {
                                val view = geckoView
                                if (view != null && tab?.isNewTab == false) {
                                    vm.tabs.captureThumbnail(view) { vm.push(Screen.TabSwitcher) }
                                } else {
                                    vm.push(Screen.TabSwitcher)
                                }
                            },
                            onMessage = { msg -> scope.launch { snackbar.showSnackbar(msg) } },
                        )
                    }
                    if (top && page != null && page.loading) ProgressLine(page.progress)
                }
            }

            // ----- address editing: editor at the top, suggestions below, over everything -----
            if (editing) {
                val placeholder = stringResource(R.string.address_placeholder, vm.settingsRepo.searchName)
                Column(Modifier.fillMaxSize()) {
                    Box(
                        // Its own background: without one the page (or the toolbar) shows around the field.
                        Modifier
                            .fillMaxWidth()
                            .background(MaterialTheme.colorScheme.surfaceContainer)
                            .height(ToolbarHeight)
                            .padding(horizontal = 8.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        AddressEditor(vm, placeholder, Modifier.fillMaxWidth())
                    }
                    SuggestionsPanel(
                        suggestions = suggestions,
                        onPick = { row -> vm.goTo(row.url) },
                        onFill = { row -> vm.fillTyped(row.fill.ifEmpty { row.url }) },
                        onDismiss = { vm.stopEditing() },
                        modifier = Modifier.fillMaxSize(),
                    )
                }
            }

            SnackbarHost(
                snackbar,
                Modifier
                    .align(Alignment.BottomCenter)
                    .padding(bottom = if (!top && chromeVisible) ToolbarHeight else 0.dp),
            )
            RestoreTabsPrompt(vm)
        }
    }

    // What the page on screen asks (alert, confirm, prompt, a list, sign-in), one at a time.
    val pagePrompt by vm.tabs.prompt.collectAsStateWithLifecycle()
    pagePrompt?.let { PagePromptDialog(it) }

    download?.let { asked ->
        DownloadQuestion(
            download = asked,
            onDownload = {
                download = null
                val name = Downloads.start(context, asked)
                toast(if (name != null) downloadStarted.format(name) else downloadUnsupported)
            },
            onCancel = {
                download = null
                Downloads.discard(asked)
            },
        )
    }

    // A page can go full screen on any tap and then draw its own "browser": say which site it is
    // and how to get out, every time.
    val fullScreenHint = stringResource(R.string.fullscreen_hint)
    val fullScreenHintSite = stringResource(R.string.fullscreen_hint_site)
    LaunchedEffect(fullScreen) {
        if (fullScreen) {
            val site = pillText(tab?.url ?: "")
            snackbar.showSnackbar(if (site.isEmpty()) fullScreenHint else fullScreenHintSite.format(site))
        }
    }

    contextTarget?.let { (tabId, target) ->
        val private = tab?.private == true
        val copied = stringResource(R.string.copied)
        val opened = stringResource(R.string.tab_opened_background)
        val shareTitle = stringResource(R.string.share_chooser)
        ContextMenuSheet(
            target = target,
            privateTab = private,
            onDismiss = { contextTarget = null },
            onAction = { action ->
                contextTarget = null
                val link = target.linkUri ?: ""
                val src = target.srcUri ?: ""
                when (action) {
                    // A page chose these addresses: web addresses only, loaded on its behalf.
                    ContextAction.OPEN_NEW_TAB -> {
                        val id = vm.tabs.openUntrusted(
                            link,
                            LoadSource.PAGE,
                            private = private,
                            activate = false,
                            opener = tabId,
                        )
                        if (id != null) toast(opened)
                    }

                    ContextAction.OPEN_PRIVATE_TAB -> vm.tabs.openUntrusted(
                        link,
                        LoadSource.PAGE,
                        private = true,
                        activate = true,
                        opener = tabId,
                    )

                    ContextAction.COPY_LINK -> if (!Share.copy(context, "URL", link, private)) toast(copied)

                    ContextAction.SHARE_LINK -> Share.text(context, link, target.copyableText, shareTitle)

                    ContextAction.COPY_LINK_TEXT -> if (!Share.copy(
                            context,
                            "Text",
                            target.copyableText ?: "",
                            private,
                        )
                    ) {
                        toast(copied)
                    }

                    ContextAction.DOWNLOAD_LINK -> {
                        val name = Downloads.enqueueUrl(context, link)
                        toast(if (name != null) downloadStarted.format(name) else downloadUnsupported)
                    }

                    ContextAction.OPEN_IMAGE_NEW_TAB -> {
                        val id = vm.tabs.openUntrusted(
                            src,
                            LoadSource.PAGE,
                            private = private,
                            activate = false,
                            opener = tabId,
                        )
                        if (id != null) toast(opened)
                    }

                    ContextAction.SAVE_IMAGE, ContextAction.SAVE_MEDIA -> {
                        val name = Downloads.enqueueUrl(context, src)
                        toast(if (name != null) downloadStarted.format(name) else downloadUnsupported)
                    }

                    ContextAction.COPY_IMAGE_LINK -> if (!Share.copy(context, "URL", src, private)) toast(copied)
                }
            },
        )
    }
}

/** Asks before a download a page started: the file name and the site it comes from. */
@Composable
private fun DownloadQuestion(download: PendingDownload, onDownload: () -> Unit, onCancel: () -> Unit) {
    AlertDialog(
        onDismissRequest = onCancel,
        title = { Text(stringResource(R.string.download_confirm_title)) },
        text = {
            Column {
                Text(download.fileName, style = MaterialTheme.typography.bodyLarge)
                if (download.host.isNotEmpty()) {
                    Text(
                        stringResource(R.string.download_confirm_from, download.host),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(top = 4.dp),
                    )
                }
                if (download.private) {
                    Text(
                        stringResource(R.string.download_confirm_private),
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.padding(top = 12.dp),
                    )
                }
            }
        },
        confirmButton = { TextButton(onClick = onDownload) { Text(stringResource(R.string.download_confirm_yes)) } },
        dismissButton = { TextButton(onClick = onCancel) { Text(stringResource(android.R.string.cancel)) } },
    )
}

/** Shown instead of the page after its process crashed or was killed by the system. */
@Composable
private fun PageCrashed(onReload: () -> Unit, modifier: Modifier = Modifier) {
    Column(
        modifier.padding(32.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(stringResource(R.string.page_crashed_title), style = MaterialTheme.typography.titleLarge)
        Text(
            stringResource(R.string.page_crashed_message),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 8.dp, bottom = 24.dp),
        )
        Button(onClick = onReload) { Text(stringResource(R.string.toolbar_reload)) }
    }
}

private const val REFRESH_START_MS = 1_500L
private const val REFRESH_MAX_MS = 20_000L

@Composable
private fun ProgressLine(progress: Float) {
    LinearProgressIndicator(progress = { progress }, modifier = Modifier.fillMaxWidth().height(3.dp))
}

/**
 * The GeckoView of the active session. Gecko lays the page out with [toolbarMaxPx] reserved for
 * the toolbar and shows the reserved strip as the toolbar slides away (the browser screen
 * reports how far with `setVerticalClipping`), so the page never jumps while the toolbar hides.
 */
@Composable
private fun PageView(
    session: KrunkSession,
    fullScreen: Boolean,
    toolbarMaxPx: Float,
    onView: (NestedGeckoView?) -> Unit,
) {
    AndroidView(
        modifier = Modifier.fillMaxSize(),
        factory = { ctx ->
            NestedGeckoView(ctx).also { onView(it) }
        },
        update = { view ->
            if (view.session != session.gecko) {
                if (view.session != null) view.releaseSession()
                view.setSession(session.gecko)
            }
            view.nestedScrollAllowed = !fullScreen
            val max = if (fullScreen) 0 else toolbarMaxPx.toInt()
            view.setDynamicToolbarMaxHeight(max)
        },
        onRelease = { view ->
            onView(null)
            view.releaseSession()
        },
    )
}
