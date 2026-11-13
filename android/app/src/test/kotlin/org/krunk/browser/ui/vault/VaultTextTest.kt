package org.krunk.browser.ui.vault

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.browser.R
import org.krunk.browser.vault.BioAvailability
import org.krunk.browser.vault.BioOffReason
import org.krunk.browser.vault.VaultFailure

class VaultTextTest {
    @Test
    fun everyFailureHasItsSentence() {
        val expected = listOf(
            VaultFailure.WrongPassword() to R.string.vault_error_wrong_password,
            VaultFailure.RateLimited(1) to R.string.vault_error_rate_limited,
            VaultFailure.WeakPassword() to R.string.vault_error_weak_password,
            VaultFailure.KeyLost("x") to R.string.vault_error_key_lost,
            VaultFailure.Corrupt("x") to R.string.vault_error_corrupt,
            VaultFailure.Io("x") to R.string.vault_error_io,
            VaultFailure.NotFound() to R.string.vault_error_not_found,
            VaultFailure.InvalidEntry("origin") to R.string.vault_error_invalid_site,
            VaultFailure.Locked() to R.string.vault_error_locked,
            VaultFailure.NoVault() to R.string.vault_error_no_vault,
            VaultFailure.AlreadyExists() to R.string.vault_error_exists,
            VaultFailure.NotAllowed() to R.string.vault_error_not_allowed,
        )
        for ((failure, id) in expected) assertEquals(failure.toString(), id, failureText(failure).id)
        // No two failures share a sentence, and none falls through to the general one.
        assertEquals(expected.size, expected.map { it.second }.toSet().size)
        assertFalse(expected.any { it.second == R.string.vault_error_other })
        assertEquals(VaultText(R.string.vault_error_other), failureText(IllegalStateException("x")))
    }

    @Test
    fun theWaitIsSaidInWholeSecondsRoundedUp() {
        assertEquals(VaultText(R.string.vault_error_rate_limited, 5), failureText(VaultFailure.RateLimited(4_200)))
        assertEquals(VaultText(R.string.vault_error_rate_limited, 5), failureText(VaultFailure.RateLimited(5_000)))
        assertEquals(VaultText(R.string.vault_error_rate_limited, 1), failureText(VaultFailure.RateLimited(1)))
        // A failure that says "wait" is never shown as "0 s".
        assertEquals(VaultText(R.string.vault_error_rate_limited, 1), failureText(VaultFailure.RateLimited(0)))

        assertEquals(0, waitSeconds(0))
        assertEquals(0, waitSeconds(-5))
        assertEquals(1, waitSeconds(1))
        assertEquals(1, waitSeconds(1_000))
        assertEquals(2, waitSeconds(1_001))
        assertEquals(Int.MAX_VALUE, waitSeconds(Long.MAX_VALUE))
        assertNull(rateLimitText(0))
        assertEquals(VaultText(R.string.vault_error_rate_limited, 30), rateLimitText(29_001))
    }

    @Test
    fun aRefusedEntryIsPutToItsField() {
        // The core's own words.
        assertEquals(EntryField.SITE, invalidEntryField("origin"))
        assertEquals(EntryField.PASSWORD, invalidEntryField("password"))
        assertEquals(EntryField.PASSWORD, invalidEntryField("password length"))
        assertEquals(EntryField.USERNAME, invalidEntryField("username too long"))
        assertEquals(EntryField.NOTE, invalidEntryField("note too long"))
        assertEquals(EntryField.SITE, invalidEntryField(null))
        assertEquals(EntryField.SITE, invalidEntryField("something new"))

        assertEquals(
            R.string.vault_error_invalid_password,
            failureText(VaultFailure.InvalidEntry("password length")).id,
        )
        assertEquals(
            R.string.vault_error_invalid_username,
            failureText(VaultFailure.InvalidEntry("username too long")).id,
        )
        assertEquals(R.string.vault_error_invalid_note, failureText(VaultFailure.InvalidEntry("note too long")).id)
    }

    @Test
    fun aNewPasswordIsCheckedForLengthFirstThenForTheRepetition() {
        assertEquals(VaultText(R.string.vault_error_too_short, 8), newPasswordError("", "", 8))
        assertEquals(VaultText(R.string.vault_error_too_short, 8), newPasswordError("1234567", "1234567", 8))
        assertEquals(VaultText(R.string.vault_error_mismatch), newPasswordError("12345678", "1234567", 8))
        assertEquals(VaultText(R.string.vault_error_mismatch), newPasswordError("12345678", "12345678 ", 8))
        assertNull(newPasswordError("12345678", "12345678", 8))
    }

    @Test
    fun onlyTheWordItselfConfirmsDeletingTheVault() {
        assertTrue(destroyConfirmed("DELETE"))
        assertTrue(destroyConfirmed(" DELETE\n"))
        for (wrong in listOf("", "delete", "Delete", "DELETE!", "DELET", "DELETE DELETE")) {
            assertFalse(wrong, destroyConfirmed(wrong))
        }
    }

    @Test
    fun theFingerprintRowFollowsWhatTheDeviceCanDo() {
        assertEquals(BioRow.Switch, bioRow(BioAvailability.AVAILABLE))
        assertEquals(BioRow.Hidden, bioRow(BioAvailability.NO_HARDWARE))
        assertEquals(
            BioRow.Unavailable(VaultText(R.string.vault_bio_none_enrolled)),
            bioRow(BioAvailability.NONE_ENROLLED),
        )
        assertEquals(
            BioRow.Unavailable(VaultText(R.string.vault_bio_unsupported_os)),
            bioRow(BioAvailability.UNSUPPORTED_OS),
        )
        assertEquals(BioRow.Unavailable(VaultText(R.string.vault_bio_unavailable)), bioRow(BioAvailability.UNAVAILABLE))
    }

    @Test
    fun everyReasonFingerprintUnlockWentOffHasItsSentence() {
        assertNull(bioOffText(null))
        assertEquals(VaultText(R.string.vault_bio_off_enrollment), bioOffText(BioOffReason.ENROLLMENT_CHANGED))
        assertEquals(VaultText(R.string.vault_bio_off_password), bioOffText(BioOffReason.PASSWORD_CHANGED))
        assertEquals(VaultText(R.string.vault_bio_off_missing), bioOffText(BioOffReason.BLOB_MISSING))
        assertEquals(VaultText(R.string.vault_bio_off_missing), bioOffText(BioOffReason.KEY_MISSING))
    }

    @Test
    fun aSecretDoesNotPrintItself() {
        val secret = Secret("hunter2")
        assertFalse(secret.toString().contains("hunter2"))
        assertFalse(VaultDialog.Entry(detail(), secret).toString().contains("hunter2"))
    }

    private fun detail() = org.krunk.browser.vault.VaultItemDetail("1", "https://example.com", "ada", "", 0, 0)
}
