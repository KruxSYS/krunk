package org.krunk.browser.vault

import android.util.Log
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map
import org.krunk.browser.KrunkApp
import org.krunk.browser.engine.logins.LoginVault
import org.krunk.browser.engine.originOf
import org.krunk.ffi.FfiException
import org.krunk.ffi.KrunkStore

/**
 * The vault as the engine side sees it: the controller, the "never for this site" list of the
 * shared database, and the switch "Offer to save sign-ins".
 */
class AppLoginVault(
    private val controller: VaultController,
    private val store: KrunkStore,
    private val now: () -> Long,
) : LoginVault {
    override fun state(): LoginVault.State = when (controller.state.value) {
        VaultState.NONE -> LoginVault.State.NONE
        VaultState.LOCKED -> LoginVault.State.LOCKED
        VaultState.UNLOCKED -> LoginVault.State.UNLOCKED
        VaultState.KEY_LOST -> LoginVault.State.KEY_LOST
    }

    override fun hasHint(url: String): Boolean = controller.hasHint(url)

    override suspend fun <T> use(block: (VaultBackend) -> T): T = controller.use(block)

    override suspend fun <T> peek(block: (VaultBackend) -> T): T = controller.peek(block)

    override val locked: Flow<Unit> = controller.lockEvents.map { }

    override fun neverSave(origin: String): Boolean = try {
        originOf(origin)?.let { store.neverSave(it) } ?: false
    } catch (e: FfiException) {
        false
    }

    override fun addNeverSave(origin: String) {
        try {
            originOf(origin)?.let { store.neverSaveAdd(it, now()) }
        } catch (e: FfiException) {
            Log.w(KrunkApp.TAG, "vault: never-save not stored: ${e.javaClass.simpleName}")
        }
    }

    override fun offersEnabled(): Boolean = try {
        store.kvGet(KV_OFFER) != "0"
    } catch (e: FfiException) {
        true
    }

    override fun setOffersEnabled(on: Boolean) {
        try {
            store.kvSet(KV_OFFER, if (on) "1" else "0")
        } catch (e: FfiException) {
            Log.w(KrunkApp.TAG, "vault: offer setting not stored: ${e.javaClass.simpleName}")
        }
    }

    companion object {
        const val KV_OFFER = "android.vault.offer_save"
    }
}
