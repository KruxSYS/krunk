package org.krunk.browser.ui.vault

import androidx.annotation.StringRes
import kotlin.coroutines.cancellation.CancellationException
import org.krunk.browser.R
import org.krunk.browser.vault.BioAvailability
import org.krunk.browser.vault.BioOffReason
import org.krunk.browser.vault.VaultFailure

/** A sentence for the user: a string resource and, where it has a number in it, that number. */
data class VaultText(@param:StringRes val id: Int, val arg: Int? = null)

/** Which field of the entry editor a refused entry is about. */
enum class EntryField { SITE, USERNAME, PASSWORD, NOTE }

/** The word the user types before a vault whose key is lost is deleted. */
const val DESTROY_WORD = "DELETE"

/** Whole seconds left of a wait, rounded up: 1 ms left is still "1 s". */
fun waitSeconds(waitMs: Long): Int {
    if (waitMs <= 0) return 0
    val whole = waitMs / 1000 + if (waitMs % 1000 == 0L) 0 else 1
    return whole.coerceAtMost(Int.MAX_VALUE.toLong()).toInt()
}

/** The countdown line of the unlock form, or null when a password may be tried now. */
fun rateLimitText(waitMs: Long): VaultText? =
    waitSeconds(waitMs).takeIf { it > 0 }?.let { VaultText(R.string.vault_error_rate_limited, it) }

/**
 * Which field the core refused. Its details are fixed words ("origin", "password", "password
 * length", "username too long", "note too long"); anything unknown is put to the address, the
 * field with the most rules.
 */
fun invalidEntryField(detail: String?): EntryField {
    val d = detail.orEmpty().lowercase()
    return when {
        "password" in d -> EntryField.PASSWORD
        "username" in d -> EntryField.USERNAME
        "note" in d -> EntryField.NOTE
        else -> EntryField.SITE
    }
}

/** Every way a vault call fails, as a sentence. None of them carries the failure's own text. */
fun failureText(failure: Throwable): VaultText = when (failure) {
    is VaultFailure.WrongPassword -> VaultText(R.string.vault_error_wrong_password)

    is VaultFailure.RateLimited -> VaultText(
        R.string.vault_error_rate_limited,
        waitSeconds(failure.waitMs).coerceAtLeast(1),
    )

    is VaultFailure.WeakPassword -> VaultText(R.string.vault_error_weak_password)

    is VaultFailure.KeyLost -> VaultText(R.string.vault_error_key_lost)

    is VaultFailure.Corrupt -> VaultText(R.string.vault_error_corrupt)

    is VaultFailure.Io -> VaultText(R.string.vault_error_io)

    is VaultFailure.NotFound -> VaultText(R.string.vault_error_not_found)

    is VaultFailure.InvalidEntry -> when (invalidEntryField(failure.message)) {
        EntryField.SITE -> VaultText(R.string.vault_error_invalid_site)
        EntryField.USERNAME -> VaultText(R.string.vault_error_invalid_username)
        EntryField.PASSWORD -> VaultText(R.string.vault_error_invalid_password)
        EntryField.NOTE -> VaultText(R.string.vault_error_invalid_note)
    }

    is VaultFailure.Locked -> VaultText(R.string.vault_error_locked)

    is VaultFailure.NoVault -> VaultText(R.string.vault_error_no_vault)

    is VaultFailure.AlreadyExists -> VaultText(R.string.vault_error_exists)

    is VaultFailure.NotAllowed -> VaultText(R.string.vault_error_not_allowed)

    else -> VaultText(R.string.vault_error_other)
}

/** A new vault password and its repetition: the first thing wrong with them, or null. */
fun newPasswordError(password: String, repeat: String, minLength: Int): VaultText? = when {
    password.length < minLength -> VaultText(R.string.vault_error_too_short, minLength)
    password != repeat -> VaultText(R.string.vault_error_mismatch)
    else -> null
}

/** Whether what the user typed allows deleting the vault: the word itself, nothing looser. */
fun destroyConfirmed(typed: String): Boolean = typed.trim() == DESTROY_WORD

/** Why the unlock form has no fingerprint button any more, or null when nothing was switched off. */
fun bioOffText(reason: BioOffReason?): VaultText? = when (reason) {
    null -> null
    BioOffReason.ENROLLMENT_CHANGED -> VaultText(R.string.vault_bio_off_enrollment)
    BioOffReason.PASSWORD_CHANGED -> VaultText(R.string.vault_bio_off_password)
    BioOffReason.BLOB_MISSING, BioOffReason.KEY_MISSING -> VaultText(R.string.vault_bio_off_missing)
}

/** The fingerprint row of the vault settings: a switch, a reason instead of it, or no row. */
sealed class BioRow {
    data object Switch : BioRow()

    data class Unavailable(val reason: VaultText) : BioRow()

    data object Hidden : BioRow()
}

fun bioRow(availability: BioAvailability): BioRow = when (availability) {
    BioAvailability.AVAILABLE -> BioRow.Switch
    BioAvailability.NONE_ENROLLED -> BioRow.Unavailable(VaultText(R.string.vault_bio_none_enrolled))
    BioAvailability.UNSUPPORTED_OS -> BioRow.Unavailable(VaultText(R.string.vault_bio_unsupported_os))
    BioAvailability.UNAVAILABLE -> BioRow.Unavailable(VaultText(R.string.vault_bio_unavailable))
    BioAvailability.NO_HARDWARE -> BioRow.Hidden
}

/** The label of one choice of "Lock after leaving Krunk". */
@StringRes
fun graceLabel(seconds: Int): Int = when {
    seconds <= 0 -> R.string.vault_grace_0
    seconds <= 30 -> R.string.vault_grace_30
    seconds <= 60 -> R.string.vault_grace_60
    else -> R.string.vault_grace_300
}

/** Runs [block]; what it threw, or null. Cancellation is not a failure and goes on up. */
internal suspend fun failureOf(block: suspend () -> Unit): Throwable? = try {
    block()
    null
} catch (e: CancellationException) {
    throw e
} catch (e: Exception) {
    e
}
