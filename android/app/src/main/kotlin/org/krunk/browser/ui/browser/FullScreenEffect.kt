package org.krunk.browser.ui.browser

import android.view.WindowManager
import androidx.activity.compose.LocalActivity
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.platform.LocalView
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

/**
 * While [fullScreen] is true the status and navigation bars are hidden (immersive sticky: a
 * swipe from the edge shows them briefly); leaving restores them. Also keeps the status bar
 * icons readable for the current theme when not in full screen.
 */
@Composable
fun SystemBarsEffect(fullScreen: Boolean, darkTheme: Boolean, darkStatusBar: Boolean = darkTheme) {
    val activity = LocalActivity.current
    val view = LocalView.current
    DisposableEffect(fullScreen, darkTheme, darkStatusBar, activity) {
        val window = activity?.window
        val controller = window?.let { WindowCompat.getInsetsController(it, view) }
        if (controller != null) {
            controller.isAppearanceLightStatusBars = !darkStatusBar
            controller.isAppearanceLightNavigationBars = !darkTheme
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
