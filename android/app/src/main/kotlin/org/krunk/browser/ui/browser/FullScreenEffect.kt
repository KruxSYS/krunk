package org.krunk.browser.ui.browser

import android.app.Activity
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.view.WindowManager
import androidx.activity.compose.LocalActivity
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalView
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.krunk.browser.ui.BrowserViewModel
import org.mozilla.geckoview.AllowOrDeny
import org.mozilla.geckoview.GeckoResult
import org.mozilla.geckoview.OrientationController

/**
 * While [fullScreen] is true the status and navigation bars are hidden (immersive sticky: a
 * swipe from the edge shows them briefly); leaving restores them. Also keeps the status bar
 * icons readable for the current theme when not in full screen.
 */
@Composable
fun SystemBarsEffect(
    fullScreen: Boolean,
    darkTheme: Boolean,
    darkStatusBar: Boolean = darkTheme,
    darkNavigationBar: Boolean = darkTheme,
) {
    val activity = LocalActivity.current
    val view = LocalView.current
    DisposableEffect(fullScreen, darkStatusBar, darkNavigationBar, activity) {
        val window = activity?.window
        val controller = window?.let { WindowCompat.getInsetsController(it, view) }
        if (controller != null) {
            controller.isAppearanceLightStatusBars = !darkStatusBar
            controller.isAppearanceLightNavigationBars = !darkNavigationBar
            if (fullScreen) {
                controller.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
                controller.hide(WindowInsetsCompat.Type.systemBars())
            } else {
                controller.show(WindowInsetsCompat.Type.systemBars())
            }
        }
        onDispose {
            if (fullScreen) controller?.show(WindowInsetsCompat.Type.systemBars())
        }
    }
    // Back from another app or the lock screen: the system may have brought the bars back.
    if (fullScreen) {
        LifecycleResumeEffect(activity, view) {
            activity?.window?.let { WindowCompat.getInsetsController(it, view) }?.let { controller ->
                controller.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
                controller.hide(WindowInsetsCompat.Type.systemBars())
            }
            onPauseOrDispose { }
        }
    }
}

/**
 * Everything of full screen that is not the browser screen's own layout (see [FullScreenPolicy]):
 *
 * - a tab that is in full screen without being the page on screen (another tab became active,
 *   another screen was opened) is taken out of it;
 * - while a page is shown full screen the screen stays on and the window uses the cutout area on
 *   the short edges (`enableEdgeToEdge` already asks for that on Android 9 and later; it is set
 *   here again only if something else changed it);
 * - pages may lock the orientation while shown full screen, and the orientation from before
 *   comes back when full screen ends for any reason ([PageOrientation]).
 *
 * The system bars are hidden by [SystemBarsEffect] of the browser screen.
 */
@Composable
fun FullScreenEffect(vm: BrowserViewModel, browserOnTop: Boolean) {
    val tabs by vm.tabs.tabs.collectAsStateWithLifecycle()
    val activeId by vm.tabs.activeId.collectAsStateWithLifecycle()
    val inFullScreen = remember(tabs) { tabs.filter { it.page.fullScreen }.map { it.id }.toSet() }
    val shown = FullScreenPolicy.shown(activeId, inFullScreen, browserOnTop)
    val leave = FullScreenPolicy.tabsToLeave(activeId, inFullScreen, browserOnTop)
    LaunchedEffect(leave) { leave.forEach { vm.tabs.session(it)?.leaveFullScreen() } }

    val activity = LocalActivity.current
    DisposableEffect(shown, activity) {
        val window = activity?.window
        var cutoutBefore: Int? = null
        if (shown && window != null) {
            window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                val mode = window.attributes.layoutInDisplayCutoutMode
                if (mode == WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_DEFAULT ||
                    mode == WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_NEVER
                ) {
                    cutoutBefore = mode
                    window.attributes = window.attributes.apply {
                        layoutInDisplayCutoutMode = WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
                    }
                }
            }
        }
        onDispose {
            if (shown && window != null) {
                window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                val before = cutoutBefore
                if (before != null && Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                    window.attributes = window.attributes.apply { layoutInDisplayCutoutMode = before }
                }
            }
        }
    }

    val orientation = remember(activity) { activity?.let { PageOrientation(it) } }
    DisposableEffect(orientation) {
        val controller = vm.engine.runtime.orientationController
        if (orientation != null) controller.delegate = orientation
        onDispose {
            if (orientation != null) {
                if (controller.delegate === orientation) controller.delegate = null
                orientation.release()
            }
        }
    }
    // Not in a coroutine: the lock state has to follow full screen in the same frame.
    SideEffect { orientation?.shownChanged(shown) }
}

/**
 * `screen.orientation.lock()` and `unlock()` of pages, on the activity (the rules are
 * [OrientationLock]'s). The engine calls on the main thread with an
 * `ActivityInfo.SCREEN_ORIENTATION_*` value (GeckoView 160 maps the web's values itself:
 * `GeckoRuntime.toAndroidOrientation`). The rotation that follows reaches the activity as a
 * configuration change, which it handles without being recreated.
 */
internal class PageOrientation(private val activity: Activity) : OrientationController.OrientationDelegate {
    private val lock = OrientationLock()
    private val handler = Handler(Looper.getMainLooper())
    private var pending: GeckoResult<AllowOrDeny>? = null
    private val expire = Runnable { if (lock.waitExpired()) settle(false) }

    override fun onOrientationLock(orientation: Int): GeckoResult<AllowOrDeny> {
        // One request at a time: an older one that still waits is refused.
        settle(false)
        return when (val answer = lock.lock(orientation, activity.requestedOrientation)) {
            is OrientationLock.Answer.Allow -> {
                activity.requestedOrientation = answer.orientation
                GeckoResult.allow()
            }

            OrientationLock.Answer.Wait -> GeckoResult<AllowOrDeny>().also {
                pending = it
                handler.postDelayed(expire, OrientationLock.WAIT_MS)
            }
        }
    }

    override fun onOrientationUnlock() {
        settle(false)
        lock.unlock()?.let { activity.requestedOrientation = it }
    }

    /** A page is shown full screen, or no longer. */
    fun shownChanged(shown: Boolean) {
        val outcome = lock.fullScreenChanged(shown, activity.requestedOrientation)
        outcome.apply?.let { activity.requestedOrientation = it }
        outcome.settle?.let(::settle)
    }

    /** The activity goes away: nothing stays locked or waiting. */
    fun release() {
        shownChanged(false)
        if (lock.waitExpired()) settle(false)
        handler.removeCallbacks(expire)
    }

    private fun settle(allow: Boolean) {
        handler.removeCallbacks(expire)
        val result = pending ?: return
        pending = null
        result.complete(if (allow) AllowOrDeny.ALLOW else AllowOrDeny.DENY)
    }
}

/**
 * While [secure] is true the window is marked secure: the system leaves it out of the Recents
 * thumbnail and refuses screenshots and screen recording. Used while a private tab (or the tab
 * switcher with private tabs) is on screen, so a private page is not kept by the system after
 * the tab is gone.
 */
@Composable
fun SecureWindowEffect(secure: Boolean) {
    val activity = LocalActivity.current
    DisposableEffect(secure, activity) {
        val window = activity?.window
        if (secure) {
            window?.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        } else {
            window?.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        }
        onDispose { }
    }
}
