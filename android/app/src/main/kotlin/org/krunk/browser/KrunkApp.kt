package org.krunk.browser

import android.app.Application
import android.os.SystemClock
import android.util.Log
import org.krunk.browser.core.KrunkCore
import org.krunk.browser.engine.GeckoEngine
import org.krunk.browser.tabs.TabManager
import org.krunk.ffi.coreVersion

/**
 * Process-wide state: the core store, the single Gecko runtime and the tab manager (Gecko
 * sessions outlive the activity, so tabs live here).
 */
class KrunkApp : Application() {
    /** Uptime at process start, for start-up timing in the log. */
    val startedAt: Long = SystemClock.uptimeMillis()

    val core: KrunkCore by lazy { KrunkCore(this) }

    val engine: GeckoEngine by lazy {
        GeckoEngine(this).also {
            it.applySettings(core.settings.current)
            Log.i(TAG, "engine ready after ${SystemClock.uptimeMillis() - startedAt} ms")
        }
    }

    private val tabsLazy = lazy { TabManager(this, core, engine) }
    val tabs: TabManager by tabsLazy

    override fun onCreate() {
        super.onCreate()
        Log.i(
            TAG,
            "Krunk ${BuildConfig.VERSION_NAME} starting (core ${coreVersion()}, GeckoView ${BuildConfig.GECKOVIEW_VERSION})",
        )
    }

    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        // Gecko's child processes run this Application too: never create the runtime from here.
        if (!tabsLazy.isInitialized()) return
        // Memory is short (foreground RUNNING_LOW/CRITICAL, or background MODERATE/COMPLETE):
        // free every background tab's session. UI_HIDDEN/BACKGROUND alone are not pressure.
        val pressure = level == TRIM_MEMORY_RUNNING_LOW || level == TRIM_MEMORY_RUNNING_CRITICAL ||
            level == TRIM_MEMORY_MODERATE || level == TRIM_MEMORY_COMPLETE
        if (pressure) tabs.onMemoryPressure()
    }

    @Deprecated("Deprecated in Java")
    override fun onLowMemory() {
        super.onLowMemory()
        // Android 14 and later no longer deliver the trim levels above to a running app; this
        // callback is what is left to hear about memory pressure there.
        if (tabsLazy.isInitialized()) tabs.onMemoryPressure()
    }

    companion object {
        const val TAG = "Krunk"
    }
}
