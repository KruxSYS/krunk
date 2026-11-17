package org.krunk.browser.vpn

/**
 * The VPN runtime as the app uses it: a mirror of the FFI object `KrunkVpn` (contract
 * `vpn-contract-android-0.10.0.md` §2) with Kotlin types of its own, so that everything but
 * [FfiVpnEngine] (the one file that names `org.krunk.ffi.KrunkVpn`) compiles and tests without
 * the native library.
 *
 * None of these calls may be given to a log: `bytes`, `password` and `token` are secrets.
 */
interface VpnEngine {
    /** Checks a profile file (≤ 64 KiB) and says what it is; nothing is stored or started. */
    @Throws(VpnEngineException::class)
    fun importProfile(fileName: String, bytes: ByteArray): VpnProfileInfo

    /**
     * Starts the tunnel with this profile (a running one is stopped first) and returns the port
     * of its proxy on 127.0.0.1. [token] is exactly 32 bytes; the proxy accepts only requests
     * that carry it. [onStatus] is called on the VPN's threads and must not call [stop].
     */
    @Throws(VpnEngineException::class)
    fun start(
        fileName: String,
        bytes: ByteArray,
        username: String?,
        password: String?,
        token: ByteArray,
        onStatus: (VpnStatus) -> Unit,
    ): Int

    fun stop()

    /** At process exit: the tunnel stops, the proxy keeps its port and refuses everything. */
    fun stopForExit()

    fun status(): VpnStatus?

    /** The default network changed: new socket and handshake now, back-off reset. Never blocks. */
    fun networkChanged()

    /** A request to [host] was refused in the last 30 s as outside the profile's routes / unknown name. */
    fun refusedRecently(host: String): Boolean

    /** `[ok, refused]` proxy authentications in debug builds; empty in release. */
    fun debugAuthCounts(): List<Long> = emptyList()
}

/** What an imported profile is (`VpnProfileInfo` of the FFI). */
data class VpnProfileInfo(
    /** "wireguard" or "openvpn". */
    val kind: String,
    val name: String,
    val server: String,
    val needsCredentials: Boolean,
    val warnings: List<String>,
)

enum class VpnState { CONNECTING, CONNECTED, RECONNECTING, FAILED, STOPPED }

/** The tunnel's state as the runtime reports it (`VpnStatus` of the FFI). */
data class VpnStatus(
    val state: VpnState,
    val error: String?,
    val sinceMs: Long,
    val rxBytes: Long,
    val txBytes: Long,
    val tunnelIp: String?,
    val server: String,
)

/** `VpnError` of the FFI. The messages are for the user; they never carry profile text. */
sealed class VpnEngineException(message: String) : Exception(message) {
    class Import(val reason: String) : VpnEngineException(reason)

    class Unsupported(val reason: String = "OpenVPN is not supported on Android yet") : VpnEngineException(reason)

    class NeedsCredentials : VpnEngineException("This profile needs a user name and password")

    class Start(val reason: String) : VpnEngineException(reason)

    class BadToken : VpnEngineException("bad proxy token")
}

/**
 * The stand-in while the FFI object is not built in: every import and start fails, so the VPN
 * stays off (or, when it was meant to be on, fail-closed).
 */
object NoVpnEngine : VpnEngine {
    private const val WHY = "The VPN is not available in this build"

    override fun importProfile(fileName: String, bytes: ByteArray): VpnProfileInfo =
        throw VpnEngineException.Import(WHY)

    override fun start(
        fileName: String,
        bytes: ByteArray,
        username: String?,
        password: String?,
        token: ByteArray,
        onStatus: (VpnStatus) -> Unit,
    ): Int = throw VpnEngineException.Start(WHY)

    override fun stop() = Unit

    override fun stopForExit() = Unit

    override fun status(): VpnStatus? = null

    override fun networkChanged() = Unit

    override fun refusedRecently(host: String): Boolean = false
}
