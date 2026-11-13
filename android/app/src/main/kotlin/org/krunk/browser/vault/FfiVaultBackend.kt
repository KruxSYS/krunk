package org.krunk.browser.vault

import java.io.File
import org.krunk.ffi.KrunkVault
import org.krunk.ffi.SaveDecision as FfiSaveDecision
import org.krunk.ffi.VaultFfiException
import org.krunk.ffi.VaultKeyWrapper
import org.krunk.ffi.VaultState as FfiVaultState
import org.krunk.ffi.WrapException
import org.krunk.ffi.vaultMinPasswordLen

/**
 * The Keystore wrapper as the shared core calls it. The core hands over the vault key already
 * sealed with the password's key; this adds the device's key on top.
 */
class FfiKeyWrapper(private val keys: KeystoreKeyWrapper) : VaultKeyWrapper {
    override fun protect(data: ByteArray): ByteArray = mapped { keys.protect(data) }

    override fun unprotect(data: ByteArray): ByteArray = mapped { keys.unprotect(data) }

    private inline fun mapped(block: () -> ByteArray): ByteArray = try {
        block()
    } catch (e: WrapFailure) {
        throw when (e.kind) {
            WrapFailureKind.MISSING -> WrapException.KeyMissing(e.detail)
            WrapFailureKind.INVALIDATED -> WrapException.KeyInvalidated(e.detail)
            WrapFailureKind.FAILED -> WrapException.Failed(e.detail)
        }
    }
}

/** The vault of the shared core (`core::vault` through `ffi/src/vault.rs`). */
class FfiVaultBackend(dir: File, wrapper: VaultKeyWrapper) : VaultBackend {
    private val vault = KrunkVault(dir.absolutePath, wrapper)

    private inline fun <T> call(block: () -> T): T = try {
        block()
    } catch (e: VaultFfiException) {
        throw failure(e)
    }

    override fun state(): VaultFileState = when (vault.state()) {
        FfiVaultState.NONE -> VaultFileState.NONE
        FfiVaultState.LOCKED -> VaultFileState.LOCKED
        FfiVaultState.UNLOCKED -> VaultFileState.UNLOCKED
    }

    override fun create(password: String, now: Long) = call { vault.create(password, now) }

    override fun unlock(password: String, now: Long) = call { vault.unlock(password, now) }

    override fun retryWaitMs(now: Long): Long = vault.retryWaitMs(now).toLong()

    override fun unlockWithKey(key: ByteArray) = call { vault.unlockWithKey(key) }

    override fun unlockKey(): ByteArray = call { vault.unlockKey() }

    override fun lock() = vault.lock()

    override fun changePassword(old: String, new: String) = call { vault.changePassword(old, new) }

    override fun search(query: String): List<VaultItem> = call {
        vault.search(query).map { VaultItem(it.id, it.origin, it.username, it.hasNote, it.created, it.updated) }
    }

    override fun entry(id: String): VaultItemDetail = call {
        vault.entry(id).let { VaultItemDetail(it.id, it.origin, it.username, it.note, it.created, it.updated) }
    }

    override fun reveal(id: String): String = call { vault.reveal(id) }

    override fun saveLogin(url: String, username: String, password: String, now: Long): String =
        call { vault.saveLogin(url, username, password, now) }

    override fun upsert(
        id: String?,
        url: String,
        username: String,
        password: String?,
        note: String?,
        now: Long,
    ): String = call { vault.upsert(id, url, username, password, note, now) }

    override fun delete(id: String) = call { vault.delete(id) }

    override fun matchesFor(url: String): List<VaultMatch> =
        call { vault.matchesFor(url).map { VaultMatch(it.id, it.origin, it.username, it.exact) } }

    override fun forHttpAuth(url: String): List<VaultMatch> =
        call { vault.forHttpAuth(url).map { VaultMatch(it.id, it.origin, it.username, it.exact) } }

    override fun secretForFill(id: String, pageUrl: String): VaultSecret =
        call { vault.secretForFill(id, pageUrl).let { VaultSecret(it.username, it.password) } }

    override fun saveDecision(url: String, username: String, password: String): SaveDecision = call {
        when (val d = vault.saveDecision(url, username, password)) {
            is FfiSaveDecision.New -> SaveDecision.New
            is FfiSaveDecision.Update -> SaveDecision.Update(d.id)
            is FfiSaveDecision.Unchanged -> SaveDecision.Unchanged
        }
    }

    override fun hasHint(url: String): Boolean = vault.hasHint(url)

    override fun bioBlob(): ByteArray? = vault.bioBlob()

    override fun setBioBlob(blob: ByteArray?) {
        try {
            vault.setBioBlob(blob)
        } catch (e: VaultFfiException) {
            // Not written: fingerprint unlock then finds no blob and falls back to the password.
        }
    }

    override fun touch(now: Long) = vault.touch(now)

    override fun autolockDue(now: Long, minutes: Int): Boolean = vault.autolockDue(now, minutes.toUInt())

    override fun destroy() {
        try {
            vault.erase()
        } catch (e: VaultFfiException) {
            throw failure(e)
        }
    }

    override fun minPasswordLength(): Int = vaultMinPasswordLen().toInt()

    companion object {
        /** The core's error as the app's. No text of either carries a secret. */
        fun failure(e: VaultFfiException): VaultFailure = when (e) {
            is VaultFfiException.WrongPassword -> VaultFailure.WrongPassword()
            is VaultFfiException.RateLimited -> VaultFailure.RateLimited(e.waitMs.toLong())
            is VaultFfiException.WeakPassword -> VaultFailure.WeakPassword()
            is VaultFfiException.KeyLost -> VaultFailure.KeyLost(e.detail)
            is VaultFfiException.Corrupt -> VaultFailure.Corrupt(e.detail)
            is VaultFfiException.Locked -> VaultFailure.Locked()
            is VaultFfiException.NoVault -> VaultFailure.NoVault()
            is VaultFfiException.AlreadyExists -> VaultFailure.AlreadyExists()
            is VaultFfiException.NotFound -> VaultFailure.NotFound()
            is VaultFfiException.NotAllowed -> VaultFailure.NotAllowed()
            is VaultFfiException.InvalidEntry -> VaultFailure.InvalidEntry(e.detail)
            is VaultFfiException.Io -> VaultFailure.Io(e.detail)
        }
    }
}
