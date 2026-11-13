package org.krunk.browser.ui.vault

import android.app.Activity
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import org.krunk.browser.vault.BioAvailability
import org.krunk.browser.vault.BioOffReason
import org.krunk.browser.vault.BioUnlockResult
import org.krunk.browser.vault.LockReason
import org.krunk.browser.vault.VaultBackend
import org.krunk.browser.vault.VaultFailure
import org.krunk.browser.vault.VaultState

/**
 * What the vault screens need from the vault: the members of `VaultController` they call, and
 * nothing else. The screens and their state holders take this, so they run against a fake in
 * tests and against the in-memory [PreviewVaultPort] until the controller is wired in
 * ([vaultPort]).
 */
interface VaultPort {
    val state: StateFlow<VaultState>

    /** One event per lock, whatever caused it. */
    val lockEvents: SharedFlow<LockReason>

    @Throws(VaultFailure::class)
    suspend fun create(password: String)

    /** Takes about a second (the password derivation). */
    @Throws(VaultFailure::class)
    suspend fun unlock(password: String)

    /** Milliseconds until the next password attempt is allowed (0 = now). */
    fun retryWaitMs(): Long

    fun lock(reason: LockReason = LockReason.MANUAL)

    @Throws(VaultFailure::class)
    suspend fun changePassword(old: String, new: String)

    /** Runs [block] off the main thread; throws [VaultFailure.Locked] when not unlocked. */
    @Throws(VaultFailure::class)
    suspend fun <T> use(block: (VaultBackend) -> T): T

    /** The "device key lost" way out: removes the vault; the state becomes NONE. */
    suspend fun destroy()

    val biometric: BiometricPort

    /** The lock timer (minutes without vault use). */
    val lockMinutes: StateFlow<Int>

    fun setLockMinutes(minutes: Int)

    /**
     * How long the vault stays open after Krunk left the foreground: one of
     * [BACKGROUND_GRACE_CHOICES], 0 = locks at once.
     */
    val backgroundGraceSeconds: StateFlow<Int>

    fun setBackgroundGraceSeconds(seconds: Int)

    /** Whether pages' sign-ins are offered for saving. Readable and settable without a vault. */
    val offerToSave: StateFlow<Boolean>

    fun setOfferToSave(on: Boolean)

    /** `VaultBackend.minPasswordLength()`, readable without an unlocked vault. */
    val minPasswordLength: Int
}

/** The members of `BiometricUnlock` the screens call. */
interface BiometricPort {
    fun availability(): BioAvailability

    val enabled: StateFlow<Boolean>

    /** Why biometric unlock was switched off behind the user's back, until it is on again. */
    val offReason: StateFlow<BioOffReason?>

    /** Throws when the user cancels or the key store refuses. */
    suspend fun enable(activity: Activity)

    suspend fun unlock(activity: Activity): BioUnlockResult

    fun disable()
}

/** The lock timer's choices (plan assumption C). */
val LOCK_MINUTE_CHOICES = listOf(1, 5, 15, 30, 60)

/** The choices of "Lock after leaving Krunk", in seconds; 0 = at once. */
val BACKGROUND_GRACE_CHOICES = listOf(0, 30, 60, 300)
