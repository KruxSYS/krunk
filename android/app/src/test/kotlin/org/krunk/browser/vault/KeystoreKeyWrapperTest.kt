package org.krunk.browser.vault

import java.security.GeneralSecurityException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** A key store in memory: real AES-GCM, and switches for what a device can do wrong. */
class FakeWrapKeyStore(private val strongBox: Boolean = true) : WrapKeyStore {
    private val keys = HashMap<String, SecretKey>()

    /** `strongBox` argument of every [generate] call, in order. */
    val generateCalls = ArrayList<Boolean>()

    /** Thrown by [encrypt] and [decrypt] when set. */
    var failure: Exception? = null

    override fun exists(alias: String): Boolean = keys.containsKey(alias)

    override fun generate(alias: String, strongBox: Boolean) {
        generateCalls += strongBox
        if (strongBox && !this.strongBox) throw StrongBoxUnavailable()
        keys[alias] = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
    }

    override fun encrypt(alias: String, plain: ByteArray): Pair<ByteArray, ByteArray> {
        failure?.let { throw it }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, keys[alias] ?: throw KeyFailure.Missing())
        return cipher.iv to cipher.doFinal(plain)
    }

    override fun decrypt(alias: String, iv: ByteArray, ct: ByteArray): ByteArray {
        failure?.let { throw it }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, keys[alias] ?: throw KeyFailure.Missing(), GCMParameterSpec(128, iv))
        return try {
            cipher.doFinal(ct)
        } catch (e: GeneralSecurityException) {
            throw KeyFailure.Other(e.javaClass.simpleName)
        }
    }

    override fun delete(alias: String) {
        keys.remove(alias)
    }
}

class KeystoreKeyWrapperTest {
    private val secret = ByteArray(48) { it.toByte() }

    private fun failureOf(block: () -> Unit): WrapFailure = try {
        block()
        throw AssertionError("expected a WrapFailure")
    } catch (e: WrapFailure) {
        e
    }

    @Test
    fun roundTrip() {
        val wrapper = KeystoreKeyWrapper(FakeWrapKeyStore())
        assertFalse(wrapper.keyPresent())
        wrapper.prepareForNewVault()
        assertTrue(wrapper.keyPresent())
        val blob = wrapper.protect(secret)
        assertArrayEquals(secret, wrapper.unprotect(blob))
        // A fresh IV every time: the same input never gives the same blob.
        assertFalse(blob.contentEquals(wrapper.protect(secret)))
    }

    @Test
    fun blobLayout() {
        val store = FakeWrapKeyStore()
        val wrapper = KeystoreKeyWrapper(store)
        wrapper.prepareForNewVault()
        val blob = wrapper.protect(secret)
        assertEquals(0x01.toByte(), blob[0])
        // version | 12-byte IV | ciphertext | 16-byte tag
        assertEquals(1 + 12 + secret.size + 16, blob.size)
        val plain = store.decrypt(KeystoreKeyWrapper.ALIAS, blob.copyOfRange(1, 13), blob.copyOfRange(13, blob.size))
        assertArrayEquals(secret, plain)
        assertEquals("krunk.vault.wrap.v1", KeystoreKeyWrapper.ALIAS)
    }

    @Test
    fun strongBoxIsAskedForFirst() {
        val store = FakeWrapKeyStore(strongBox = true)
        KeystoreKeyWrapper(store).prepareForNewVault()
        assertEquals(listOf(true), store.generateCalls)
    }

    @Test
    fun withoutStrongBoxTheTeeKeyIsMade() {
        val store = FakeWrapKeyStore(strongBox = false)
        val wrapper = KeystoreKeyWrapper(store)
        wrapper.prepareForNewVault()
        assertEquals(listOf(true, false), store.generateCalls)
        assertArrayEquals(secret, wrapper.unprotect(wrapper.protect(secret)))
    }

    @Test
    fun missingKeyIsReportedAndNeverMade() {
        val store = FakeWrapKeyStore()
        val wrapper = KeystoreKeyWrapper(store)
        wrapper.prepareForNewVault()
        val blob = wrapper.protect(secret)
        wrapper.deleteKey()
        assertFalse(wrapper.keyPresent())

        assertEquals(WrapFailureKind.MISSING, failureOf { wrapper.unprotect(blob) }.kind)
        assertEquals(WrapFailureKind.MISSING, failureOf { wrapper.protect(secret) }.kind)
        // Only the explicit call before made a key.
        assertEquals(1, store.generateCalls.size)
        assertFalse(wrapper.keyPresent())
    }

    @Test
    fun invalidatedKey() {
        val store = FakeWrapKeyStore()
        val wrapper = KeystoreKeyWrapper(store)
        wrapper.prepareForNewVault()
        val blob = wrapper.protect(secret)
        store.failure = KeyFailure.Invalidated()
        assertEquals(WrapFailureKind.INVALIDATED, failureOf { wrapper.unprotect(blob) }.kind)
        assertEquals(WrapFailureKind.INVALIDATED, failureOf { wrapper.protect(secret) }.kind)
    }

    @Test
    fun damagedBlobsFail() {
        val wrapper = KeystoreKeyWrapper(FakeWrapKeyStore())
        wrapper.prepareForNewVault()
        val blob = wrapper.protect(secret)

        val tampered = blob.copyOf().also { it[20] = (it[20] + 1).toByte() }
        val tamperedFailure = failureOf { wrapper.unprotect(tampered) }
        assertEquals(WrapFailureKind.FAILED, tamperedFailure.kind)
        assertEquals("AEADBadTagException", tamperedFailure.detail)

        assertEquals(WrapFailureKind.FAILED, failureOf { wrapper.unprotect(blob.copyOf(20)) }.kind)
        assertEquals(WrapFailureKind.FAILED, failureOf { wrapper.unprotect(ByteArray(0)) }.kind)

        val wrongVersion = blob.copyOf().also { it[0] = 0x02 }
        assertEquals(WrapFailureKind.FAILED, failureOf { wrapper.unprotect(wrongVersion) }.kind)
    }

    @Test
    fun aBlobOfAnotherKeyFails() {
        val first = KeystoreKeyWrapper(FakeWrapKeyStore()).apply { prepareForNewVault() }
        val second = KeystoreKeyWrapper(FakeWrapKeyStore()).apply { prepareForNewVault() }
        assertEquals(WrapFailureKind.FAILED, failureOf { second.unprotect(first.protect(secret)) }.kind)
    }

    @Test
    fun nothingButWrapFailureGetsOut() {
        val store = FakeWrapKeyStore()
        val wrapper = KeystoreKeyWrapper(store)
        wrapper.prepareForNewVault()
        val blob = wrapper.protect(secret)
        for (thrown in listOf(
            IllegalStateException("x"),
            KeyFailure.Other("ProviderException"),
            KeyFailure.Missing(),
        )) {
            store.failure = thrown
            failureOf { wrapper.unprotect(blob) }
            failureOf { wrapper.protect(secret) }
        }
        store.failure = IllegalStateException("x")
        val failure = failureOf { wrapper.unprotect(blob) }
        assertEquals(WrapFailureKind.FAILED, failure.kind)
        // The class name, not the message.
        assertEquals("IllegalStateException", failure.detail)
    }

    @Test
    fun aKeyStoreThatCannotMakeKeys() {
        val broken = object : WrapKeyStore by FakeWrapKeyStore() {
            override fun generate(alias: String, strongBox: Boolean) {
                if (strongBox) throw StrongBoxUnavailable()
                throw KeyFailure.Other("ProviderException")
            }
        }
        val failure = failureOf { KeystoreKeyWrapper(broken).prepareForNewVault() }
        assertEquals(WrapFailureKind.FAILED, failure.kind)
        assertEquals("ProviderException", failure.detail)
    }
}
