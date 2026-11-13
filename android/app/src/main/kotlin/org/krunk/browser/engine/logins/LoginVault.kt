package org.krunk.browser.engine.logins

import org.krunk.browser.vault.VaultBackend

/** What the engine side needs of the vault and of the settings around it. */
interface LoginVault {
    enum class State { NONE, LOCKED, UNLOCKED, KEY_LOST }

    fun state(): State

    /** Works while locked: does the site of [url] have logins? */
    fun hasHint(url: String): Boolean

    /** Runs [block] off the main thread on the open vault; `VaultFailure.Locked` otherwise. */
    suspend fun <T> use(block: (VaultBackend) -> T): T

    /** "Never for this site". */
    fun neverSave(origin: String): Boolean

    fun addNeverSave(origin: String)

    /** The setting "Offer to save passwords". */
    fun offersEnabled(): Boolean

    fun setOffersEnabled(on: Boolean)
}
