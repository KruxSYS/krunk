package org.krunk.browser.vault

/**
 * The vault as the app sees it. The real one is the shared core through UniFFI
 * ([FfiVaultBackend]); tests use a fake. Every call may block (file access, and for [create],
 * [unlock] and [changePassword] the password derivation, about a second): call off the main
 * thread.
 *
 * Passwords cross this interface only where a user action asked for one: [reveal],
 * [secretForFill], and inwards on create, unlock, change and save. Lists never carry them.
 */
interface VaultBackend {
    fun state(): VaultFileState

    @Throws(VaultFailure::class)
    fun create(password: String, now: Long)

    @Throws(VaultFailure::class)
    fun unlock(password: String, now: Long)

    /** Milliseconds until the next password attempt is allowed (0 = now). */
    fun retryWaitMs(now: Long): Long

    /** Biometric path: the unlock key as [unlockKey] gave it. */
    @Throws(VaultFailure::class)
    fun unlockWithKey(key: ByteArray)

    @Throws(VaultFailure::class)
    fun unlockKey(): ByteArray

    /** Drops the open vault; its secrets are wiped on the Rust side. */
    fun lock()

    @Throws(VaultFailure::class)
    fun changePassword(old: String, new: String)

    @Throws(VaultFailure::class)
    fun search(query: String): List<VaultItem>

    @Throws(VaultFailure::class)
    fun entry(id: String): VaultItemDetail

    @Throws(VaultFailure::class)
    fun reveal(id: String): String

    @Throws(VaultFailure::class)
    fun saveLogin(url: String, username: String, password: String, now: Long): String

    @Throws(VaultFailure::class)
    fun upsert(id: String?, url: String, username: String, password: String?, note: String?, now: Long): String

    @Throws(VaultFailure::class)
    fun delete(id: String)

    /** Logins for a page: exact origin, else the same registrable domain (https only). */
    @Throws(VaultFailure::class)
    fun matchesFor(url: String): List<VaultMatch>

    /** Logins for an HTTP sign-in dialog: exact origin only. */
    @Throws(VaultFailure::class)
    fun forHttpAuth(url: String): List<VaultMatch>

    /** The login to fill, re-checked against the page's address ([VaultFailure.NotAllowed]). */
    @Throws(VaultFailure::class)
    fun secretForFill(id: String, pageUrl: String): VaultSecret

    @Throws(VaultFailure::class)
    fun saveDecision(url: String, username: String, password: String): SaveDecision

    /** Works while locked: does this site have logins? Never throws. */
    fun hasHint(url: String): Boolean

    fun bioBlob(): ByteArray?

    fun setBioBlob(blob: ByteArray?)

    fun touch(now: Long)

    fun autolockDue(now: Long, minutes: Int): Boolean

    /** Removes the vault file and its side file (the "device key lost" way out). */
    fun destroy()

    fun minPasswordLength(): Int
}

enum class VaultFileState { NONE, LOCKED, UNLOCKED }

data class VaultItem(
    val id: String,
    val origin: String,
    val username: String,
    val hasNote: Boolean,
    val created: Long,
    val updated: Long,
)

data class VaultItemDetail(
    val id: String,
    val origin: String,
    val username: String,
    val note: String,
    val created: Long,
    val updated: Long,
)

data class VaultMatch(val id: String, val origin: String, val username: String, val exact: Boolean)

/** A login on its way into a page or dialog. Not a data class: no `toString` with the password. */
class VaultSecret(val username: String, val password: String)

sealed class SaveDecision {
    data object New : SaveDecision()

    data class Update(val id: String) : SaveDecision()

    data object Unchanged : SaveDecision()
}

/** Why a vault call failed. None of the texts carries a secret. */
sealed class VaultFailure(detail: String) : Exception(detail) {
    class WrongPassword : VaultFailure("wrong password")

    class RateLimited(val waitMs: Long) : VaultFailure("too many attempts")

    class WeakPassword : VaultFailure("password too short")

    /** The Keystore key that wraps the vault key is gone or unusable on this device. */
    class KeyLost(detail: String) : VaultFailure(detail)

    class Corrupt(detail: String) : VaultFailure(detail)

    class Locked : VaultFailure("vault is locked")

    class NoVault : VaultFailure("no vault")

    class AlreadyExists : VaultFailure("a vault exists already")

    class NotFound : VaultFailure("no such entry")

    class NotAllowed : VaultFailure("this login does not belong to the page")

    class InvalidEntry(detail: String) : VaultFailure(detail)

    class Io(detail: String) : VaultFailure(detail)
}
