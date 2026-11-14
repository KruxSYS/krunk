package org.krunk.browser.update

import android.content.Context
import androidx.compose.runtime.Composable

/**
 * The Play build: Google Play updates the app, so there is no update check, no update screen, no
 * request to Krunk's server and no installer (and no REQUEST_INSTALL_PACKAGES permission). The
 * direct flavour's version of this object has the website APK's update check.
 */
object UpdateChannel {
    const val AVAILABLE = false

    @Suppress("UNUSED_PARAMETER")
    fun onAppStart(context: Context) = Unit

    @Composable
    fun AboutSection() = Unit
}
