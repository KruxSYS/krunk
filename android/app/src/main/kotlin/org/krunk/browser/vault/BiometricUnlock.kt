package org.krunk.browser.vault

import android.app.Activity
import android.content.Context
import android.hardware.biometrics.BiometricManager
import android.hardware.biometrics.BiometricPrompt
import android.os.Build
import android.os.CancellationSignal
import android.security.keystore.KeyProperties
import android.util.Log
import androidx.annotation.RequiresApi
import java.security.GeneralSecurityException
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import kotlin.coroutines.resume
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import org.krunk.browser.KrunkApp

enum class BioAvailability { AVAILABLE, NO_HARDWARE, NONE_ENROLLED, UNSUPPORTED_OS, UNAVAILABLE }

/** Why biometric unlock switched itself off; the password dialog says so. */
enum class BioOffReason { ENROLLMENT_CHANGED, PASSWORD_CHANGED, BLOB_MISSING, KEY_MISSING }

sealed interface BioUnlockResult {
    data object Unlocked : BioUnlockResult

    /** The user closed the prompt or chose "Use password". Nothing changed. */
    data object Cancelled : BioUnlockResult

    /** Biometric unlock is off (as of now, or already): ask for the password and say why. */
    data class FellBack(val reason: BioOffReason?) : BioUnlockResult

    /** It did not work this time (sensor error, lockout): ask for the password. Nothing changed. */
    data object Failed : BioUnlockResult
}

enum class BioEnableResult { ENABLED, CANCELLED, UNAVAILABLE, LOCKED, FAILED }

/**
 * The biometric key as [BiometricUnlock] needs it. Its ciphers work only after the system has
 * seen a strong biometric match for exactly that cipher (a `CryptoObject`).
 */
interface BioKeyStore {
    /** Never throws. */
    fun exists(): Boolean

    /** Makes a new key, replacing an old one. */
    @Throws(KeyFailure::class, StrongBoxUnavailable::class)
    fun generate(strongBox: Boolean)

    @Throws(KeyFailure::class)
    fun encryptCipher(): Cipher

    /** [KeyFailure.Invalidated] when the enrolled biometrics have changed since [generate]. */
    @Throws(KeyFailure::class)
    fun decryptCipher(iv: ByteArray): Cipher

    /** Never throws. */
    fun delete()
}

sealed interface BioPromptResult {
    /** The cipher the system has released for one operation. */
    class Success(val cipher: Cipher) : BioPromptResult

    data object Cancelled : BioPromptResult

    /** [code] is a `BiometricPrompt.BIOMETRIC_ERROR_*` value. */
    data class Error(val code: Int) : BioPromptResult
}

/** The system's biometric prompt. Call on the main thread. */
fun interface BioPrompt {
    suspend fun authenticate(cipher: Cipher): BioPromptResult
}

/**
 * Optional unlock by fingerprint or face, off by default. Switching it on encrypts the vault's
 * unlock key (never the password) with a key store key that works only after a strong
 * biometric match and dies when the enrolled biometrics change; the result (version byte 0x01,
 * 12-byte IV, ciphertext with tag) is kept by the backend next to the vault. The vault
 * password always works.
 *
 * "On" is not stored anywhere: it is on while both the blob and the key exist. Call [refresh]
 * when a screen that shows the switch or the unlock dialog opens.
 *
 * @param backend for the blob, which must be readable while the vault is locked
 * @param dispatcher where key store and backend calls run; [prompt] runs on the caller's
 */
class BiometricUnlock(
    private val controller: VaultController,
    private val backend: VaultBackend,
    private val keys: BioKeyStore,
    private val prompt: BioPrompt,
    private val availabilityCheck: () -> BioAvailability,
    private val dispatcher: CoroutineDispatcher,
) {
    private val _enabled = MutableStateFlow(false)
    val enabled: StateFlow<Boolean> = _enabled.asStateFlow()

    private val _offReason = MutableStateFlow<BioOffReason?>(null)

    /** Set when biometric unlock went off by itself; cleared by [enable] and [disable]. */
    val offReason: StateFlow<BioOffReason?> = _offReason.asStateFlow()

    init {
        controller.onBiometricReset = { reason ->
            // The blob is gone already (the controller removed it, or the whole vault).
            val wasOn = _enabled.value || keys.exists()
            keys.delete()
            _enabled.value = false
            val changed = wasOn && reason == LockReason.PASSWORD_CHANGED
            _offReason.value = if (changed) BioOffReason.PASSWORD_CHANGED else null
        }
    }

    /** Biometric unlock is offered only when this is [BioAvailability.AVAILABLE]. */
    fun availability(): BioAvailability = availabilityCheck()

    /**
     * Works out [enabled] from what is stored. Half a pair (a key without its blob or the
     * other way round) is removed and reported in [offReason].
     */
    suspend fun refresh() {
        withContext(dispatcher) { reconcile() }
    }

    /** On the dispatcher. Returns the blob when biometric unlock is on. */
    private fun reconcile(): ByteArray? {
        val blob = backend.bioBlob()
        val key = keys.exists()
        when {
            blob != null && key -> {
                _enabled.value = true
                return blob
            }

            blob == null && key -> switchOff(BioOffReason.BLOB_MISSING)

            blob != null -> switchOff(BioOffReason.KEY_MISSING)

            else -> _enabled.value = false
        }
        return null
    }

    /** On the dispatcher: removes key and blob. An earlier reason is the truer one and stays. */
    private fun switchOff(reason: BioOffReason?) {
        keys.delete()
        backend.setBioBlob(null)
        _enabled.value = false
        if (reason == null || _offReason.value == null) _offReason.value = reason
    }

    /**
     * Switches biometric unlock on. The vault must be unlocked; the user confirms with the
     * biometric prompt. Anything but [BioEnableResult.ENABLED] leaves it off.
     */
    suspend fun enable(): BioEnableResult {
        if (availability() != BioAvailability.AVAILABLE) return BioEnableResult.UNAVAILABLE
        if (controller.state.value != VaultState.UNLOCKED) return BioEnableResult.LOCKED
        val result = try {
            val cipher = withContext(dispatcher) {
                switchOff(null)
                try {
                    keys.generate(strongBox = true)
                } catch (_: StrongBoxUnavailable) {
                    keys.generate(strongBox = false)
                }
                keys.encryptCipher()
            }
            when (val answer = prompt.authenticate(cipher)) {
                is BioPromptResult.Success -> {
                    controller.use { vault -> vault.setBioBlob(seal(answer.cipher, vault)) }
                    BioEnableResult.ENABLED
                }

                BioPromptResult.Cancelled -> BioEnableResult.CANCELLED

                is BioPromptResult.Error -> BioEnableResult.FAILED
            }
        } catch (_: VaultFailure.Locked) {
            BioEnableResult.LOCKED
        } catch (e: CancellationException) {
            withContext(NonCancellable + dispatcher) { switchOff(null) }
            throw e
        } catch (e: Exception) {
            // KeyFailure, a cipher the system refused, a backend error.
            Log.w(KrunkApp.TAG, "biometric unlock not switched on: ${e.javaClass.simpleName}")
            BioEnableResult.FAILED
        }
        withContext(dispatcher) {
            if (result == BioEnableResult.ENABLED) {
                _offReason.value = null
                _enabled.value = true
            } else {
                switchOff(null)
            }
        }
        return result
    }

    /** Encrypts the vault's unlock key; the plain copy is wiped before this returns. */
    private fun seal(cipher: Cipher, vault: VaultBackend): ByteArray {
        val key = vault.unlockKey()
        try {
            return byteArrayOf(VERSION) + cipher.iv + cipher.doFinal(key)
        } finally {
            key.fill(0)
        }
    }

    /**
     * Unlocks the vault with the biometric prompt. A key the system has invalidated (new
     * fingerprint or face enrolled), a missing half, or a blob from before a password change
     * switch biometric unlock off: [BioUnlockResult.FellBack] with the reason.
     */
    suspend fun unlock(): BioUnlockResult {
        when (controller.state.value) {
            VaultState.UNLOCKED -> return BioUnlockResult.Unlocked
            VaultState.LOCKED -> Unit
            VaultState.NONE, VaultState.KEY_LOST -> return BioUnlockResult.Failed
        }
        val prepared = withContext(dispatcher) { prepareUnlock() }
        val (cipher, sealed) = when (prepared) {
            is Prepared.Ready -> prepared.cipher to prepared.sealed
            is Prepared.Stop -> return prepared.result
        }
        val released = when (val answer = prompt.authenticate(cipher)) {
            is BioPromptResult.Success -> answer.cipher
            BioPromptResult.Cancelled -> return BioUnlockResult.Cancelled
            is BioPromptResult.Error -> return BioUnlockResult.Failed
        }
        val key = try {
            withContext(dispatcher) { released.doFinal(sealed) }
        } catch (e: GeneralSecurityException) {
            Log.w(KrunkApp.TAG, "biometric unlock: ${e.javaClass.simpleName}")
            return BioUnlockResult.Failed
        } catch (e: RuntimeException) {
            Log.w(KrunkApp.TAG, "biometric unlock: ${e.javaClass.simpleName}")
            return BioUnlockResult.Failed
        }
        return try {
            controller.unlockWithKey(key)
            if (controller.state.value == VaultState.UNLOCKED) BioUnlockResult.Unlocked else BioUnlockResult.Failed
        } catch (_: VaultFailure.WrongPassword) {
            // The blob holds an unlock key from before a password change.
            withContext(dispatcher) { switchOff(BioOffReason.PASSWORD_CHANGED) }
            BioUnlockResult.FellBack(_offReason.value)
        } catch (e: VaultFailure) {
            Log.w(KrunkApp.TAG, "biometric unlock: ${e.javaClass.simpleName}")
            BioUnlockResult.Failed
        }
    }

    private sealed interface Prepared {
        class Ready(val cipher: Cipher, val sealed: ByteArray) : Prepared

        class Stop(val result: BioUnlockResult) : Prepared
    }

    private fun prepareUnlock(): Prepared {
        val blob = reconcile() ?: return Prepared.Stop(BioUnlockResult.FellBack(_offReason.value))
        if (blob.size < 1 + IV_LENGTH + TAG_LENGTH || blob[0] != VERSION) {
            switchOff(BioOffReason.BLOB_MISSING)
            return Prepared.Stop(BioUnlockResult.FellBack(_offReason.value))
        }
        return try {
            val cipher = keys.decryptCipher(blob.copyOfRange(1, 1 + IV_LENGTH))
            Prepared.Ready(cipher, blob.copyOfRange(1 + IV_LENGTH, blob.size))
        } catch (_: KeyFailure.Invalidated) {
            switchOff(BioOffReason.ENROLLMENT_CHANGED)
            Prepared.Stop(BioUnlockResult.FellBack(_offReason.value))
        } catch (_: KeyFailure.Missing) {
            switchOff(BioOffReason.KEY_MISSING)
            Prepared.Stop(BioUnlockResult.FellBack(_offReason.value))
        } catch (e: KeyFailure.Other) {
            Log.w(KrunkApp.TAG, "biometric key: ${e.className}")
            Prepared.Stop(BioUnlockResult.Failed)
        }
    }

    /** Switches biometric unlock off: the key and the blob are deleted. */
    suspend fun disable() {
        withContext(dispatcher) { switchOff(null) }
    }

    companion object {
        const val VERSION: Byte = 0x01
        const val IV_LENGTH = 12
        const val TAG_LENGTH = 16

        /**
         * Biometric unlock on the device's key store with the system prompt.
         * [negativeText] is the prompt's button that leads back to the password ("Use password").
         */
        fun forActivity(
            activity: Activity,
            controller: VaultController,
            backend: VaultBackend,
            dispatcher: CoroutineDispatcher,
            title: String,
            negativeText: String,
        ): BiometricUnlock = BiometricUnlock(
            controller,
            backend,
            AndroidBioKeyStore(),
            BioPrompt { cipher ->
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                    AndroidBioPrompt(activity, title, negativeText).authenticate(cipher)
                } else {
                    BioPromptResult.Error(0)
                }
            },
            { AndroidBiometrics.availability(activity) },
            dispatcher,
        )
    }
}

object AndroidBiometrics {
    /** Offered from Android 11, where a key can be bound to strong biometrics alone. */
    fun availability(context: Context): BioAvailability {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return BioAvailability.UNSUPPORTED_OS
        val manager = context.getSystemService(BiometricManager::class.java) ?: return BioAvailability.NO_HARDWARE
        return when (manager.canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_STRONG)) {
            BiometricManager.BIOMETRIC_SUCCESS -> BioAvailability.AVAILABLE
            BiometricManager.BIOMETRIC_ERROR_NO_HARDWARE -> BioAvailability.NO_HARDWARE
            BiometricManager.BIOMETRIC_ERROR_NONE_ENROLLED -> BioAvailability.NONE_ENROLLED
            else -> BioAvailability.UNAVAILABLE
        }
    }
}

/**
 * The biometric key: AES-256-GCM, usable only through a strong biometric match for each
 * operation, invalidated for good when a fingerprint or face is enrolled. StrongBox first.
 */
class AndroidBioKeyStore : BioKeyStore {
    override fun exists(): Boolean = AndroidKeys.exists(ALIAS)

    override fun generate(strongBox: Boolean) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) throw KeyFailure.Other("UnsupportedOs")
        AndroidKeys.generate(ALIAS, strongBox) { spec ->
            spec.setUserAuthenticationRequired(true)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                spec.setUserAuthenticationParameters(0, KeyProperties.AUTH_BIOMETRIC_STRONG)
            }
            spec.setInvalidatedByBiometricEnrollment(true)
        }
    }

    override fun encryptCipher(): Cipher = AndroidKeys.mapped {
        Cipher.getInstance(AndroidKeys.TRANSFORMATION).apply {
            init(Cipher.ENCRYPT_MODE, AndroidKeys.secretKey(ALIAS))
        }
    }

    override fun decryptCipher(iv: ByteArray): Cipher = AndroidKeys.mapped {
        Cipher.getInstance(AndroidKeys.TRANSFORMATION).apply {
            init(Cipher.DECRYPT_MODE, AndroidKeys.secretKey(ALIAS), GCMParameterSpec(AndroidKeys.TAG_BITS, iv))
        }
    }

    override fun delete() = AndroidKeys.delete(ALIAS)

    companion object {
        const val ALIAS = "krunk.vault.bio.v1"
    }
}

/**
 * The platform's biometric prompt (no AndroidX Biometric: its stable release needs a
 * `FragmentActivity`). Strong biometrics only, no device-credential fallback: the way back is
 * the negative button, which leads to the vault password.
 */
@RequiresApi(Build.VERSION_CODES.R)
class AndroidBioPrompt(private val activity: Activity, private val title: String, private val negativeText: String) :
    BioPrompt {
    override suspend fun authenticate(cipher: Cipher): BioPromptResult = suspendCancellableCoroutine { cont ->
        val signal = CancellationSignal()
        cont.invokeOnCancellation { signal.cancel() }
        val executor = activity.mainExecutor
        val systemPrompt = BiometricPrompt.Builder(activity)
            .setTitle(title)
            .setAllowedAuthenticators(BiometricManager.Authenticators.BIOMETRIC_STRONG)
            .setNegativeButton(negativeText, executor) { _, _ ->
                if (cont.isActive) cont.resume(BioPromptResult.Cancelled)
            }
            .build()
        val callback = object : BiometricPrompt.AuthenticationCallback() {
            override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                val released = result.cryptoObject?.cipher
                if (cont.isActive) {
                    cont.resume(if (released != null) BioPromptResult.Success(released) else BioPromptResult.Error(0))
                }
            }

            override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                val cancelled = errorCode == BiometricPrompt.BIOMETRIC_ERROR_USER_CANCELED ||
                    errorCode == BiometricPrompt.BIOMETRIC_ERROR_CANCELED
                if (cont.isActive) {
                    cont.resume(if (cancelled) BioPromptResult.Cancelled else BioPromptResult.Error(errorCode))
                }
            }
            // onAuthenticationFailed (no match) keeps the prompt open: nothing to do.
        }
        try {
            systemPrompt.authenticate(BiometricPrompt.CryptoObject(cipher), signal, executor, callback)
        } catch (e: RuntimeException) {
            Log.w(KrunkApp.TAG, "biometric prompt: ${e.javaClass.simpleName}")
            if (cont.isActive) cont.resume(BioPromptResult.Error(0))
        }
    }
}
