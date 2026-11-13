package org.krunk.browser.vault

import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.security.ProviderException
import java.security.UnrecoverableKeyException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Why a Keystore key could not be used. The texts carry class names only, never key material. */
sealed class KeyFailure(detail: String) : Exception(detail) {
    /** No key under the alias (never made, app data restored from elsewhere, key store reset). */
    class Missing : KeyFailure("no key")

    /** The key exists but the system will never let it be used again. */
    class Invalidated : KeyFailure("key invalidated")

    /** Anything else; [className] is the class of the exception the system threw. */
    class Other(val className: String) : KeyFailure(className)
}

/** [WrapKeyStore.generate] with `strongBox = true` on a device without StrongBox. */
class StrongBoxUnavailable : Exception("no StrongBox")

/**
 * The key store as [KeystoreKeyWrapper] needs it; the seam that lets JVM tests run the wrapper
 * and its error paths with a fake. Every function but [exists] and [delete] throws [KeyFailure].
 */
interface WrapKeyStore {
    /** Never throws: a key store that cannot answer has no usable key. */
    fun exists(alias: String): Boolean

    /** Makes a new key, replacing one under the same alias. */
    @Throws(KeyFailure::class, StrongBoxUnavailable::class)
    fun generate(alias: String, strongBox: Boolean)

    /** Returns the IV the key store chose and the ciphertext with its tag. */
    @Throws(KeyFailure::class)
    fun encrypt(alias: String, plain: ByteArray): Pair<ByteArray, ByteArray>

    @Throws(KeyFailure::class)
    fun decrypt(alias: String, iv: ByteArray, ct: ByteArray): ByteArray

    /** Never throws. */
    fun delete(alias: String)
}

enum class WrapFailureKind { MISSING, INVALIDATED, FAILED }

/** What [KeystoreKeyWrapper] throws; the core turns it into "the device key is lost". */
class WrapFailure(val kind: WrapFailureKind, val detail: String) : Exception("$kind: $detail")

/**
 * Wraps the vault key (already sealed with the vault password by the core) with a key that
 * never leaves this device's key store, so a copied vault file cannot even be guessed at
 * elsewhere. The key needs no user authentication: the vault password is what guards the vault.
 *
 * Blob: version byte 0x01, 12-byte IV, ciphertext with the GCM tag.
 *
 * The key is made only by [prepareForNewVault] ("Create vault"). [protect] and [unprotect]
 * never make one: with the key gone, a new key could not open the existing vault, and making
 * one silently would hide that the vault is lost.
 */
class KeystoreKeyWrapper(private val store: WrapKeyStore) {
    fun keyPresent(): Boolean = store.exists(ALIAS)

    /** A fresh key for a new vault: StrongBox if the device has one, else the TEE. */
    @Throws(WrapFailure::class)
    fun prepareForNewVault() = guarded {
        try {
            store.generate(ALIAS, strongBox = true)
        } catch (_: StrongBoxUnavailable) {
            store.generate(ALIAS, strongBox = false)
        }
    }

    @Throws(WrapFailure::class)
    fun protect(data: ByteArray): ByteArray = guarded {
        if (!store.exists(ALIAS)) throw KeyFailure.Missing()
        val (iv, ct) = store.encrypt(ALIAS, data)
        if (iv.size != IV_LENGTH) throw WrapFailure(WrapFailureKind.FAILED, "IV of ${iv.size} bytes")
        byteArrayOf(VERSION) + iv + ct
    }

    @Throws(WrapFailure::class)
    fun unprotect(data: ByteArray): ByteArray = guarded {
        if (!store.exists(ALIAS)) throw KeyFailure.Missing()
        if (data.size < 1 + IV_LENGTH + TAG_LENGTH) throw WrapFailure(WrapFailureKind.FAILED, "blob too short")
        if (data[0] != VERSION) throw WrapFailure(WrapFailureKind.FAILED, "blob version ${data[0]}")
        store.decrypt(ALIAS, data.copyOfRange(1, 1 + IV_LENGTH), data.copyOfRange(1 + IV_LENGTH, data.size))
    }

    fun deleteKey() = store.delete(ALIAS)

    /** Nothing but [WrapFailure] leaves this class, whatever the key store does. */
    private inline fun <T> guarded(block: () -> T): T = try {
        block()
    } catch (e: WrapFailure) {
        throw e
    } catch (_: KeyFailure.Missing) {
        throw WrapFailure(WrapFailureKind.MISSING, "no key in the key store")
    } catch (_: KeyFailure.Invalidated) {
        throw WrapFailure(WrapFailureKind.INVALIDATED, "the key can no longer be used")
    } catch (e: KeyFailure.Other) {
        throw WrapFailure(WrapFailureKind.FAILED, e.className)
    } catch (e: Exception) {
        throw WrapFailure(WrapFailureKind.FAILED, e.javaClass.simpleName)
    }

    companion object {
        const val ALIAS = "krunk.vault.wrap.v1"
        const val VERSION: Byte = 0x01
        const val IV_LENGTH = 12
        const val TAG_LENGTH = 16

        /** The wrapper on the device's real key store. */
        fun onDevice(): KeystoreKeyWrapper = KeystoreKeyWrapper(AndroidWrapKeyStore())
    }
}

/** Shared by the two real key stores (wrap key and biometric key). */
internal object AndroidKeys {
    const val PROVIDER = "AndroidKeyStore"
    const val TRANSFORMATION = "AES/GCM/NoPadding"
    const val TAG_BITS = 128

    /** The key under [alias]; [KeyFailure] if it is missing or unusable. */
    fun secretKey(alias: String): SecretKey = mapped {
        val keyStore = KeyStore.getInstance(PROVIDER).apply { load(null) }
        keyStore.getKey(alias, null) as? SecretKey ?: throw KeyFailure.Missing()
    }

    fun exists(alias: String): Boolean = try {
        KeyStore.getInstance(PROVIDER).apply { load(null) }.containsAlias(alias)
    } catch (_: GeneralSecurityException) {
        false
    } catch (_: IOException) {
        false
    } catch (_: RuntimeException) {
        false
    }

    fun delete(alias: String) {
        try {
            KeyStore.getInstance(PROVIDER).apply { load(null) }.deleteEntry(alias)
        } catch (_: GeneralSecurityException) {
            // Nothing to delete, or a key store that does not answer: either way no usable key.
        } catch (_: IOException) {
        } catch (_: RuntimeException) {
        }
    }

    /**
     * Generates an AES-256-GCM key. StrongBox exists from Android 9; before that, and on
     * devices without it, [StrongBoxUnavailable] tells the caller to ask for the TEE key.
     */
    fun generate(alias: String, strongBox: Boolean, configure: (KeyGenParameterSpec.Builder) -> Unit) {
        if (strongBox && Build.VERSION.SDK_INT < Build.VERSION_CODES.P) throw StrongBoxUnavailable()
        mapped {
            val spec = KeyGenParameterSpec.Builder(
                alias,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                // The key store chooses a fresh IV for every encryption.
                .setRandomizedEncryptionRequired(true)
            if (strongBox && Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) spec.setIsStrongBoxBacked(true)
            configure(spec)
            try {
                KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, PROVIDER).run {
                    init(spec.build())
                    generateKey()
                }
            } catch (e: ProviderException) {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P && e is StrongBoxUnavailableException) {
                    throw StrongBoxUnavailable()
                }
                throw e
            }
        }
    }

    /** Turns what the key store throws into [KeyFailure]; nothing else gets out. */
    inline fun <T> mapped(block: () -> T): T = try {
        block()
    } catch (e: KeyFailure) {
        throw e
    } catch (e: StrongBoxUnavailable) {
        throw e
    } catch (_: KeyPermanentlyInvalidatedException) {
        throw KeyFailure.Invalidated()
    } catch (_: UnrecoverableKeyException) {
        throw KeyFailure.Invalidated()
    } catch (e: GeneralSecurityException) {
        // AEADBadTagException, KeyStoreException, InvalidKeyException, ...
        throw KeyFailure.Other(e.javaClass.simpleName)
    } catch (e: IOException) {
        throw KeyFailure.Other(e.javaClass.simpleName)
    } catch (e: RuntimeException) {
        // ProviderException and whatever else a vendor's key store throws.
        throw KeyFailure.Other(e.javaClass.simpleName)
    }
}

/** The device's key store: AES-256-GCM, hardware-backed, not exportable, no user authentication. */
class AndroidWrapKeyStore : WrapKeyStore {
    override fun exists(alias: String): Boolean = AndroidKeys.exists(alias)

    override fun generate(alias: String, strongBox: Boolean) = AndroidKeys.generate(alias, strongBox) {}

    override fun encrypt(alias: String, plain: ByteArray): Pair<ByteArray, ByteArray> = AndroidKeys.mapped {
        val cipher = Cipher.getInstance(AndroidKeys.TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, AndroidKeys.secretKey(alias))
        val ct = cipher.doFinal(plain)
        cipher.iv to ct
    }

    override fun decrypt(alias: String, iv: ByteArray, ct: ByteArray): ByteArray = AndroidKeys.mapped {
        val cipher = Cipher.getInstance(AndroidKeys.TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, AndroidKeys.secretKey(alias), GCMParameterSpec(AndroidKeys.TAG_BITS, iv))
        cipher.doFinal(ct)
    }

    override fun delete(alias: String) = AndroidKeys.delete(alias)
}
