package org.krunk.browser.vault

import android.app.Application
import java.io.File

/** Where the app gets its [VaultBackend] from. */
object VaultBackends {
    /**
     * The backend for this install: the shared core's vault (`core::vault` through UniFFI), its
     * files under `VaultFiles.dir(app.filesDir)`, its key wrapped by this device's Keystore key.
     */
    fun create(app: Application): VaultBackend =
        FfiVaultBackend(VaultFiles.dir(app.filesDir), FfiKeyWrapper(KeystoreKeyWrapper.onDevice()))
}

/** No vault at all: there is none, and none can be made (for tests and previews). */
class UnavailableVaultBackend(val dir: File) : VaultBackend {
    private fun unavailable(): Nothing = throw VaultFailure.Io("vault not built in")

    override fun state(): VaultFileState = VaultFileState.NONE

    override fun create(password: String, now: Long) = unavailable()

    override fun unlock(password: String, now: Long) = unavailable()

    override fun retryWaitMs(now: Long): Long = 0

    override fun unlockWithKey(key: ByteArray) = unavailable()

    override fun unlockKey(): ByteArray = unavailable()

    override fun lock() = Unit

    override fun changePassword(old: String, new: String) = unavailable()

    override fun search(query: String): List<VaultItem> = unavailable()

    override fun entry(id: String): VaultItemDetail = unavailable()

    override fun reveal(id: String): String = unavailable()

    override fun saveLogin(url: String, username: String, password: String, now: Long): String = unavailable()

    override fun upsert(
        id: String?,
        url: String,
        username: String,
        password: String?,
        note: String?,
        now: Long,
    ): String = unavailable()

    override fun delete(id: String) = unavailable()

    override fun matchesFor(url: String): List<VaultMatch> = unavailable()

    override fun forHttpAuth(url: String): List<VaultMatch> = unavailable()

    override fun secretForFill(id: String, pageUrl: String): VaultSecret = unavailable()

    override fun saveDecision(url: String, username: String, password: String): SaveDecision = unavailable()

    // The calls below are documented as never failing; with no vault there is nothing to do.
    override fun hasHint(url: String): Boolean = false

    override fun bioBlob(): ByteArray? = null

    override fun setBioBlob(blob: ByteArray?) = Unit

    override fun touch(now: Long) = Unit

    override fun autolockDue(now: Long, minutes: Int): Boolean = false

    override fun destroy() = Unit

    override fun minPasswordLength(): Int = 8
}
