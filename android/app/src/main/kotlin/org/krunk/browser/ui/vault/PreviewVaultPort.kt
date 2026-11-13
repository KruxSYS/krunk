package org.krunk.browser.ui.vault

import android.app.Activity
import java.net.URI
import kotlin.coroutines.CoroutineContext
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.withContext
import org.krunk.browser.vault.BioAvailability
import org.krunk.browser.vault.BioOffReason
import org.krunk.browser.vault.BioUnlockResult
import org.krunk.browser.vault.LockReason
import org.krunk.browser.vault.SaveDecision
import org.krunk.browser.vault.VaultBackend
import org.krunk.browser.vault.VaultFailure
import org.krunk.browser.vault.VaultFileState
import org.krunk.browser.vault.VaultItem
import org.krunk.browser.vault.VaultItemDetail
import org.krunk.browser.vault.VaultMatch
import org.krunk.browser.vault.VaultSecret
import org.krunk.browser.vault.VaultState

/**
 * A vault that lives in memory and encrypts nothing: what the screens run against until the
 * real controller is wired in, and the fake of their tests. It keeps the rules the screens
 * depend on (wrong password, the wait after repeated failures, locked calls fail, entries are
 * validated) and none of the protection.
 */
class PreviewVaultPort(
    private val clock: () -> Long = System::currentTimeMillis,
    private val workContext: CoroutineContext = Dispatchers.Default,
    /** Stands in for the password derivation, so the busy state is visible. */
    private val deriveMs: Long = 700,
) : VaultPort {
    val backend = InMemoryVaultBackend()

    /** Tests: the device key is gone. */
    var keyLost = false
        set(value) {
            field = value
            sync()
        }

    private val _state = MutableStateFlow(VaultState.NONE)
    override val state: StateFlow<VaultState> = _state

    private val _lockEvents = MutableSharedFlow<LockReason>(extraBufferCapacity = 8)
    override val lockEvents: SharedFlow<LockReason> = _lockEvents

    private val _lockMinutes = MutableStateFlow(DEFAULT_LOCK_MINUTES)
    override val lockMinutes: StateFlow<Int> = _lockMinutes

    private val _backgroundGraceSeconds = MutableStateFlow(DEFAULT_GRACE_SECONDS)
    override val backgroundGraceSeconds: StateFlow<Int> = _backgroundGraceSeconds

    private val _offerToSave = MutableStateFlow(true)
    override val offerToSave: StateFlow<Boolean> = _offerToSave

    override val biometric = PreviewBiometric(this)

    override val minPasswordLength: Int get() = backend.minPasswordLength()

    override suspend fun create(password: String) {
        work { backend.create(password, clock()) }
        sync()
    }

    override suspend fun unlock(password: String) {
        if (keyLost) throw VaultFailure.KeyLost("preview")
        try {
            work { backend.unlock(password, clock()) }
        } finally {
            sync()
        }
    }

    override fun retryWaitMs(): Long = backend.retryWaitMs(clock())

    override fun lock(reason: LockReason) {
        if (backend.state() != VaultFileState.UNLOCKED) return
        backend.lock()
        sync()
        _lockEvents.tryEmit(reason)
    }

    override suspend fun changePassword(old: String, new: String) {
        work { backend.changePassword(old, new) }
        biometric.switchedOff(BioOffReason.PASSWORD_CHANGED)
    }

    override suspend fun <T> use(block: (VaultBackend) -> T): T {
        if (backend.state() != VaultFileState.UNLOCKED) throw VaultFailure.Locked()
        return withContext(workContext) { block(backend) }
    }

    override suspend fun destroy() {
        val wasOpen = backend.state() == VaultFileState.UNLOCKED
        backend.destroy()
        biometric.disable()
        keyLost = false
        if (wasOpen) _lockEvents.tryEmit(LockReason.DESTROYED)
    }

    override fun setLockMinutes(minutes: Int) {
        if (minutes in LOCK_MINUTE_CHOICES) _lockMinutes.value = minutes
    }

    override fun setBackgroundGraceSeconds(seconds: Int) {
        if (seconds in BACKGROUND_GRACE_CHOICES) _backgroundGraceSeconds.value = seconds
    }

    override fun setOfferToSave(on: Boolean) {
        _offerToSave.value = on
    }

    internal fun unlockedByBiometric() {
        backend.unlockWithKey(backend.unlockKey())
        sync()
    }

    private suspend fun <T> work(block: () -> T): T {
        if (deriveMs > 0) delay(deriveMs)
        return withContext(workContext) { block() }
    }

    private fun sync() {
        _state.value = when {
            backend.state() == VaultFileState.NONE -> VaultState.NONE
            keyLost -> VaultState.KEY_LOST
            backend.state() == VaultFileState.UNLOCKED -> VaultState.UNLOCKED
            else -> VaultState.LOCKED
        }
    }

    private companion object {
        const val DEFAULT_LOCK_MINUTES = 15
        const val DEFAULT_GRACE_SECONDS = 60
    }
}

/** Biometric unlock without a sensor: every prompt is answered with [nextResult]. */
class PreviewBiometric(private val port: PreviewVaultPort) : BiometricPort {
    var available = BioAvailability.AVAILABLE
    var nextResult = BioUnlockResult.UNLOCKED

    private val _enabled = MutableStateFlow(false)
    override val enabled: StateFlow<Boolean> = _enabled

    private val _offReason = MutableStateFlow<BioOffReason?>(null)
    override val offReason: StateFlow<BioOffReason?> = _offReason

    override fun availability(): BioAvailability = available

    override suspend fun enable(activity: Activity) = enableNow()

    /** [enable] without an activity, for tests. */
    fun enableNow() {
        check(available == BioAvailability.AVAILABLE) { "biometric unlock is not available" }
        _enabled.value = true
        _offReason.value = null
    }

    override suspend fun unlock(activity: Activity): BioUnlockResult = unlockNow()

    fun unlockNow(): BioUnlockResult {
        if (!_enabled.value) return BioUnlockResult.FAILED
        if (nextResult == BioUnlockResult.UNLOCKED) port.unlockedByBiometric()
        return nextResult
    }

    override fun disable() {
        _enabled.value = false
        _offReason.value = null
    }

    fun switchedOff(reason: BioOffReason) {
        if (!_enabled.value) return
        _enabled.value = false
        _offReason.value = reason
    }
}

/** [VaultBackend] over a list. No encryption; every call is synchronized on the instance. */
class InMemoryVaultBackend : VaultBackend {
    private class Login(
        val id: String,
        var origin: String,
        var username: String,
        var password: String,
        var note: String,
        val created: Long,
        var updated: Long,
    )

    private var master: String? = null
    private var open = false
    private var failures = 0
    private var lastFailure = 0L
    private var nextId = 1
    private val logins = mutableListOf<Login>()
    private var blob: ByteArray? = null
    private var lastUse = 0L

    /** Tests: the next call that needs the open vault fails with this. */
    var failNext: VaultFailure? = null

    /** Tests: how often [reveal] was called. */
    var reveals = 0
        private set

    @Synchronized
    override fun state(): VaultFileState = when {
        master == null -> VaultFileState.NONE
        open -> VaultFileState.UNLOCKED
        else -> VaultFileState.LOCKED
    }

    @Synchronized
    override fun create(password: String, now: Long) {
        if (master != null) throw VaultFailure.AlreadyExists()
        if (password.length < minPasswordLength()) throw VaultFailure.WeakPassword()
        master = password
        open = true
        lastUse = now
    }

    @Synchronized
    override fun unlock(password: String, now: Long) {
        val expected = master ?: throw VaultFailure.NoVault()
        val wait = retryWaitMs(now)
        if (wait > 0) throw VaultFailure.RateLimited(wait)
        if (password != expected) {
            failures++
            lastFailure = now
            throw VaultFailure.WrongPassword()
        }
        failures = 0
        open = true
        lastUse = now
    }

    @Synchronized
    override fun retryWaitMs(now: Long): Long {
        if (failures < FREE_ATTEMPTS) return 0
        val wait = WAIT_STEP_MS * (failures - FREE_ATTEMPTS + 1)
        return (lastFailure + wait - now).coerceAtLeast(0)
    }

    @Synchronized
    override fun unlockWithKey(key: ByteArray) {
        if (master == null) throw VaultFailure.NoVault()
        open = true
    }

    @Synchronized
    override fun unlockKey(): ByteArray = ByteArray(KEY_BYTES)

    @Synchronized
    override fun lock() {
        open = false
    }

    @Synchronized
    override fun changePassword(old: String, new: String) {
        requireOpen()
        if (old != master) throw VaultFailure.WrongPassword()
        if (new.length < minPasswordLength()) throw VaultFailure.WeakPassword()
        master = new
    }

    @Synchronized
    override fun search(query: String): List<VaultItem> {
        requireOpen()
        val q = query.trim().lowercase()
        return logins
            .filter { q.isEmpty() || it.origin.lowercase().contains(q) || it.username.lowercase().contains(q) }
            .sortedWith(compareBy({ hostOf(it.origin) }, { it.username.lowercase() }))
            .map { VaultItem(it.id, it.origin, it.username, it.note.isNotEmpty(), it.created, it.updated) }
    }

    @Synchronized
    override fun entry(id: String): VaultItemDetail {
        val l = find(id)
        return VaultItemDetail(l.id, l.origin, l.username, l.note, l.created, l.updated)
    }

    @Synchronized
    override fun reveal(id: String): String {
        val l = find(id)
        reveals++
        return l.password
    }

    @Synchronized
    override fun saveLogin(url: String, username: String, password: String, now: Long): String {
        requireOpen()
        val origin = originOf(url) ?: throw VaultFailure.InvalidEntry("origin")
        val same = logins.firstOrNull { it.origin == origin && it.username == username }
        return upsert(same?.id, url, username, password, same?.note, now)
    }

    @Synchronized
    override fun upsert(
        id: String?,
        url: String,
        username: String,
        password: String?,
        note: String?,
        now: Long,
    ): String {
        requireOpen()
        val origin = originOf(url) ?: throw VaultFailure.InvalidEntry("origin")
        if (username.length > MAX_FIELD) throw VaultFailure.InvalidEntry("username too long")
        if (password != null && (password.isEmpty() || password.length > MAX_FIELD)) {
            throw VaultFailure.InvalidEntry("password length")
        }
        if ((note?.length ?: 0) > MAX_NOTE) throw VaultFailure.InvalidEntry("note too long")
        if (id == null) {
            val secret = password ?: throw VaultFailure.InvalidEntry("password")
            val made = Login((nextId++).toString(), origin, username, secret, note.orEmpty(), now, now)
            logins += made
            return made.id
        }
        val l = find(id)
        l.origin = origin
        l.username = username
        if (password != null) l.password = password
        if (note != null) l.note = note
        l.updated = now
        return l.id
    }

    @Synchronized
    override fun delete(id: String) {
        logins.remove(find(id))
    }

    @Synchronized
    override fun matchesFor(url: String): List<VaultMatch> = forHttpAuth(url)

    @Synchronized
    override fun forHttpAuth(url: String): List<VaultMatch> {
        requireOpen()
        val origin = originOf(url) ?: return emptyList()
        return logins.filter { it.origin == origin }.map { VaultMatch(it.id, it.origin, it.username, exact = true) }
    }

    @Synchronized
    override fun secretForFill(id: String, pageUrl: String): VaultSecret {
        val l = find(id)
        if (originOf(pageUrl) != l.origin) throw VaultFailure.NotAllowed()
        return VaultSecret(l.username, l.password)
    }

    @Synchronized
    override fun saveDecision(url: String, username: String, password: String): SaveDecision {
        requireOpen()
        val origin = originOf(url) ?: throw VaultFailure.InvalidEntry("origin")
        val same = logins.firstOrNull { it.origin == origin && it.username == username }
        return when {
            same == null -> SaveDecision.New
            same.password == password -> SaveDecision.Unchanged
            else -> SaveDecision.Update(same.id)
        }
    }

    @Synchronized
    override fun hasHint(url: String): Boolean {
        val origin = originOf(url) ?: return false
        return logins.any { it.origin == origin }
    }

    @Synchronized
    override fun bioBlob(): ByteArray? = blob

    @Synchronized
    override fun setBioBlob(blob: ByteArray?) {
        this.blob = blob
    }

    @Synchronized
    override fun touch(now: Long) {
        lastUse = now
    }

    @Synchronized
    override fun autolockDue(now: Long, minutes: Int): Boolean = open && now - lastUse >= minutes * MINUTE_MS

    @Synchronized
    override fun destroy() {
        master = null
        open = false
        failures = 0
        logins.clear()
        blob = null
    }

    override fun minPasswordLength(): Int = MIN_PASSWORD

    private fun requireOpen() {
        if (!open) throw VaultFailure.Locked()
        failNext?.let {
            failNext = null
            throw it
        }
    }

    private fun find(id: String): Login {
        requireOpen()
        return logins.firstOrNull { it.id == id } ?: throw VaultFailure.NotFound()
    }

    private companion object {
        const val MIN_PASSWORD = 8
        const val FREE_ATTEMPTS = 3
        const val WAIT_STEP_MS = 5_000L
        const val KEY_BYTES = 32
        const val MAX_FIELD = 1_000
        const val MAX_NOTE = 16 * 1024
        const val MINUTE_MS = 60_000L

        /** `scheme://host[:port]` of an http(s) address; a bare host is taken as https. */
        fun originOf(input: String): String? {
            val text = input.trim()
            if (text.isEmpty() || text.any { it.isWhitespace() }) return null
            val withScheme = if (text.contains("://")) text else "https://$text"
            val uri = runCatching { URI(withScheme) }.getOrNull() ?: return null
            val scheme = uri.scheme?.lowercase() ?: return null
            if (scheme != "http" && scheme != "https") return null
            val host = uri.host?.lowercase()?.takeIf { it.isNotEmpty() } ?: return null
            val port = if (uri.port == -1) "" else ":${uri.port}"
            return "$scheme://$host$port"
        }

        fun hostOf(origin: String): String = origin.substringAfter("://")
    }
}
