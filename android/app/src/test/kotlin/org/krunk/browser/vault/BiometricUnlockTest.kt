package org.krunk.browser.vault

import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The biometric key in memory: real AES-GCM ciphers, no authentication. */
class FakeBioKeyStore(private val strongBox: Boolean = true) : BioKeyStore {
    private var key: SecretKey? = null

    /** `strongBox` argument of every [generate] call, in order. */
    val generateCalls = ArrayList<Boolean>()

    /** A fingerprint was enrolled since the key was made. */
    var invalidated = false

    /** Thrown by [generate] (after the StrongBox answer) when set. */
    var failGenerate: KeyFailure? = null

    override fun exists(): Boolean = key != null

    override fun generate(strongBox: Boolean) {
        generateCalls += strongBox
        if (strongBox && !this.strongBox) throw StrongBoxUnavailable()
        failGenerate?.let { throw it }
        key = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
        invalidated = false
    }

    override fun encryptCipher(): Cipher = Cipher.getInstance("AES/GCM/NoPadding").apply {
        init(Cipher.ENCRYPT_MODE, usable())
    }

    override fun decryptCipher(iv: ByteArray): Cipher = Cipher.getInstance("AES/GCM/NoPadding").apply {
        init(Cipher.DECRYPT_MODE, usable(), GCMParameterSpec(128, iv))
    }

    override fun delete() {
        key = null
    }

    private fun usable(): SecretKey {
        if (invalidated) throw KeyFailure.Invalidated()
        return key ?: throw KeyFailure.Missing()
    }
}

/** The system prompt: answers at once with [answer] (a match by default). */
class FakeBioPrompt : BioPrompt {
    var answer: (Cipher) -> BioPromptResult = { BioPromptResult.Success(it) }
    var shown = 0

    override suspend fun authenticate(cipher: Cipher): BioPromptResult {
        shown++
        return answer(cipher)
    }
}

@OptIn(ExperimentalCoroutinesApi::class)
class BiometricUnlockTest {
    private val backend = FakeVaultBackend()
    private var keys = FakeBioKeyStore()
    private val prompt = FakeBioPrompt()
    private var availability = BioAvailability.AVAILABLE
    private var now = 1_000_000L

    private class Setup(val vault: VaultController, val bio: BiometricUnlock)

    /** A vault that exists and is unlocked, with biometric unlock off. */
    private suspend fun TestScope.setup(): Setup {
        val dispatcher = UnconfinedTestDispatcher(testScheduler)
        val vault = VaultController(
            backend = backend,
            keyPresent = { true },
            prepareForNewVault = {},
            deleteKey = {},
            clock = { now },
            dispatcher = dispatcher,
            lockMinutes = { 15 },
            backgroundGraceSeconds = { 60 },
        )
        vault.create(PASSWORD)
        return Setup(vault, BiometricUnlock(vault, backend, keys, prompt, { availability }, dispatcher))
    }

    /** Everything that must stay as it is when an unlock merely falls back to the password. */
    private fun Setup.snapshot() = listOf(
        vault.state.value,
        bio.enabled.value,
        bio.offReason.value,
        keys.exists(),
        backend.bioBlob()?.toList(),
        backend.state(),
        vault.retryWaitMs(),
    )

    @Test
    fun offByDefault() = runTest {
        val s = setup()
        s.bio.refresh()
        assertFalse(s.bio.enabled.value)
        assertNull(s.bio.offReason.value)
        assertNull(backend.bioBlob())
        assertFalse(keys.exists())
    }

    @Test
    fun offeredOnlyWhenAvailable() = runTest {
        val s = setup()
        for (state in BioAvailability.entries.filter { it != BioAvailability.AVAILABLE }) {
            availability = state
            assertEquals(state, s.bio.availability())
            assertEquals(BioEnableResult.UNAVAILABLE, s.bio.enable())
        }
        assertEquals(0, prompt.shown)
        assertTrue(keys.generateCalls.isEmpty())
        assertFalse(s.bio.enabled.value)

        availability = BioAvailability.AVAILABLE
        assertEquals(BioEnableResult.ENABLED, s.bio.enable())
    }

    @Test
    fun enableStoresTheSealedUnlockKey() = runTest {
        val s = setup()
        val unlockKey = backend.unlockKey()
        assertEquals(BioEnableResult.ENABLED, s.bio.enable())
        assertTrue(s.bio.enabled.value)
        assertEquals(1, prompt.shown)

        val blob = assertNotNullAnd(backend.bioBlob())
        assertEquals(0x01.toByte(), blob[0])
        assertEquals(1 + 12 + unlockKey.size + 16, blob.size)
        // Sealed, not stored: the key itself is nowhere in the blob.
        assertFalse(blob.toList().windowed(unlockKey.size).contains(unlockKey.toList()))
        val plain = keys.decryptCipher(blob.copyOfRange(1, 13)).doFinal(blob.copyOfRange(13, blob.size))
        assertArrayEquals(unlockKey, plain)
    }

    @Test
    fun enableNeedsAnUnlockedVault() = runTest {
        val s = setup()
        s.vault.lock()
        assertEquals(BioEnableResult.LOCKED, s.bio.enable())
        assertEquals(0, prompt.shown)
        assertFalse(keys.exists())
    }

    @Test
    fun aCancelledOrFailedEnableLeavesNothingBehind() = runTest {
        val s = setup()
        prompt.answer = { BioPromptResult.Cancelled }
        assertEquals(BioEnableResult.CANCELLED, s.bio.enable())
        assertFalse(keys.exists())
        assertNull(backend.bioBlob())

        prompt.answer = { BioPromptResult.Error(7) }
        assertEquals(BioEnableResult.FAILED, s.bio.enable())
        assertFalse(keys.exists())

        // No biometrics enrolled after all: the key store refuses to make the key.
        prompt.answer = { BioPromptResult.Success(it) }
        keys.failGenerate = KeyFailure.Other("InvalidAlgorithmParameterException")
        assertEquals(BioEnableResult.FAILED, s.bio.enable())
        assertFalse(s.bio.enabled.value)
        assertNull(s.bio.offReason.value)
        assertNull(backend.bioBlob())
    }

    @Test
    fun strongBoxIsAskedForFirst() = runTest {
        val s = setup()
        assertEquals(BioEnableResult.ENABLED, s.bio.enable())
        assertEquals(listOf(true), keys.generateCalls)
    }

    @Test
    fun withoutStrongBoxTheTeeKeyIsMade() = runTest {
        keys = FakeBioKeyStore(strongBox = false)
        val s = setup()
        assertEquals(BioEnableResult.ENABLED, s.bio.enable())
        assertEquals(listOf(true, false), keys.generateCalls)
        s.vault.lock()
        assertEquals(BioUnlockResult.Unlocked, s.bio.unlock())
    }

    @Test
    fun unlockGoesThroughUnlockWithKey() = runTest {
        val s = setup()
        s.bio.enable()
        s.vault.lock()
        assertEquals(0, backend.keyUnlocks)

        assertEquals(BioUnlockResult.Unlocked, s.bio.unlock())
        assertEquals(VaultState.UNLOCKED, s.vault.state.value)
        assertEquals(1, backend.keyUnlocks)
        assertEquals(2, prompt.shown)
        // Still on, for the next time.
        assertTrue(s.bio.enabled.value)
        s.vault.lock()
        assertEquals(BioUnlockResult.Unlocked, s.bio.unlock())
    }

    @Test
    fun aNewProcessFindsItSwitchedOn() = runTest {
        val s = setup()
        s.bio.enable()
        s.vault.lock()
        val later =
            BiometricUnlock(s.vault, backend, keys, prompt, { availability }, UnconfinedTestDispatcher(testScheduler))
        assertFalse(later.enabled.value)
        later.refresh()
        assertTrue(later.enabled.value)
        assertEquals(BioUnlockResult.Unlocked, later.unlock())
    }

    @Test
    fun anInvalidatedKeySwitchesItOff() = runTest {
        val s = setup()
        s.bio.enable()
        s.vault.lock()
        keys.invalidated = true
        val shown = prompt.shown

        assertEquals(BioUnlockResult.FellBack(BioOffReason.ENROLLMENT_CHANGED), s.bio.unlock())
        assertFalse(keys.exists())
        assertNull(backend.bioBlob())
        assertFalse(s.bio.enabled.value)
        assertEquals(BioOffReason.ENROLLMENT_CHANGED, s.bio.offReason.value)
        assertEquals(shown, prompt.shown)
        assertEquals(VaultState.LOCKED, s.vault.state.value)

        // The password still works, and the reason stays until the user decides.
        s.vault.unlock(PASSWORD)
        assertEquals(BioOffReason.ENROLLMENT_CHANGED, s.bio.offReason.value)
        assertEquals(BioEnableResult.ENABLED, s.bio.enable())
        assertNull(s.bio.offReason.value)
    }

    @Test
    fun aPasswordChangeSwitchesItOff() = runTest {
        val s = setup()
        s.bio.enable()
        s.vault.changePassword(PASSWORD, NEW_PASSWORD)
        assertNull(backend.bioBlob())
        assertFalse(keys.exists())
        assertFalse(s.bio.enabled.value)
        assertEquals(BioOffReason.PASSWORD_CHANGED, s.bio.offReason.value)

        s.vault.lock()
        val shown = prompt.shown
        assertEquals(BioUnlockResult.FellBack(BioOffReason.PASSWORD_CHANGED), s.bio.unlock())
        assertEquals(shown, prompt.shown)
        s.vault.unlock(NEW_PASSWORD)
        assertEquals(VaultState.UNLOCKED, s.vault.state.value)
    }

    @Test
    fun aPasswordChangeWithBiometricsOffGivesNoReason() = runTest {
        val s = setup()
        s.vault.changePassword(PASSWORD, NEW_PASSWORD)
        assertFalse(s.bio.enabled.value)
        assertNull(s.bio.offReason.value)
    }

    @Test
    fun aBlobFromBeforeAPasswordChangeSwitchesItOff() = runTest {
        // The blob survived a password change (say, one made where nobody removed it).
        val s = setup()
        s.bio.enable()
        val stale = backend.bioBlob()
        s.vault.use { it.changePassword(PASSWORD, NEW_PASSWORD) }
        backend.setBioBlob(stale)
        s.vault.lock()

        assertEquals(BioUnlockResult.FellBack(BioOffReason.PASSWORD_CHANGED), s.bio.unlock())
        assertEquals(VaultState.LOCKED, s.vault.state.value)
        assertNull(backend.bioBlob())
        assertFalse(keys.exists())
        assertFalse(s.bio.enabled.value)
    }

    @Test
    fun aMissingBlobFallsBackToThePassword() = runTest {
        val s = setup()
        s.bio.enable()
        s.vault.lock()
        backend.setBioBlob(null)
        val shown = prompt.shown

        assertEquals(BioUnlockResult.FellBack(BioOffReason.BLOB_MISSING), s.bio.unlock())
        assertEquals(shown, prompt.shown)
        assertFalse(s.bio.enabled.value)
        // The key without its blob is of no use and goes too; the vault is as it was.
        assertFalse(keys.exists())
        assertEquals(VaultState.LOCKED, s.vault.state.value)
        assertEquals(0, s.vault.retryWaitMs())
        assertEquals(0, backend.keyUnlocks)
        s.vault.unlock(PASSWORD)
    }

    @Test
    fun aMissingKeyFallsBackToThePassword() = runTest {
        val s = setup()
        s.bio.enable()
        s.vault.lock()
        keys.delete()
        assertEquals(BioUnlockResult.FellBack(BioOffReason.KEY_MISSING), s.bio.unlock())
        assertNull(backend.bioBlob())
        assertEquals(BioOffReason.KEY_MISSING, s.bio.offReason.value)
    }

    @Test
    fun unlockWhileSwitchedOffAsksForThePasswordWithoutAReason() = runTest {
        val s = setup()
        s.vault.lock()
        val before = s.snapshot()
        assertEquals(BioUnlockResult.FellBack(null), s.bio.unlock())
        assertEquals(before, s.snapshot())
        assertEquals(0, prompt.shown)
    }

    @Test
    fun aCancelledPromptChangesNothing() = runTest {
        val s = setup()
        s.bio.enable()
        s.vault.lock()
        val before = s.snapshot()

        prompt.answer = { BioPromptResult.Cancelled }
        assertEquals(BioUnlockResult.Cancelled, s.bio.unlock())
        assertEquals(before, s.snapshot())

        // A sensor error or lockout: the same, with another answer.
        prompt.answer = { BioPromptResult.Error(7) }
        assertEquals(BioUnlockResult.Failed, s.bio.unlock())
        assertEquals(before, s.snapshot())
        assertEquals(0, backend.keyUnlocks)

        // Biometrics still work afterwards, and so does the password.
        prompt.answer = { BioPromptResult.Success(it) }
        assertEquals(BioUnlockResult.Unlocked, s.bio.unlock())
        s.vault.lock()
        s.vault.unlock(PASSWORD)
    }

    @Test
    fun disableDeletesKeyAndBlob() = runTest {
        val s = setup()
        s.bio.enable()
        s.bio.disable()
        assertFalse(s.bio.enabled.value)
        assertNull(s.bio.offReason.value)
        assertFalse(keys.exists())
        assertNull(backend.bioBlob())
        s.vault.lock()
        assertEquals(BioUnlockResult.FellBack(null), s.bio.unlock())
    }

    @Test
    fun destroyingTheVaultRemovesTheKey() = runTest {
        val s = setup()
        s.bio.enable()
        s.vault.destroy()
        assertFalse(keys.exists())
        assertFalse(s.bio.enabled.value)
        assertNull(s.bio.offReason.value)
    }

    @Test
    fun unlockOfAnOpenOrAbsentVault() = runTest {
        val s = setup()
        s.bio.enable()
        val shown = prompt.shown
        assertEquals(BioUnlockResult.Unlocked, s.bio.unlock())
        assertEquals(shown, prompt.shown)
        s.vault.destroy()
        assertEquals(BioUnlockResult.Failed, s.bio.unlock())
    }

    private fun assertNotNullAnd(blob: ByteArray?): ByteArray {
        assertNotNull(blob)
        return blob ?: ByteArray(0)
    }

    private companion object {
        const val PASSWORD = "correct horse"
        const val NEW_PASSWORD = "battery staple"
    }
}
