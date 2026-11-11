package org.krunk.browser.ui.browser

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.launch
import org.krunk.browser.R
import org.krunk.browser.engine.ContextTarget
import org.krunk.browser.engine.KrunkSession
import org.krunk.browser.engine.NestedGeckoView
import org.krunk.browser.tabs.TabEvent
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.Screen
import org.krunk.browser.ui.newtab.NewTabScreen
import org.krunk.browser.ui.tabs.RestoreTabsPrompt
import org.krunk.browser.ui.theme.LocalKrunkDark
import org.krunk.ffi.ToolbarPosition

/**
 * The browser: the page (GeckoView, or the native new-tab page), the toolbar at the bottom or
 * the top (hides while scrolling down, via the page's nested scroll), the address editor with
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

    SystemBarsEffect(fullScreen = fullScreen, darkTheme = dark)

    // Back, in order: find bar, address editing, full screen, page history; else the activity.
    BackHandler(enabled = findActive) { vm.closeFind() }
    BackHandler(enabled = !findActive && editing) { vm.stopEditing() }
    BackHandler(enabled = !findActive && !editing && fullScreen) { session?.exitFullScreen() }
    BackHandler(enabled = !findActive && !editing && !fullScreen && page?.canGoBack == true) { session?.goBack() }

    toolbarScroll.heightPx = toolbarHeightPx
    toolbarScroll.enabled = !editing && !findActive && !fullScreen && tab?.isNewTab == false
    LaunchedEffect(tab?.id, editing, findActive, tab?.isNewTab) { toolbarScroll.show() }
    LaunchedEffect(page?.loading) { if (page?.loading != true) refreshing = false }

    val downloadStarted = stringResource(R.string.download_started)
    val downloadUnsupported = stringResource(R.string.download_unsupported)
    LaunchedEffect(Unit) {
        vm.tabs.events.collect { ev ->
            when (ev) {
                is TabEvent.ContextMenu -> contextTarget = ev.tabId to ev.target

                is TabEvent.Download -> {
                    val name = Downloads.enqueue(context, ev.response)
                    snackbar.showSnackbar(if (name != null) downloadStarted.format(name) else downloadUnsupported)
                }

                is TabEvent.OpenScreen -> Screen.forPage(ev.page)?.let { vm.push(it) }

                is TabEvent.RestorePrompt -> Unit
            }
        }
    }

    val chromeVisible = !fullScreen
    val visibleToolbarPx = if (chromeVisible) toolbarHeightPx - toolbarScroll.offsetPx else 0f

    Box(Modifier.fillMaxSize().then(if (fullScreen) Modifier else Modifier.safeDrawingPadding())) {
        // ----- page -----
        if (tab?.isNewTab == true || session == null) {
            NewTabScreen(
                vm,
                Modifier
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
                    .graphicsLayer { translationY = if (top) visibleToolbarPx else 0f },
            ) {
                PageView(
                    session = session,
                    fullScreen = fullScreen,
                    toolbarMaxPx = toolbarHeightPx,
                    visibleToolbarPx = visibleToolbarPx,
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
                    Modifier.fillMaxWidth().height(ToolbarHeight).padding(horizontal = 8.dp),
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
                fun toast(msg: String) = scope.launch { snackbar.showSnackbar(msg) }
                when (action) {
                    ContextAction.OPEN_NEW_TAB -> {
                        vm.tabs.openTab(link, private = private, activate = false, opener = tabId)
                        toast(opened)
                    }

                    ContextAction.OPEN_PRIVATE_TAB -> vm.tabs.openTab(
                        link,
                        private = true,
                        activate = true,
                        opener = tabId,
                    )

                    ContextAction.COPY_LINK -> if (!Share.copy(context, "URL", link)) toast(copied)

                    ContextAction.SHARE_LINK -> Share.text(context, link, target.copyableText, shareTitle)

                    ContextAction.COPY_LINK_TEXT -> if (!Share.copy(
                            context,
                            "Text",
                            target.copyableText ?: "",
                        )
                    ) {
                        toast(copied)
                    }

                    ContextAction.DOWNLOAD_LINK -> {
                        val name = Downloads.enqueueUrl(context, link)
                        toast(if (name != null) downloadStarted.format(name) else downloadUnsupported)
                    }

                    ContextAction.OPEN_IMAGE_NEW_TAB -> {
                        vm.tabs.openTab(src, private = private, activate = false, opener = tabId)
                        toast(opened)
                    }

                    ContextAction.SAVE_IMAGE, ContextAction.SAVE_MEDIA -> {
                        val name = Downloads.enqueueUrl(context, src)
                        toast(if (name != null) downloadStarted.format(name) else downloadUnsupported)
                    }

                    ContextAction.COPY_IMAGE_LINK -> if (!Share.copy(context, "URL", src)) toast(copied)
                }
            },
        )
    }
}

@Composable
private fun ProgressLine(progress: Float) {
    LinearProgressIndicator(progress = { progress }, modifier = Modifier.fillMaxWidth().height(3.dp))
}

/**
 * The GeckoView of the active session. Gecko lays the page out with [toolbarMaxPx] reserved for
 * the toolbar and shows the reserved strip as the toolbar slides away (vertical clipping), so
 * the page never jumps while the toolbar hides.
 */
@Composable
private fun PageView(
    session: KrunkSession,
    fullScreen: Boolean,
    toolbarMaxPx: Float,
    visibleToolbarPx: Float,
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
            view.setVerticalClipping(if (fullScreen) 0 else -visibleToolbarPx.toInt())
        },
        onRelease = { view ->
            onView(null)
            view.releaseSession()
        },
    )
}
