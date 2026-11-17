package org.krunk.browser.vpn

import java.security.SecureRandom
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext

/**
 * Where the VPN's proxy settings go for the engine: `krunk-guard`'s filter through
 * `BlockerController.setVpn` (M2, contract §3). Called whenever one of the values changes, on
 * any thread. [passHex] is the session token: never log it.
 */
fun interface VpnGuardSink {
    fun setVpn(on: Boolean, pending: Boolean, port: Int?, passHex: String?, scope: String, lan: Boolean)
}

enum class VpnPhase { OFF, CONNECTING, CONNECTED, FAILED }

/** What the VPN screen and the address bar show (contract §5). */
data class VpnUiState(
    val settings: VpnSettings = VpnSettings(),
    val profiles: List<VpnProfile> = emptyList(),
    val status: VpnStatus? = null,
    val phase: VpnPhase = VpnPhase.OFF,
)

/** A checked profile file, not saved yet ([VpnController.importFile]). */
sealed class ImportResult {
    /** [toString] never shows the file. */
    class Ok(val fileName: String, val bytes: ByteArray, val info: VpnProfileInfo) : ImportResult() {
        val warnings: List<String> get() = info.warnings

        override fun toString() = "ImportResult.Ok(${info.kind}, ${bytes.size} bytes)"
    }

    data class Failed(val message: String) : ImportResult()
}

/**
 * The in-browser VPN (plan §6): the runtime runs in this process, no foreground service.
 *
 * - **On** ([VpnSettings.enabled]): Krunk's traffic in the scope goes through the VPN's proxy
 *   or nowhere. While the proxy is not up (connecting, failed, missing credentials) the guard
 *   holds the engine's requests ("pending") and [VpnRoute] refuses Krunk's own.
 * - **Off**: everything goes direct, as before 0.10.0. "Disconnect" turns it off (Windows).
 * - Every start gets a new 32-byte token from [random]; its hex form goes only to the guard
 *   ([VpnGuardSink]) and to [VpnRoute]. The previous one is wiped.
 * - At process start ([startAtLaunch]) a VPN left on connects only with "Connect when Krunk
 *   starts"; without it the start is direct (Windows `init_vpn`).
 *
 * Profile text, credentials and the token are never logged; [log] gets states and errors only.
 */
class VpnController(
    private val engine: VpnEngine,
    private val store: VpnStore,
    private val settingsFile: VpnSettingsFile,
    private val io: CoroutineDispatcher,
    private val guard: VpnGuardSink,
    private val clock: () -> Long = System::currentTimeMillis,
    private val random: SecureRandom = SecureRandom(),
    private val log: (String) -> Unit = {},
) {
    private val lock = Any()
    private var settings = VpnSettings()
    private var profiles: List<VpnProfile> = emptyList()
    private var status: VpnStatus? = null
    private var port: Int? = null
    private var token: ByteArray? = null
    private var tokenHex: String? = null

    /** Bumped at every start and stop: status reports of an older run are dropped. */
    @Volatile
    private var generation = 0
    private var lastGuard: List<Any?>? = null

    private val _state = MutableStateFlow(VpnUiState())
    val state: StateFlow<VpnUiState> = _state.asStateFlow()

    /** The proxy port while the VPN's proxy is up. */
    fun proxyPort(): Int? = synchronized(lock) { port }

    /** The session token as 64 hex chars (for `BlockerController` and [VpnRoute] only). */
    fun token(): String? = synchronized(lock) { tokenHex }

    /** True while the VPN is on (for the engine's start-up prefs, with [proxyPort]). */
    fun isOn(): Boolean = synchronized(lock) { settings.enabled }

    /** Reads settings and the profile list (blocking; files and the key store). */
    fun load() = synchronized(lock) {
        settings = settingsFile.read()
        profiles = readProfiles()
        if (settings.selectedProfileId != null && profiles.none { it.id == settings.selectedProfileId }) {
            settings = settings.copy(selectedProfileId = null)
        }
        publish()
    }

    /**
     * At process start, before the engine is made (blocking): a VPN left on connects now when
     * "Connect when Krunk starts" is set, so the port is known for the engine's prefs; without
     * it the VPN is turned off. Returns the port, or null (VPN off, or on and not up: then the
     * engine must start fail-closed).
     */
    fun startAtLaunch(): Int? = synchronized(lock) {
        load()
        if (!settings.enabled) return null
        if (!settings.connectAtStart) {
            settings = settings.copy(enabled = false)
            saveSettings()
            publish()
            return null
        }
        log("vpn: connecting at start")
        startLocked()
        port
    }

    suspend fun importFile(fileName: String, bytes: ByteArray): ImportResult = withContext(io) {
        if (bytes.isEmpty() || bytes.size > VpnStore.MAX_PROFILE) {
            return@withContext ImportResult.Failed("This file is not a VPN profile (empty or larger than 64 KB)")
        }
        try {
            ImportResult.Ok(fileName, bytes.copyOf(), engine.importProfile(fileName, bytes))
        } catch (e: VpnEngineException) {
            ImportResult.Failed(e.message ?: "This file is not a VPN profile")
        }
    }

    /** Saves an imported profile; the first one is selected. */
    suspend fun save(import: ImportResult.Ok, creds: VpnCredentials?): Result<VpnProfile> = locked {
        val profile = store.add(import.fileName, import.bytes, import.info, creds)
        import.bytes.fill(0)
        profiles = readProfiles()
        if (settings.selectedProfileId == null) {
            settings = settings.copy(selectedProfileId = profile.id)
            saveSettings()
        }
        publish()
        profile
    }

    suspend fun setCredentials(id: String, creds: VpnCredentials?): Result<Unit> = locked {
        store.setCredentials(id, creds)
    }

    suspend fun remove(id: String): Result<Unit> = locked {
        if (settings.selectedProfileId == id) {
            if (settings.enabled) stopLocked()
            settings = settings.copy(selectedProfileId = null, enabled = false)
            saveSettings()
        }
        store.remove(id)
        profiles = readProfiles()
        publish()
    }

    suspend fun setEnabled(on: Boolean) {
        if (on) connect() else disconnect()
    }

    /** Turns the VPN on with [profileId] (or the selected profile) and starts it; also "retry". */
    suspend fun connect(profileId: String? = null): Result<Unit> = locked {
        if (profileId != null && VpnStore.validId(profileId)) settings = settings.copy(selectedProfileId = profileId)
        settings = settings.copy(enabled = true)
        saveSettings()
        startLocked()
    }

    /** Turns the VPN off: everything goes direct again. */
    suspend fun disconnect(): Result<Unit> = locked {
        settings = settings.copy(enabled = false)
        saveSettings()
        stopLocked()
        log("vpn: off")
    }

    suspend fun setScope(scope: VpnScope) = change { it.copy(scope = scope) }

    suspend fun setLanDirect(on: Boolean) = change { it.copy(lanDirect = on) }

    suspend fun setConnectAtStart(on: Boolean) = change { it.copy(connectAtStart = on) }

    suspend fun setShowInAddressBar(on: Boolean) = change { it.copy(showInAddressBar = on) }

    /** "Not reachable through this VPN" for the error page (contract §2). */
    fun refusedRecently(host: String): Boolean = proxyPort() != null && engine.refusedRecently(host)

    /** The default network or its link properties changed (plan §6.2). */
    fun networkChanged() {
        if (proxyPort() != null) engine.networkChanged()
    }

    /**
     * Krunk came to the foreground: a VPN that is on and failed (or never came up) starts
     * again; one that is reconnecting is told to try now (its back-off is reset).
     */
    suspend fun onForeground() {
        val (on, st, up) = synchronized(lock) { Triple(settings.enabled, status?.state, port != null) }
        if (!on) return
        when {
            st == VpnState.RECONNECTING && up -> engine.networkChanged()
            st == VpnState.FAILED || st == VpnState.STOPPED || !up -> connect()
        }
    }

    /** Sends the current guard values again (the engine was just made and missed them). */
    fun resendGuard() = synchronized(lock) {
        lastGuard = null
        publish()
    }

    private suspend fun change(update: (VpnSettings) -> VpnSettings): Result<Unit> = locked {
        settings = update(settings)
        saveSettings()
        publish()
    }

    private suspend fun <T> locked(block: () -> T): Result<T> = withContext(io) {
        synchronized(lock) {
            try {
                Result.success(block())
            } catch (e: java.io.IOException) {
                log("vpn: ${e.javaClass.simpleName}: ${e.message}")
                Result.failure(e)
            }
        }
    }

    private fun readProfiles(): List<VpnProfile> = try {
        store.list()
    } catch (e: VpnStoreException) {
        log("vpn: profiles unreadable: ${e.message}")
        emptyList()
    }

    private fun saveSettings() {
        try {
            settingsFile.write(settings)
        } catch (_: java.io.IOException) {
            log("vpn: settings not saved")
        }
    }

    /** Starts the selected profile; on any failure the VPN stays on without a proxy (fail-closed). */
    private fun startLocked() {
        val id = settings.selectedProfileId ?: profiles.singleOrNull()?.id
        val profile = profiles.firstOrNull { it.id == id }
        if (profile == null) {
            fail("Choose a VPN profile")
            return
        }
        if (settings.selectedProfileId != profile.id) {
            settings = settings.copy(selectedProfileId = profile.id)
            saveSettings()
        }
        val (bytes, creds) = try {
            store.read(profile.id)
        } catch (_: VpnStoreException) {
            fail("The stored profile could not be opened. Import it again.")
            return
        }
        if (profile.needsCredentials && creds == null) {
            bytes.fill(0)
            fail("Enter the user name and password for this profile first")
            return
        }
        val fresh = ByteArray(32).also(random::nextBytes)
        val gen = ++generation
        val newPort = try {
            engine.start(profile.fileName, bytes, creds?.username, creds?.password, fresh) { s -> onStatus(gen, s) }
        } catch (e: VpnEngineException) {
            fresh.fill(0)
            generation++
            runCatching { engine.stop() }
            clearSession()
            fail(e.message ?: "The VPN could not start")
            return
        } finally {
            bytes.fill(0)
        }
        clearSession()
        token = fresh
        tokenHex = hex(fresh)
        port = newPort
        status = engine.status() ?: VpnStatus(VpnState.CONNECTING, null, clock(), 0, 0, null, profile.server)
        log("vpn: started a ${profile.kind} profile")
        publish()
    }

    private fun stopLocked() {
        generation++
        if (port != null) runCatching { engine.stop() }
        clearSession()
        status = null
        publish()
    }

    private fun clearSession() {
        token?.fill(0)
        token = null
        tokenHex = null
        port = null
    }

    private fun fail(message: String) {
        log("vpn: failed: $message")
        status = VpnStatus(VpnState.FAILED, message, clock(), 0, 0, null, status?.server.orEmpty())
        publish()
    }

    private fun onStatus(gen: Int, s: VpnStatus) = synchronized(lock) {
        if (gen != generation || port == null) return
        if (status?.state != s.state) log("vpn: ${s.state}${s.error?.let { " ($it)" } ?: ""}")
        status = s
        publish()
    }

    private fun publish() {
        val on = settings.enabled
        val up = on && port != null
        val connected = up && status?.state == VpnState.CONNECTED
        val phase = when {
            !on -> VpnPhase.OFF
            connected -> VpnPhase.CONNECTED
            status?.state == VpnState.FAILED || status?.state == VpnState.STOPPED -> VpnPhase.FAILED
            else -> VpnPhase.CONNECTING
        }
        val livePort = if (up) port else null
        val livePass = if (up) tokenHex else null
        VpnRoute.state = VpnRouteState(on, connected, settings.scope, settings.lanDirect, livePort, livePass)
        _state.value = VpnUiState(settings, profiles, if (on) status else null, phase)
        val scope = if (settings.scope == VpnScope.PRIVATE) "private" else "all"
        val g = listOf(on, on && !up, livePort, livePass, scope, settings.lanDirect)
        if (g != lastGuard) {
            lastGuard = g
            guard.setVpn(on, on && !up, livePort, livePass, scope, settings.lanDirect)
        }
    }

    companion object {
        fun hex(bytes: ByteArray): String = bytes.joinToString("") { "%02x".format(it) }
    }
}
