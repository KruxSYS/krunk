package org.krunk.browser.engine

import android.content.Context
import android.util.Log
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import kotlinx.coroutines.suspendCancellableCoroutine
import org.krunk.browser.BuildConfig
import org.krunk.browser.KrunkApp
import org.krunk.ffi.AndroidSettings
import org.krunk.ffi.ThemeMode
import org.mozilla.geckoview.GeckoResult
import org.mozilla.geckoview.GeckoRuntime
import org.mozilla.geckoview.GeckoRuntimeSettings
import org.mozilla.geckoview.GeckoSession
import org.mozilla.geckoview.GeckoSessionSettings
import org.mozilla.geckoview.StorageController

/**
 * The one [GeckoRuntime] of the process and the settings Krunk applies to it. Private tabs use
 * the same runtime with `usePrivateMode` sessions (Gecko keeps their data in memory only).
 */
class GeckoEngine(context: Context) {
    val runtime: GeckoRuntime

    init {
        val settings = GeckoRuntimeSettings.Builder()
            .consoleOutput(BuildConfig.DEBUG)
            .javaScriptEnabled(true)
            .remoteDebuggingEnabled(BuildConfig.DEBUG)
            .aboutConfigEnabled(BuildConfig.DEBUG)
            // Krunk's promise: nothing of the engine's own background traffic beyond what the
            // user triggers. Gecko has no telemetry in GeckoView builds; crash reports stay local.
            .build()
        runtime = GeckoRuntime.create(context.applicationContext, settings)
        Log.i(KrunkApp.TAG, "GeckoRuntime ready (GeckoView ${BuildConfig.GECKOVIEW_VERSION})")
    }

    val icons: IconsExtension by lazy { IconsExtension(runtime) }

    /**
     * Pushes the user's settings into the runtime: HTTPS-only, the colour scheme web content
     * sees, text scaling (system font scale or the in-app factor), forced pinch zoom.
     */
    fun applySettings(settings: AndroidSettings) {
        val rs = runtime.settings
        rs.allowInsecureConnections =
            if (settings.httpsOnly) GeckoRuntimeSettings.HTTPS_ONLY else GeckoRuntimeSettings.ALLOW_ALL
        rs.preferredColorScheme = when (settings.theme) {
            ThemeMode.SYSTEM -> GeckoRuntimeSettings.COLOR_SCHEME_SYSTEM
            ThemeMode.LIGHT -> GeckoRuntimeSettings.COLOR_SCHEME_LIGHT
            ThemeMode.DARK -> GeckoRuntimeSettings.COLOR_SCHEME_DARK
        }
        if (settings.followSystemFont) {
            rs.automaticFontSizeAdjustment = true
        } else {
            rs.automaticFontSizeAdjustment = false
            rs.fontSizeFactor = (settings.textScale * settings.defaultZoom).toFloat().coerceIn(0.5f, 3f)
        }
        rs.forceUserScalableEnabled = settings.forceZoom
    }

    /** A session for a tab. Desktop mode is toggled later per tab. */
    fun createSession(private: Boolean): GeckoSession {
        val s = GeckoSessionSettings.Builder()
            .usePrivateMode(private)
            .userAgentMode(GeckoSessionSettings.USER_AGENT_MODE_MOBILE)
            .viewportMode(GeckoSessionSettings.VIEWPORT_MODE_MOBILE)
            .build()
        return GeckoSession(s)
    }

    /** Clears site data. Callers close sessions first so nothing re-accumulates. */
    fun clearData(cookies: Boolean, cache: Boolean, siteData: Boolean): GeckoResult<Void> {
        var flags = 0L
        if (cookies) flags = flags or StorageController.ClearFlags.COOKIES
        if (cache) flags = flags or StorageController.ClearFlags.ALL_CACHES
        if (siteData) {
            flags =
                flags or StorageController.ClearFlags.SITE_DATA or StorageController.ClearFlags.PERMISSIONS
        }
        return if (flags == 0L) GeckoResult.fromValue(null) else runtime.storageController.clearData(flags)
    }
}

/** Waits for a [GeckoResult] (call on the main thread, where the engine delivers it). */
suspend fun <T> GeckoResult<T>.await(): T? = suspendCancellableCoroutine { cont ->
    accept(
        { value -> if (cont.isActive) cont.resume(value) },
        { error ->
            if (cont.isActive) cont.resumeWithException(error ?: IllegalStateException("engine request failed"))
        },
    )
}
