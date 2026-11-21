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

    /**
     * Like [use] for what the engine asks by itself (a page with a sign-in form was loaded, a
     * form was sent): not a use of the vault, so it does not delay the inactivity lock.
     */
    suspend fun <T> peek(block: (VaultBackend) -> T): T

    /** Emits each time the vault locks: open pickers and dialogs drop what they show of it. */
    val locked: kotlinx.coroutines.flow.Flow<Unit>

    /** "Never for this site". */
    fun neverSave(origin: String): Boolean

    fun addNeverSave(origin: String)

    /** The setting "Offer to save passwords". */
    fun offersEnabled(): Boolean

    fun setOffersEnabled(on: Boolean)

    /** The setting "Suggest strong passwords" (Android 0.13.0). */
    fun suggestEnabled(): Boolean

    fun setSuggestEnabled(on: Boolean)
}
