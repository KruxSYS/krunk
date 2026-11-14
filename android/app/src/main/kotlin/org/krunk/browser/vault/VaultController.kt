package org.krunk.browser.vault

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

enum class VaultState {
    /** No vault file. */
    NONE,
    LOCKED,
    UNLOCKED,

    /** The file is there, the device key that opens it is not: it can only be deleted. */
    KEY_LOST,
}

enum class LockReason { MANUAL, TIMER, BACKGROUND, PASSWORD_CHANGED, DESTROYED }

/**
 * The vault's lock state, and the one object the screens and the engine side talk to.
 *
 * Nothing secret is kept here: the open vault lives in the core behind [backend], and locking
 * ([VaultBackend.lock]) drops and wipes it there. The vault locks by hand, after
 * [lockMinutes] without use, and [backgroundGraceSeconds] after the app left the foreground
 * (0 = at once; the grace lets the user fetch a code from another app and come back). A dead
 * process starts locked because nothing persists.
 *
 * The three key functions are the Keystore wrap key's ([KeystoreKeyWrapper]): the key is made
 * only by [create], and a vault file without its key shows as [VaultState.KEY_LOST] before any
 * password is asked.
 *
 * @param clock wall-clock milliseconds
 * @param dispatcher where the blocking backend calls of the suspend functions run
 * @param lockMinutes the lock timer setting, read at every [tick]
 * @param backgroundGraceSeconds the "lock after leaving Krunk" setting, read at [onBackground]
 * @param timerScope when given, [tick] runs there every [TICK_MS] while the vault is unlocked,
 * and the lock after the background grace is scheduled there. Both are best effort (a process
 * in the background may be frozen): [onForeground] makes up for what they missed before
 * anything of the vault can be shown.
 */
class VaultController(
    private val backend: VaultBackend,
    private val keyPresent: () -> Boolean,
    private val prepareForNewVault: () -> Unit,
    private val deleteKey: () -> Unit,
    private val clock: () -> Long,
    private val dispatcher: CoroutineDispatcher,
    private val lockMinutes: () -> Int,
    private val backgroundGraceSeconds: () -> Int,
    private val timerScope: CoroutineScope? = null,
) {
    private val _state = MutableStateFlow(VaultState.NONE)
    val state: StateFlow<VaultState> = _state.asStateFlow()

    private val _lockEvents = MutableSharedFlow<LockReason>(
        extraBufferCapacity = 16,
        onBufferOverflow = BufferOverflow.DROP_OLDEST,
    )

    /**
     * One event per lock (and one for [destroy]): whoever shows something from the vault
     * (a revealed password, a picker, a pending prompt) clears it.
     */
    val lockEvents: SharedFlow<LockReason> = _lockEvents.asSharedFlow()

    /**
     * Called (on the calling thread of [changePassword] / [destroy], after the backend's part)
     * when the stored biometric blob has become useless: [LockReason.PASSWORD_CHANGED] or
     * [LockReason.DESTROYED]. [BiometricUnlock] sets it to delete its key and say why.
     */
    @Volatile
    var onBiometricReset: ((LockReason) -> Unit)? = null

    /** Held over every state change together with the backend call it rests on. */
    private val gate = Any()

    /** One of create / unlock / change / destroy at a time. */
    private val ops = Mutex()

    // The four below are guarded by [gate].
    private var background = false

    /** [clock] at [onBackground], and the grace that applied then. */
    private var backgroundSince = 0L
    private var backgroundGraceMs = 0L
    private var graceJob: Job? = null

    /** An unlock found the key unusable; the alias may still exist, so [refresh] must not undo it. */
    @Volatile
    private var keyLost = false

    init {
        refresh()
        timerScope?.launch {
            state.collectLatest { now ->
                while (now == VaultState.UNLOCKED) {
                    delay(TICK_MS)
                    tick()
                }
            }
        }
    }

    /**
     * Makes the device key, then the vault; the vault is unlocked afterwards. If the vault
     * cannot be made the new key is removed again.
     */
    suspend fun create(password: String) = ops.withLock {
        withContext(dispatcher) {
            // Before the key is touched: a new key would orphan an existing vault.
            if (backend.state() != VaultFileState.NONE) throw VaultFailure.AlreadyExists()
            try {
                prepareForNewVault()
            } catch (e: WrapFailure) {
                deleteKey()
                throw VaultFailure.Io("device key: ${e.kind}")
            }
            try {
                backend.create(password, clock())
            } catch (e: Exception) {
                deleteKey()
                throw e
            }
            backend.touch(clock())
        }
        keyLost = false
        opened()
    }

    /**
     * The state is [VaultState.UNLOCKED] afterwards, unless the app has left the foreground
     * meanwhile and the background grace is over (then the vault is locked again at once).
     *
     * @throws VaultFailure [VaultFailure.WrongPassword], [VaultFailure.RateLimited] (see
     * [retryWaitMs]), [VaultFailure.KeyLost] (the state is [VaultState.KEY_LOST] then), ...
     */
    suspend fun unlock(password: String) = open { backend.unlock(password, clock()) }

    /** The biometric path. [key] is wiped when this returns, whatever happened. */
    suspend fun unlockWithKey(key: ByteArray) = try {
        open { backend.unlockWithKey(key) }
    } finally {
        key.fill(0)
    }

    private suspend fun open(how: () -> Unit) = ops.withLock {
        try {
            withContext(dispatcher) {
                when (backend.state()) {
                    VaultFileState.NONE -> throw VaultFailure.NoVault()

                    VaultFileState.UNLOCKED -> Unit

                    VaultFileState.LOCKED -> {
                        if (!keyPresent()) throw VaultFailure.KeyLost("no key in the key store")
                        how()
                    }
                }
                backend.touch(clock())
            }
        } catch (e: VaultFailure.KeyLost) {
            keyLost = true
            _state.value = VaultState.KEY_LOST
            throw e
        }
        keyLost = false
        opened()
    }

    /**
     * The backend opened the vault. The state is read back rather than assumed: a lock may
     * have come in while the password was being checked.
     */
    private fun opened() = synchronized(gate) {
        refresh()
        // The app went to the background meanwhile, and for longer than the grace.
        if (background && graceOver()) lock(LockReason.BACKGROUND)
    }

    /** Milliseconds until the next password attempt is allowed (0 = now). */
    fun retryWaitMs(): Long = backend.retryWaitMs(clock())

    fun lock(reason: LockReason = LockReason.MANUAL) = synchronized(gate) {
        backend.lock()
        if (_state.compareAndSet(VaultState.UNLOCKED, VaultState.LOCKED)) _lockEvents.tryEmit(reason)
        Unit
    }

    /**
     * The vault stays unlocked, as on Windows. The stored biometric blob held the old unlock
     * key: it is removed, and biometric unlock is off until switched on again.
     */
    suspend fun changePassword(old: String, new: String) = ops.withLock {
        if (_state.value != VaultState.UNLOCKED) throw VaultFailure.Locked()
        val stillOpen = withContext(dispatcher) {
            backend.changePassword(old, new)
            backend.setBioBlob(null)
            backend.touch(clock())
            backend.state() == VaultFileState.UNLOCKED
        }
        onBiometricReset?.invoke(LockReason.PASSWORD_CHANGED)
        // A backend that ends the change locked: say so rather than show a stale state.
        if (!stillOpen) lock(LockReason.PASSWORD_CHANGED)
    }

    /**
     * Runs [block] with the open vault on the dispatcher and counts as use for the lock timer.
     *
     * @throws VaultFailure.Locked when the vault is not unlocked (also when it locks meanwhile)
     */
    suspend fun <T> use(block: (VaultBackend) -> T): T {
        if (_state.value != VaultState.UNLOCKED) throw VaultFailure.Locked()
        return withContext(dispatcher) {
            backend.touch(clock())
            block(backend)
        }
    }

    /**
     * Like [use] for what the engine asks by itself (logins for a loaded page, "is this login
     * new?"): it does not count as the user using the vault, so it never delays the lock.
     */
    suspend fun <T> peek(block: (VaultBackend) -> T): T {
        if (_state.value != VaultState.UNLOCKED) throw VaultFailure.Locked()
        return withContext(dispatcher) { block(backend) }
    }

    /**
     * "Try again" on the key-lost screen: asks the key store anew. A key that is really gone is
     * found gone again; one that was only out of reach gives the locked vault back.
     */
    fun recheckKey() {
        synchronized(gate) { keyLost = false }
        refresh()
    }

    /** Works while locked: does this site have logins? */
    fun hasHint(url: String): Boolean = _state.value != VaultState.NONE && backend.hasHint(url)

    /**
     * The app left the foreground. With a grace of 0 the vault locks at once; otherwise it
     * locks when the grace is over and the app is still away: by the scheduled job if the
     * process gets to run it, else in [onForeground].
     */
    fun onBackground() = synchronized(gate) {
        graceJob?.cancel()
        graceJob = null
        background = true
        backgroundSince = clock()
        backgroundGraceMs = backgroundGraceSeconds().coerceAtLeast(0) * 1000L
        if (backgroundGraceMs == 0L) {
            lock(LockReason.BACKGROUND)
        } else {
            // Also when locked: an unlock on its way may still open the vault while away.
            val grace = backgroundGraceMs
            graceJob = timerScope?.launch {
                delay(grace)
                graceElapsed(coroutineContext[Job])
            }
        }
    }

    /** The scheduled end of the grace; [job] tells a stale run (the app has been back since) apart. */
    private fun graceElapsed(job: Job?) = synchronized(gate) {
        if (background && graceJob === job) {
            graceJob = null
            lock(LockReason.BACKGROUND)
        }
    }

    /** Away for the whole grace? A clock that went backwards counts as yes. */
    private fun graceOver(): Boolean {
        val away = clock() - backgroundSince
        return away < 0 || away >= backgroundGraceMs
    }

    /**
     * The app is in front again. Call before any screen draws: a vault that was away for the
     * grace or longer is locked here, first thing and on the calling thread. Then the lock
     * timer is checked; time in the background is time without use.
     */
    fun onForeground() = synchronized(gate) {
        graceJob?.cancel()
        graceJob = null
        if (background && graceOver()) lock(LockReason.BACKGROUND)
        background = false
        refresh()
        tick()
    }

    /** Locks when the timer has run out. Called periodically and on return to the foreground. */
    fun tick() {
        if (_state.value == VaultState.UNLOCKED && backend.autolockDue(clock(), lockMinutes())) {
            lock(LockReason.TIMER)
        }
    }

    /** The way out of [VaultState.KEY_LOST]: removes the vault, its side file and the device key. */
    suspend fun destroy() = ops.withLock {
        // The way out of a lost key only: an open or merely locked vault is never deleted here.
        if (_state.value != VaultState.KEY_LOST) throw VaultFailure.NotAllowed()
        withContext(dispatcher) {
            backend.lock()
            backend.destroy()
            deleteKey()
        }
        onBiometricReset?.invoke(LockReason.DESTROYED)
        keyLost = false
        refresh()
        _lockEvents.tryEmit(LockReason.DESTROYED)
        Unit
    }

    /** Works the state out again from the vault file and the device key. */
    fun refresh() = synchronized(gate) {
        _state.value = when (backend.state()) {
            VaultFileState.NONE -> VaultState.NONE
            VaultFileState.UNLOCKED -> VaultState.UNLOCKED
            VaultFileState.LOCKED -> if (keyLost || !keyPresent()) VaultState.KEY_LOST else VaultState.LOCKED
        }
    }

    companion object {
        const val TICK_MS = 15_000L
        const val DEFAULT_LOCK_MINUTES = 15
        const val DEFAULT_BACKGROUND_GRACE_SECONDS = 60
    }
}
