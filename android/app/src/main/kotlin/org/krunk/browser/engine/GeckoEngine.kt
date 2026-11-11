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
import org.mozilla.geckoview.ContentBlocking
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
class GeckoEngine(context: Context, automaticUpdates: Boolean) {
    val runtime: GeckoRuntime

    init {
        val builder = GeckoRuntimeSettings.Builder()
            .consoleOutput(BuildConfig.DEBUG)
            .javaScriptEnabled(true)
            .remoteDebuggingEnabled(BuildConfig.DEBUG)
            .aboutConfigEnabled(BuildConfig.DEBUG)
            // Krunk's promise: nothing of the engine's own background traffic beyond what the
            // user triggers. The engine does not keep it by itself: see EngineConfig. Safe
            // Browsing is off (it means contact with Google); tracker blocking is the bundled
            // blocker's job, so the engine's own (whose lists come from Mozilla) is off; cookies
            // of third parties are kept apart per site, which needs no list.
            .contentBlocking(
                ContentBlocking.Settings.Builder()
                    .safeBrowsing(ContentBlocking.SafeBrowsing.NONE)
                    .antiTracking(ContentBlocking.AntiTracking.NONE)
                    .cookieBehavior(ContentBlocking.CookieBehavior.ACCEPT_FIRST_PARTY_AND_ISOLATE_OTHERS)
                    .cookieBehaviorPrivateMode(ContentBlocking.CookieBehavior.ACCEPT_FIRST_PARTY_AND_ISOLATE_OTHERS)
                    .build(),
            )
        try {
            val config = EngineConfig.write(context.applicationContext.filesDir, automaticUpdates)
            builder.configFilePath(config.absolutePath)
        } catch (e: java.io.IOException) {
            // Without the file the engine runs with its own defaults (background requests to
            // Mozilla and Google). The store on the same disk is failing too in that case.
            Log.e(KrunkApp.TAG, "engine config not written", e)
        }
        val settings = builder.build()
        runtime = GeckoRuntime.create(context.applicationContext, settings)
        Log.i(KrunkApp.TAG, "GeckoRuntime ready (GeckoView ${BuildConfig.GECKOVIEW_VERSION})")
    }

    val icons: IconsExtension by lazy { IconsExtension(runtime) }

    /**
     * The engine's own user agent. Krunk's few requests of its own (favicons, search suggestions)
     * carry it instead of the platform's default, which names the device model and build.
     */
    val defaultUserAgent: String by lazy { GeckoSession.getDefaultUserAgent() }

    /**
     * Pushes the user's settings into the runtime: HTTPS-only, the colour scheme web content
     * sees, text scaling (system font scale or the in-app factor), forced pinch zoom.
     */
    fun applySettings(settings: AndroidSettings) {
        userAgent = settings.userAgent.trim()
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

    /** Settings > Advanced > User agent ("" = the engine's own); sessions are created with it. */
    @Volatile
    private var userAgent: String = ""

    /** A session for a tab. Desktop mode is toggled later per tab. */
    fun createSession(private: Boolean): GeckoSession {
        val builder = GeckoSessionSettings.Builder()
            .usePrivateMode(private)
            .userAgentMode(GeckoSessionSettings.USER_AGENT_MODE_MOBILE)
            .viewportMode(GeckoSessionSettings.VIEWPORT_MODE_MOBILE)
        if (userAgent.isNotEmpty()) builder.userAgentOverride(userAgent)
        return GeckoSession(builder.build())
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
