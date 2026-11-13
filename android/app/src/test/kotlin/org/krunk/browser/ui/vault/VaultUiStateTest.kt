package org.krunk.browser.ui.vault

import kotlin.coroutines.EmptyCoroutineContext
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.browser.R
import org.krunk.browser.vault.BioAvailability
import org.krunk.browser.vault.BioOffReason
import org.krunk.browser.vault.BioUnlockResult
import org.krunk.browser.vault.LockReason
import org.krunk.browser.vault.VaultFailure
import org.krunk.browser.vault.VaultState

@OptIn(ExperimentalCoroutinesApi::class)
class VaultUiStateTest {
    private class Fixture(val port: PreviewVaultPort, val holder: VaultUiState) {
        val model get() = holder.model.value
        val entry get() = model.dialog as? VaultDialog.Entry
        val editor get() = model.dialog as? VaultDialog.Editor
    }

    /** A port and a holder on the test's clock; nothing takes real time. */
    private fun TestScope.fixture(): Fixture {
        val port =
            PreviewVaultPort(clock = { testScheduler.currentTime }, workContext = EmptyCoroutineContext, deriveMs = 0)
        val holder = VaultUiState(port, backgroundScope, clock = { testScheduler.currentTime })
        runCurrent()
        return Fixture(port, holder)
    }

    /** An open vault with two logins. */
    private fun TestScope.opened(): Fixture {
        val f = fixture()
        f.holder.create(PASSWORD, PASSWORD)
        runCurrent()
        for ((site, user) in listOf("https://example.com" to "ada", "https://krunk.test" to "grace")) {
            f.holder.add()
            f.holder.save(site, user, "s3cret-$user", "")
            runCurrent()
        }
        f.holder.noticeShown()
        return f
    }

    // ----- create -----

    @Test
    fun aTooShortPasswordIsRefusedBeforeTheVaultIsAsked() = runTest {
        val f = fixture()
        f.holder.create("short", "short")
        runCurrent()
        assertEquals(VaultText(R.string.vault_error_too_short, 8), f.model.createError)
        assertEquals(VaultState.NONE, f.model.vault)
    }

    @Test
    fun twoDifferentPasswordsAreRefused() = runTest {
        val f = fixture()
        f.holder.create(PASSWORD, PASSWORD + "x")
        runCurrent()
        assertEquals(VaultText(R.string.vault_error_mismatch), f.model.createError)
        assertEquals(VaultState.NONE, f.port.state.value)
        // Typing again takes the error away.
        f.holder.createEdited()
        assertNull(f.model.createError)
    }

    @Test
    fun aGoodPasswordCreatesAnOpenEmptyVault() = runTest {
        val f = fixture()
        f.holder.create(PASSWORD, PASSWORD)
        runCurrent()
        assertEquals(VaultState.UNLOCKED, f.model.vault)
        assertNull(f.model.createError)
        assertFalse(f.model.busy)
        assertTrue(f.model.loaded)
        assertTrue(f.model.items.isEmpty())
    }

    // ----- unlock -----

    @Test
    fun aWrongPasswordIsSaidAndTheVaultStaysLocked() = runTest {
        val f = opened()
        f.holder.lockNow()
        runCurrent()
        f.holder.unlock.submit("not it")
        runCurrent()
        val unlock = f.holder.unlock.model.value
        assertEquals(VaultText(R.string.vault_error_wrong_password), unlock.error)
        assertEquals(0, unlock.waitSeconds)
        assertFalse(unlock.busy)
        assertEquals(VaultState.LOCKED, f.model.vault)
    }

    @Test
    fun afterRepeatedFailuresTheWaitIsCountedDownAndBlocksAttempts() = runTest {
        val f = opened()
        f.holder.lockNow()
        runCurrent()
        repeat(3) {
            f.holder.unlock.submit("not it")
            runCurrent()
        }
        fun unlock() = f.holder.unlock.model.value
        assertEquals(5, unlock().waitSeconds)
        assertEquals(VaultText(R.string.vault_error_rate_limited, 5), unlock().waitText)
        assertFalse(unlock().canSubmit)

        // The right password does not get through while waiting.
        f.holder.unlock.submit(PASSWORD)
        runCurrent()
        assertEquals(VaultState.LOCKED, f.model.vault)

        advanceTimeBy(1_000)
        runCurrent()
        assertEquals(VaultText(R.string.vault_error_rate_limited, 4), unlock().waitText)
        advanceTimeBy(3_500)
        runCurrent()
        assertEquals(1, unlock().waitSeconds)
        advanceTimeBy(500)
        runCurrent()
        assertNull(unlock().waitText)
        assertTrue(unlock().canSubmit)

        f.holder.unlock.submit(PASSWORD)
        runCurrent()
        assertEquals(VaultState.UNLOCKED, f.model.vault)
        assertEquals(2, f.model.items.size)
    }

    @Test
    fun aRateLimitFromTheVaultShowsOnlyTheCountdown() = runTest {
        val f = opened()
        f.holder.lockNow()
        runCurrent()
        repeat(3) {
            f.holder.unlock.submit("not it")
            runCurrent()
        }
        // A second form (the dialog over a page) opened during the wait starts counting at once.
        val dialog = UnlockUiState(f.port, backgroundScope)
        runCurrent()
        assertEquals(5, dialog.model.value.waitSeconds)
        assertNull(dialog.model.value.error)
    }

    @Test
    fun aLostKeyIsSaidAtUnlock() = runTest {
        val f = opened()
        f.holder.lockNow()
        runCurrent()
        val unlock = UnlockUiState(f.port, backgroundScope)
        f.port.keyLost = true
        // The form was open before the state changed.
        unlock.submit(PASSWORD)
        runCurrent()
        assertEquals(VaultText(R.string.vault_error_key_lost), unlock.model.value.error)
    }

    @Test
    fun theFingerprintButtonIsOfferedOnlyWhenSwitchedOnAndUsable() = runTest {
        val f = opened()
        fun unlock() = f.holder.unlock.model.value
        assertFalse(unlock().biometricOffered)
        f.port.biometric.enableNow()
        runCurrent()
        assertTrue(unlock().biometricOffered)

        f.holder.lockNow()
        runCurrent()
        f.port.biometric.nextResult = BioUnlockResult.FAILED
        f.holder.unlock.biometric { f.port.biometric.unlockNow() }
        runCurrent()
        assertEquals(VaultText(R.string.vault_bio_failed), unlock().error)
        assertEquals(VaultState.LOCKED, f.model.vault)

        f.port.biometric.nextResult = BioUnlockResult.CANCELLED
        f.holder.unlock.biometric { f.port.biometric.unlockNow() }
        runCurrent()
        assertNull(unlock().error)

        f.port.biometric.nextResult = BioUnlockResult.UNLOCKED
        f.holder.unlock.biometric { f.port.biometric.unlockNow() }
        runCurrent()
        assertEquals(VaultState.UNLOCKED, f.model.vault)
    }

    @Test
    fun aSwitchedOffFingerprintUnlockIsExplainedOnTheForm() = runTest {
        val f = opened()
        f.port.biometric.enableNow()
        f.holder.askChangePassword()
        f.holder.changePassword(PASSWORD, "another password", "another password")
        runCurrent()
        val unlock = f.holder.unlock.model.value
        assertFalse(unlock.biometricOffered)
        assertEquals(BioOffReason.PASSWORD_CHANGED, unlock.offReason)
        assertEquals(VaultText(R.string.vault_bio_off_password), bioOffText(unlock.offReason))
    }

    // ----- list and search -----

    @Test
    fun theListIsAsTheVaultReturnsItAndSearchNarrowsIt() = runTest {
        val f = opened()
        assertEquals(listOf("https://example.com", "https://krunk.test"), f.model.items.map { it.origin })
        f.holder.setQuery("  KRUNK ")
        runCurrent()
        assertEquals(listOf("grace"), f.model.items.map { it.username })
        f.holder.setQuery("nothing like it")
        runCurrent()
        assertTrue(f.model.items.isEmpty())
        assertTrue(f.model.loaded)
        f.holder.setQuery("")
        runCurrent()
        assertEquals(2, f.model.items.size)
    }

    @Test
    fun aFailingListIsSaidAndNotMistakenForAnEmptyVault() = runTest {
        val f = opened()
        f.port.backend.failNext = VaultFailure.Corrupt("test")
        f.holder.refresh()
        runCurrent()
        assertEquals(VaultText(R.string.vault_error_corrupt), f.model.notice)
        assertEquals(2, f.model.items.size)
    }

    // ----- reveal -----

    @Test
    fun aPasswordIsFetchedOnlyOnShowAndHidesItselfAfterThirtySeconds() = runTest {
        val f = opened()
        f.holder.open(f.model.items.first().id)
        runCurrent()
        assertEquals("ada", f.entry?.detail?.username)
        assertNull(f.entry?.revealed)
        assertEquals(0, f.port.backend.reveals)

        f.holder.reveal()
        runCurrent()
        assertEquals("s3cret-ada", f.entry?.revealed?.value)
        assertEquals(1, f.port.backend.reveals)

        advanceTimeBy(VaultUiState.REVEAL_MS - 1)
        runCurrent()
        assertNotNull(f.entry?.revealed)
        advanceTimeBy(1)
        runCurrent()
        assertNotNull(f.entry)
        assertNull(f.entry?.revealed)
    }

    @Test
    fun hideAndLeavingTakeTheRevealedPasswordAway() = runTest {
        val f = opened()
        f.holder.open(f.model.items.first().id)
        runCurrent()
        f.holder.reveal()
        runCurrent()
        f.holder.hide()
        assertNull(f.entry?.revealed)

        f.holder.reveal()
        runCurrent()
        assertNotNull(f.entry?.revealed)
        f.holder.clearSecrets()
        assertNull(f.model.dialog)
        assertTrue(f.model.items.isEmpty())
    }

    @Test
    fun aLockClearsTheRevealedPasswordTheOpenEntryAndTheSearch() = runTest {
        val f = opened()
        f.holder.setQuery("example")
        f.holder.open(f.model.items.first().id)
        runCurrent()
        f.holder.reveal()
        runCurrent()
        assertNotNull(f.entry?.revealed)

        f.port.lock(LockReason.BACKGROUND)
        runCurrent()
        assertEquals(VaultState.LOCKED, f.model.vault)
        assertNull(f.model.dialog)
        assertEquals("", f.model.query)
        assertTrue(f.model.items.isEmpty())
    }

    @Test
    fun aLockClosesAnOpenEditor() = runTest {
        val f = opened()
        f.holder.add()
        assertNotNull(f.editor)
        f.port.lock(LockReason.TIMER)
        runCurrent()
        assertNull(f.model.dialog)
        assertEquals(VaultText(R.string.vault_locked_timer), f.model.notice)
        // Nothing opens while locked.
        f.holder.add()
        f.holder.askChangePassword()
        f.holder.openSettings()
        assertNull(f.model.dialog)
    }

    @Test
    fun aPasswordThatArrivesAfterTheLockIsDropped() = runTest {
        val f = opened()
        f.holder.open(f.model.items.first().id)
        runCurrent()
        f.holder.reveal()
        // The lock comes before the reveal's coroutine ran.
        f.port.lock(LockReason.MANUAL)
        runCurrent()
        assertNull(f.model.dialog)
        assertEquals(0, f.port.backend.reveals)
    }

    @Test
    fun copyingFetchesThePasswordWithoutShowingIt() = runTest {
        val f = opened()
        f.holder.open(f.model.items.last().id)
        runCurrent()
        var copied: String? = null
        f.holder.withPassword { copied = it }
        runCurrent()
        assertEquals("s3cret-grace", copied)
        assertNull(f.entry?.revealed)
    }

    // ----- add, edit, delete -----

    @Test
    fun aRefusedEntryNamesItsField() = runTest {
        val f = opened()
        f.holder.add()
        f.holder.save("not an address", "u", "p", "")
        runCurrent()
        assertEquals(EntryField.SITE, f.editor?.errorField)
        assertEquals(VaultText(R.string.vault_error_invalid_site), f.editor?.error)
        assertFalse(f.editor?.busy ?: true)

        f.holder.save("https://new.example", "u", "", "")
        runCurrent()
        assertEquals(EntryField.PASSWORD, f.editor?.errorField)

        f.holder.save("new.example/login", "u", "p", "a note")
        runCurrent()
        assertNull(f.model.dialog)
        assertEquals(VaultText(R.string.vault_saved), f.model.notice)
        assertEquals(3, f.model.items.size)
        assertTrue(f.model.items.any { it.origin == "https://new.example" && it.hasNote })
    }

    @Test
    fun editingWithAnEmptyPasswordKeepsTheStoredOne() = runTest {
        val f = opened()
        val id = f.model.items.first().id
        f.holder.open(id)
        runCurrent()
        f.holder.edit()
        assertEquals("ada", f.editor?.username)
        f.holder.save("https://example.com", "ada.l", "", "changed")
        runCurrent()
        // Back on the entry, with the new values and nothing revealed.
        assertEquals("ada.l", f.entry?.detail?.username)
        assertEquals("changed", f.entry?.detail?.note)
        assertNull(f.entry?.revealed)
        f.holder.reveal()
        runCurrent()
        assertEquals("s3cret-ada", f.entry?.revealed?.value)
    }

    @Test
    fun cancellingTheEditorGoesBackToTheEntry() = runTest {
        val f = opened()
        f.holder.open(f.model.items.first().id)
        runCurrent()
        f.holder.edit()
        f.holder.backToEntry()
        runCurrent()
        assertNotNull(f.entry)
        f.holder.dismissDialog()
        f.holder.add()
        f.holder.backToEntry()
        assertNull(f.model.dialog)
    }

    @Test
    fun deletingNeedsTheConfirmation() = runTest {
        val f = opened()
        f.holder.open(f.model.items.first().id)
        runCurrent()
        // Not asked yet: nothing happens.
        f.holder.confirmDelete()
        runCurrent()
        assertEquals(2, f.model.items.size)

        f.holder.askDelete()
        val asked = f.model.dialog as? VaultDialog.ConfirmDelete
        assertEquals("https://example.com", asked?.detail?.origin)
        assertEquals("ada", asked?.detail?.username)
        f.holder.backToEntry()
        assertNotNull(f.entry)
        assertEquals(2, f.model.items.size)

        f.holder.askDelete()
        f.holder.confirmDelete()
        runCurrent()
        assertNull(f.model.dialog)
        assertEquals(listOf("grace"), f.model.items.map { it.username })
        assertEquals(VaultText(R.string.vault_deleted), f.model.notice)
    }

    // ----- password, settings -----

    @Test
    fun changingThePasswordChecksTheNewOneAndTheOldOne() = runTest {
        val f = opened()
        f.holder.askChangePassword()
        fun dialog() = f.model.dialog as? VaultDialog.ChangePassword

        f.holder.changePassword(PASSWORD, "short", "short")
        assertEquals(VaultText(R.string.vault_error_too_short, 8), dialog()?.error)
        f.holder.changePassword(PASSWORD, "long enough 1", "long enough 2")
        assertEquals(VaultText(R.string.vault_error_mismatch), dialog()?.error)
        f.holder.changePassword("not the old one", "long enough 1", "long enough 1")
        runCurrent()
        assertEquals(VaultText(R.string.vault_error_wrong_password), dialog()?.error)

        f.holder.changePassword(PASSWORD, "long enough 1", "long enough 1")
        runCurrent()
        assertNull(f.model.dialog)
        assertEquals(VaultText(R.string.vault_password_changed), f.model.notice)
        f.holder.lockNow()
        runCurrent()
        f.holder.unlock.submit("long enough 1")
        runCurrent()
        assertEquals(VaultState.UNLOCKED, f.model.vault)
    }

    @Test
    fun theLockChoicesAcceptOnlyTheirValues() = runTest {
        val f = opened()
        assertEquals(15, f.port.lockMinutes.value)
        assertEquals(60, f.port.backgroundGraceSeconds.value)
        for (minutes in LOCK_MINUTE_CHOICES) {
            f.holder.setLockMinutes(minutes)
            assertEquals(minutes, f.port.lockMinutes.value)
        }
        f.holder.setLockMinutes(7)
        assertEquals(60, f.port.lockMinutes.value)

        for (seconds in BACKGROUND_GRACE_CHOICES) {
            f.holder.setBackgroundGraceSeconds(seconds)
            assertEquals(seconds, f.port.backgroundGraceSeconds.value)
        }
        f.holder.setBackgroundGraceSeconds(0)
        f.holder.setBackgroundGraceSeconds(45)
        assertEquals(0, f.port.backgroundGraceSeconds.value)
    }

    @Test
    fun theGraceChoicesHaveTheirOwnLabels() {
        assertEquals(listOf(0, 30, 60, 300), BACKGROUND_GRACE_CHOICES)
        assertEquals(
            listOf(R.string.vault_grace_0, R.string.vault_grace_30, R.string.vault_grace_60, R.string.vault_grace_300),
            BACKGROUND_GRACE_CHOICES.map(::graceLabel),
        )
    }

    @Test
    fun theSettingsOpenWithoutAVaultAndOfferToSaveIsOnUntilSwitchedOff() = runTest {
        val f = fixture()
        assertEquals(VaultState.NONE, f.model.vault)
        f.holder.openSettings()
        assertEquals(VaultDialog.Settings, f.model.dialog)
        assertTrue(f.port.offerToSave.value)
        f.holder.setOfferToSave(false)
        assertFalse(f.port.offerToSave.value)
        f.holder.setBackgroundGraceSeconds(300)
        assertEquals(300, f.port.backgroundGraceSeconds.value)
    }

    @Test
    fun switchingFingerprintUnlockOnAndOff() = runTest {
        val f = opened()
        f.holder.setBiometric(true) { f.port.biometric.enableNow() }
        runCurrent()
        assertTrue(f.port.biometric.enabled.value)
        f.holder.setBiometric(false) { error("not asked for") }
        assertFalse(f.port.biometric.enabled.value)

        // The prompt was cancelled or the key store refused: said, and still off.
        f.port.biometric.available = BioAvailability.NONE_ENROLLED
        f.holder.setBiometric(true) { f.port.biometric.enableNow() }
        runCurrent()
        assertFalse(f.port.biometric.enabled.value)
        assertEquals(VaultText(R.string.vault_bio_enable_failed), f.model.notice)
    }

    // ----- key lost -----

    @Test
    fun aVaultWithALostKeyIsDeletedOnlyAfterTheTypedWord() = runTest {
        val f = opened()
        f.holder.lockNow()
        f.port.keyLost = true
        runCurrent()
        assertEquals(VaultState.KEY_LOST, f.model.vault)

        // Not asked yet: nothing happens.
        f.holder.destroy(DESTROY_WORD)
        runCurrent()
        assertEquals(VaultState.KEY_LOST, f.port.state.value)

        f.holder.askDestroy()
        for (wrong in listOf("", "delete", "DELETE it", "yes")) {
            f.holder.destroy(wrong)
            runCurrent()
            assertEquals(VaultState.KEY_LOST, f.port.state.value)
            assertEquals(VaultDialog.ConfirmDestroy(refused = true), f.model.dialog)
        }

        f.holder.destroy(" DELETE ")
        runCurrent()
        assertEquals(VaultState.NONE, f.model.vault)
        assertNull(f.model.dialog)
        assertEquals(VaultText(R.string.vault_destroyed), f.model.notice)
    }

    @Test
    fun theDeleteQuestionIsOnlyForALostKey() = runTest {
        val f = opened()
        f.holder.askDestroy()
        assertNull(f.model.dialog)
        f.holder.lockNow()
        runCurrent()
        f.holder.askDestroy()
        assertNull(f.model.dialog)
    }

    private companion object {
        const val PASSWORD = "correct horse"
    }
}
