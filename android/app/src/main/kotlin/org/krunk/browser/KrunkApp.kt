package org.krunk.browser

import android.app.Application
import android.os.Build
import android.os.SystemClock
import android.util.Log
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import org.krunk.browser.core.KrunkCore
import org.krunk.browser.engine.GeckoEngine
import org.krunk.browser.engine.blocker.AllowList
import org.krunk.browser.engine.blocker.BlockerController
import org.krunk.browser.tabs.TabManager
import org.krunk.browser.vault.KeystoreKeyWrapper
import org.krunk.browser.vault.VaultBackend
import org.krunk.browser.vault.VaultBackends
import org.krunk.browser.vault.VaultController
import org.krunk.browser.vault.VaultLockSetting
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
        val settings = core.settings.current
        val host = object : BlockerController.Host {
            override fun settings() = core.settings.current
            override val allow = AllowList(core.store)
            override fun listsUpdated(at: Long) {
                runCatching { core.store.kvSet(AllowList.KV_LISTS_UPDATED, at.toString()) }
            }
        }
        GeckoEngine(this, automaticUpdates = settings.filterAutoUpdate, blockerHost = host).also {
            it.applySettings(settings)
            DebugHooks.install(this, it, core)
            Log.i(TAG, "engine ready after ${SystemClock.uptimeMillis() - startedAt} ms")
        }
    }

    private val tabsLazy = lazy { TabManager(this, core, engine) }
    val tabs: TabManager by tabsLazy

    /** When the vault locks by itself: timer and background grace (see [VaultLockSetting]). */
    val vaultLock: VaultLockSetting by lazy { VaultLockSetting(core.store::kvGet, core.store::kvSet) }

    /** For what must read vault data while it is locked (the biometric blob); else use [vault]. */
    val vaultBackend: VaultBackend by lazy { VaultBackends.create(this) }

    private val vaultLazy = lazy {
        val wrapKey = KeystoreKeyWrapper.onDevice()
        VaultController(
            backend = vaultBackend,
            keyPresent = wrapKey::keyPresent,
            prepareForNewVault = wrapKey::prepareForNewVault,
            deleteKey = wrapKey::deleteKey,
            clock = System::currentTimeMillis,
            dispatcher = Dispatchers.IO,
            lockMinutes = { vaultLock.current },
            backgroundGraceSeconds = { vaultLock.currentGraceSeconds },
            timerScope = CoroutineScope(SupervisorJob() + Dispatchers.Default),
        )
    }
    val vault: VaultController by vaultLazy

    override fun onCreate() {
        super.onCreate()
        // Gecko's child processes run this class too: they must not load the Rust library just
        // to print a version.
        val mainProcess = Build.VERSION.SDK_INT < Build.VERSION_CODES.P || getProcessName() == packageName
        if (mainProcess) {
            Log.i(
                TAG,
                "Krunk ${BuildConfig.VERSION_NAME} starting (core ${coreVersion()}, " +
                    "GeckoView ${BuildConfig.GECKOVIEW_VERSION})",
            )
            // The vault locks once no activity of Krunk has been on screen for the grace period.
            // A vault nobody has asked for yet is locked anyway and is not created for this.
            ProcessLifecycleOwner.get().lifecycle.addObserver(
                object : DefaultLifecycleObserver {
                    override fun onStart(owner: LifecycleOwner) {
                        if (vaultLazy.isInitialized()) vault.onForeground()
                    }

                    override fun onStop(owner: LifecycleOwner) {
                        if (vaultLazy.isInitialized()) vault.onBackground()
                    }
                },
            )
        }
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
