package org.krunk.browser.engine

import android.content.Context
import android.util.Log
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import kotlinx.coroutines.suspendCancellableCoroutine
import org.krunk.browser.BuildConfig
import org.krunk.browser.KrunkApp
import org.krunk.browser.engine.blocker.BlockerController
import org.krunk.browser.engine.logins.LoginsHub
import org.krunk.browser.engine.permissions.SitePermissions
import org.krunk.ffi.AndroidSettings
import org.krunk.ffi.ThemeMode
import org.krunk.ffi.siteIsRegistrable
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
class GeckoEngine(
    context: Context,
    automaticUpdates: Boolean,
    blockerHost: BlockerController.Host,
    /** [EngineConfig.vpnPrefs] when Krunk's VPN is on at this start, else empty. */
    vpnPrefs: List<Pair<String, Any>> = emptyList(),
) {
    val runtime: GeckoRuntime

    /** The ad and tracker blocker; started with the runtime, before any tab exists. */
    val blocker: BlockerController

    /** Saved logins as the engine may see them (placeholders only), and the vault behind them. */
    val logins = LoginsHub()

    /** Copies of the files pages are given through file fields. */
    val uploads = Uploads(context)

    /** Site permissions: every session gets its delegate (set once the store exists, see KrunkApp). */
    var permissions: SitePermissions? = null

    /** Signs Krunk's own website in while the app is signed in to the account (set by KrunkApp). */
    var webSignIn: WebSignIn? = null

    /** For the engine calls that want one (handing a picked file to a page). */
    val appContext: Context = context.applicationContext

    init {
        uploads.clearAll()
        val builder = GeckoRuntimeSettings.Builder()
            .consoleOutput(BuildConfig.DEBUG)
            .javaScriptEnabled(true)
            .remoteDebuggingEnabled(BuildConfig.DEBUG)
            .aboutConfigEnabled(BuildConfig.DEBUG)
            // The engine never fills a sign-in form by itself: every fill goes through the
            // tab's own prompt, where Krunk knows the tab (see LoginsHub).
            .loginAutofillEnabled(false)
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
        // Before the engine starts: nothing of an earlier Remote Settings sync may be left for it
        // to verify against Mozilla's servers (see EngineConfig). Once per change, not per start.
        val files = context.applicationContext.filesDir
        try {
            if (EngineConfig.mustDropRemoteSettingsStore(EngineConfig.lastStart(files), automaticUpdates)) {
                val dropped = EngineConfig.dropRemoteSettingsStore(files)
                if (dropped >
                    0
                ) {
                    Log.i(KrunkApp.TAG, "engine: removed $dropped files of an earlier Remote Settings sync")
                }
            }
            EngineConfig.rememberStart(files, automaticUpdates)
        } catch (e: java.io.IOException) {
            Log.w(KrunkApp.TAG, "engine: Remote Settings store not checked: ${e.javaClass.simpleName}")
        } catch (e: SecurityException) {
            Log.w(KrunkApp.TAG, "engine: Remote Settings store not checked: ${e.javaClass.simpleName}")
        }
        try {
            val config = EngineConfig.write(
                context.applicationContext.filesDir,
                automaticUpdates,
                extraPrefs = vpnPrefs,
            )
            builder.configFilePath(config.absolutePath)
        } catch (e: java.io.IOException) {
            // Without the file the engine runs with its own defaults (background requests to
            // Mozilla and Google). The store on the same disk is failing too in that case.
            Log.e(KrunkApp.TAG, "engine config not written", e)
        }
        val settings = builder.build()
        runtime = GeckoRuntime.create(context.applicationContext, settings)
        Log.i(KrunkApp.TAG, "GeckoRuntime ready (GeckoView ${BuildConfig.GECKOVIEW_VERSION})")
        runtime.autocompleteStorageDelegate = logins.storage
        blocker = BlockerController(runtime, blockerHost, vpnAtStart = vpnPrefs.isNotEmpty())
        blocker.start()
    }

    val icons: IconsExtension by lazy { IconsExtension(runtime) }

    /** Lists the sites that hold cookies or stored data; installed when first asked. */
    val siteData: SiteDataExtension by lazy { SiteDataExtension(runtime) }

    /** Certificate exceptions ("Continue anyway"), through the same extension. */
    val certs: CertOverrides by lazy { CertOverrides(siteData) }

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
        blocker.settingsChanged(settings)
    }

    /** Settings > Advanced > User agent ("" = the engine's own); sessions are created with it. */
    @Volatile
    private var userAgent: String = ""

    /**
     * A session for a tab. [desktop]: the tab asks for the desktop site, so the session has the
     * desktop user agent and viewport from the start (the user can still switch it per tab).
     */
    fun createSession(private: Boolean, desktop: Boolean = false): GeckoSession {
        val builder = GeckoSessionSettings.Builder()
            .usePrivateMode(private)
            .userAgentMode(GeckoSessionSettings.USER_AGENT_MODE_MOBILE)
            .viewportMode(GeckoSessionSettings.VIEWPORT_MODE_MOBILE)
        if (desktop) {
            builder.userAgentMode(GeckoSessionSettings.USER_AGENT_MODE_DESKTOP)
            builder.viewportMode(GeckoSessionSettings.VIEWPORT_MODE_DESKTOP)
        }
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

    /**
     * Clears what one site (a registrable domain, an IP address or `localhost`) holds: cookies
     * and sign-ins, stored data, cached files; with [withPermissions] also the engine's
     * permission answers for it. The engine clears the site's own data in every tab kind and
     * what third parties stored while embedded in it. History is Krunk's and is not touched.
     *
     * A "site" that is a public suffix itself or a single-label host (`github.io`, `dev`,
     * `localhost`: the row of a host that has no registrable domain) is cleared by its exact
     * host: the base-domain call matches cookies by root domain (`ClearDataService`'s
     * `hasRootDomain`) and would sign the user out of every site under that suffix. The
     * host call runs the same cleaners (the few without a per-host variant clear everything in
     * both calls: media device ids, HTTP auth, client-auth tokens).
     */
    fun clearSiteData(site: String, withPermissions: Boolean): GeckoResult<Void> {
        var flags = StorageController.ClearFlags.COOKIES or StorageController.ClearFlags.DOM_STORAGES or
            StorageController.ClearFlags.ALL_CACHES or StorageController.ClearFlags.AUTH_SESSIONS
        if (withPermissions) flags = flags or StorageController.ClearFlags.PERMISSIONS
        return if (siteIsRegistrable(site)) {
            runtime.storageController.clearDataFromBaseDomain(site, flags)
        } else {
            runtime.storageController.clearDataFromHost(site, flags)
        }
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
