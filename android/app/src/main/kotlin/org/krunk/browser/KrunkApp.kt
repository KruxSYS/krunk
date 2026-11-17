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
import kotlinx.coroutines.launch
import org.krunk.browser.core.KrunkCore
import org.krunk.browser.downloads.DownloadController
import org.krunk.browser.engine.EngineConfig
import org.krunk.browser.engine.ErrorPage
import org.krunk.browser.engine.GeckoEngine
import org.krunk.browser.engine.LoadSource
import org.krunk.browser.engine.VpnLoadGuard
import org.krunk.browser.engine.blocker.AllowList
import org.krunk.browser.engine.blocker.BlockerController
import org.krunk.browser.engine.blocker.VpnGuardConfig
import org.krunk.browser.engine.blocker.VpnGuardScope
import org.krunk.browser.engine.permissions.SitePermissions
import org.krunk.browser.notify.SiteNotifications
import org.krunk.browser.sync.SyncController
import org.krunk.browser.sync.SyncHttp
import org.krunk.browser.tabs.TabManager
import org.krunk.browser.update.UpdateChannel
import org.krunk.browser.vault.AppLoginVault
import org.krunk.browser.vault.FfiKeyWrapper
import org.krunk.browser.vault.FfiVaultBackend
import org.krunk.browser.vault.KeystoreKeyWrapper
import org.krunk.browser.vault.VaultBackend
import org.krunk.browser.vault.VaultBackends
import org.krunk.browser.vault.VaultController
import org.krunk.browser.vault.VaultLockSetting
import org.krunk.browser.vault.VaultState
import org.krunk.browser.vpn.FfiVpnEngine
import org.krunk.browser.vpn.VpnController
import org.krunk.browser.vpn.VpnGuardSink
import org.krunk.browser.vpn.VpnNetworkWatch
import org.krunk.browser.vpn.VpnPhase
import org.krunk.browser.vpn.VpnScope
import org.krunk.browser.vpn.VpnSettingsFile
import org.krunk.browser.vpn.VpnStore
import org.krunk.ffi.KrunkSync
import org.krunk.ffi.coreVersion
import org.krunk.ffi.webOrigin

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

        // The VPN (0.10.0) starts before the engine when "Connect when Krunk starts" is on, so its
        // port is known for the engine's start-up prefs; on and not up: the engine starts closed
        // (port null: the backstop points at a closed port and pages wait for the guard's filter).
        // Connects now when "Connect when Krunk starts" is set; the engine gets the backstop prefs
        // either way while the VPN is on, and the guard gets the port over its native port.
        vpn.startAtLaunch()
        val vpnPrefs = if (vpn.isOn()) EngineConfig.vpnPrefs() else emptyList()
        GeckoEngine(this, settings.filterAutoUpdate, host, vpnPrefs).also {
            engineMade = it
            it.applySettings(settings)
            DebugHooks.install(this, it, core)
            // From here on pages may be offered saved logins: the engine side asks this vault.
            it.logins.vault = loginVault
            // Site permissions and site notifications (0.4.0).
            it.permissions = sitePermissions
            it.runtime.webNotificationDelegate = siteNotifications
            vaultScope.launch {
                vault.state.collect { _ -> it.logins.onVaultState(loginVault.state(), System.currentTimeMillis()) }
            }
            // The guard (made with the engine) gets the VPN's current values; the error page
            // learns whether a failed load is the VPN's doing.
            vpn.resendGuard()
            // "Connected" for the page means usable: the tunnel is up AND the engine is routed
            // through it (krunk-guard's filter installed).
            val engine = it
            val filterInstalled = { engine.blocker.vpnFilterInstalled.value }
            VpnLoadGuard.blocking = { privateTab ->
                vpn.isOn() && !filterInstalled() &&
                    (vpn.state.value.settings.scope == VpnScope.ALL || privateTab)
            }
            ErrorPage.vpnSource = object : ErrorPage.VpnSource {
                override val vpnOn get() = vpn.isOn()
                override val connected get() = vpn.state.value.phase == VpnPhase.CONNECTED && filterInstalled()
                override fun refusedRecently(host: String) = vpn.refusedRecently(host)
            }
            Log.i(TAG, "engine ready after ${SystemClock.uptimeMillis() - startedAt} ms")
        }
    }

    /** The engine once made; the VPN's guard values must never be the reason it is made. */
    @Volatile
    private var engineMade: GeckoEngine? = null

    /**
     * Where the VPN's proxy values go: `krunk-guard`'s filter (contract §3), on the main thread
     * and in order. Before the engine exists nothing is sent: it starts with [EngineConfig.vpnPrefs]
     * and gets the values right after ([VpnController.resendGuard]).
     */
    private val vpnGuard = VpnGuardSink { on, _, port, pass, scope, lan ->
        val config = when {
            !on -> null

            else -> {
                val s = if (scope == "private") VpnGuardScope.PRIVATE else VpnGuardScope.ALL
                if (port != null && pass != null) {
                    VpnGuardConfig.connected(port, pass, s, lan)
                } else {
                    VpnGuardConfig.pending(s, lan)
                }
            }
        }
        vaultScope.launch { engineMade?.blocker?.setVpn(config) }
    }

    private val vpnLazy = lazy {
        VpnController(
            engine = FfiVpnEngine(),
            store = VpnStore.onDevice(filesDir),
            settingsFile = VpnSettingsFile(VpnStore.dir(filesDir)),
            io = Dispatchers.IO,
            guard = vpnGuard,
            log = { Log.i(TAG, it) },
        ).also { c ->
            c.load()
            // Wi-Fi <-> mobile, new addresses: the tunnel rebinds and handshakes at once.
            VpnNetworkWatch(c::networkChanged).register(this)
        }
    }

    /** The in-browser VPN (0.10.0); made with the engine (main process only). */
    val vpn: VpnController by vpnLazy

    private val tabsLazy = lazy { TabManager(this, core, engine).also(::startSiteFeatures) }
    val tabs: TabManager by tabsLazy

    /** Krunk's own downloads (0.4.0): questions, transfers, the list, notifications. */
    val downloads: DownloadController by lazy { DownloadController(this) }

    /** What sites may do (camera, location, notifications, ...): prompts, answers, Android's permissions. */
    val sitePermissions: SitePermissions by lazy {
        SitePermissions(this, core.store) { engine.runtime.storageController }
    }

    private val siteNotificationsLazy = lazy {
        SiteNotifications(this, sitePermissions::notificationsPostable, sitePermissions::notificationDropped)
    }

    /** Notifications of sites, shown as Android notifications. */
    val siteNotifications: SiteNotifications by siteNotificationsLazy

    /** [siteNotifications] when the engine runs; a broadcast must not start it. */
    val siteNotificationsIfRunning: SiteNotifications?
        get() = if (siteNotificationsLazy.isInitialized()) siteNotifications else null

    /**
     * Once the engine and the tabs exist: the engine's copy of the permission answers is checked
     * against Krunk's, and the end of the private session is watched for (the last private tab
     * closes: its answers and notifications go).
     */
    private fun startSiteFeatures(tabs: TabManager) {
        sitePermissions.reconcile()
        // The answers are read once, off the main thread; requests are answered from memory.
        vaultScope.launch(Dispatchers.IO) { sitePermissions.warm() }
        vaultScope.launch {
            var hadPrivate = false
            tabs.tabs.collect { list ->
                val hasPrivate = list.any { it.private }
                if (hadPrivate && !hasPrivate) {
                    sitePermissions.privateSessionEnded()
                    siteNotificationsIfRunning?.privateSessionEnded()
                }
                hadPrivate = hasPrivate
            }
        }
    }

    /**
     * A site notification was tapped: the page hears of it and a tab of that site comes to the
     * front (the site is opened in a new tab when none is left; never for a private page).
     */
    fun siteNotificationTapped(id: Int, token: String?, origin: String?) {
        val tap = siteNotifications.tapped(id, token, origin)
        val site = tap.origin ?: return
        val all = tabs.tabs.value
        val active = tabs.activeId.value
        val tab = all.firstOrNull { it.id == active && it.private == tap.private && webOrigin(it.url) == site }
            ?: all.firstOrNull { it.private == tap.private && webOrigin(it.url) == site }
        when {
            tab != null -> tabs.activate(tab.id)
            !tap.private -> tabs.openUntrusted("$site/", LoadSource.EXTERNAL_APP, activate = true, live = true)
        }
    }

    /** When the vault locks by itself: timer and background grace (see [VaultLockSetting]). */
    val vaultLock: VaultLockSetting by lazy { VaultLockSetting(core.store::kvGet, core.store::kvSet) }

    /** For what must read vault data while it is locked (the biometric blob); else use [vault]. */
    val vaultBackend: VaultBackend by lazy { VaultBackends.create(this) }

    private val vaultScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

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

    /** The vault as the engine side uses it (fill and save prompts); set on the engine below. */
    val loginVault: AppLoginVault by lazy { AppLoginVault(vault, core.store, System::currentTimeMillis) }

    /**
     * Krunk account and sync (0.6.0). Made on first use; making it reads local files only, and
     * nothing goes to the network before the user signs in.
     */
    private val syncLazy = lazy {
        val http = SyncHttp(
            base = BuildConfig.SYNC_BASE_URL,
            allowHttp = BuildConfig.SYNC_ALLOW_HTTP,
            userAgent = "Krunk/${BuildConfig.VERSION_NAME} (Android)",
            log = BuildConfig.SYNC_LOG,
        )
        val core = KrunkSync(
            dir = SyncController.dir(filesDir).absolutePath,
            db = this.core.databaseFile.absolutePath,
            transport = http,
            wrapper = FfiKeyWrapper(KeystoreKeyWrapper.forSync()),
            vault = (vaultBackend as? FfiVaultBackend)?.ffi,
            label = "Krunk on Android ${Build.VERSION.RELEASE}",
            version = BuildConfig.VERSION_NAME,
            verifyBase = BuildConfig.SYNC_VERIFY_BASE.ifEmpty { null },
        )
        SyncController(core, vaultScope, Dispatchers.IO, System::currentTimeMillis) { changed ->
            vaultScope.launch {
                if (changed.settings) {
                    this@KrunkApp.core.settings.reload()
                    if (tabsLazy.isInitialized()) engine.applySettings(this@KrunkApp.core.settings.current)
                }
                if (changed.history) this@KrunkApp.core.historySynced()
                if (changed.allowlist && tabsLazy.isInitialized()) engine.blocker.allowListChanged()
                if ((changed.passwords || changed.neverSave) && vaultLazy.isInitialized()) vault.refresh()
            }
        }.also { c ->
            vaultScope.launch {
                vault.state.collect { if (it == VaultState.UNLOCKED) c.vaultUnlocked() }
            }
        }
    }
    val sync: SyncController by syncLazy

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
            // A private page's notification an earlier process left in the shade goes now,
            // whatever this start is for (the engine and its notifications may not be made).
            SiteNotifications.sweepPrivate(this)
            // A downloaded update APK an earlier run left behind is deleted (direct flavour; no
            // network: the update check runs only when the user taps it).
            UpdateChannel.onAppStart(this)
            // The vault locks once no activity of Krunk has been on screen for the grace period.
            // A vault nobody has asked for yet is locked anyway and is not created for this.
            ProcessLifecycleOwner.get().lifecycle.addObserver(
                object : DefaultLifecycleObserver {
                    override fun onStart(owner: LifecycleOwner) {
                        if (vaultLazy.isInitialized()) vault.onForeground()
                        // Sync's schedule runs only while Krunk is on screen (no request before
                        // sign-in: the schedule only reads local state until then).
                        sync.onForeground()
                        // A VPN that failed or is reconnecting tries again now (plan §6.1).
                        if (vpnLazy.isInitialized()) vaultScope.launch { vpn.onForeground() }
                    }

                    override fun onStop(owner: LifecycleOwner) {
                        if (vaultLazy.isInitialized()) vault.onBackground()
                        if (syncLazy.isInitialized()) sync.onBackground()
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
