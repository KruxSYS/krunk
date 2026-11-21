package org.krunk.browser.ui.vault

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.krunk.browser.R
import org.krunk.browser.vault.BioAvailability
import org.krunk.browser.vault.LockReason
import org.krunk.browser.vault.VaultFailure
import org.krunk.browser.vault.VaultItem
import org.krunk.browser.vault.VaultItemDetail
import org.krunk.browser.vault.VaultState

/** A password on screen. Not a data class: no `toString` with the password, equal only to itself. */
class Secret(val value: String) {
    override fun toString(): String = "Secret"
}

/** What is open on top of the list. One at a time; a lock closes whichever it is. */
sealed class VaultDialog {
    /** One login. [revealed] is set only after "Show", and only for a while. */
    data class Entry(val detail: VaultItemDetail, val revealed: Secret? = null) : VaultDialog()

    /**
     * Add ([id] null) or edit. Holds what the fields start with; what the user types, the
     * password above all, stays in the dialog and goes when the dialog goes.
     */
    data class Editor(
        val id: String?,
        val origin: String = "",
        val username: String = "",
        val note: String = "",
        val error: VaultText? = null,
        val errorField: EntryField? = null,
        val busy: Boolean = false,
    ) : VaultDialog()

    data class ConfirmDelete(val detail: VaultItemDetail) : VaultDialog()

    data class ChangePassword(val error: VaultText? = null, val busy: Boolean = false) : VaultDialog()

    data object Settings : VaultDialog()

    /** Right after the vault was made: "Unlock with your fingerprint?" Asked once. */
    data object OfferBiometric : VaultDialog()

    /** KEY_LOST: the typed confirmation. [refused] after a wrong word. */
    data class ConfirmDestroy(val refused: Boolean = false, val busy: Boolean = false) : VaultDialog()
}

data class VaultUiModel(
    val vault: VaultState,
    /** Creating the vault. */
    val busy: Boolean = false,
    val createError: VaultText? = null,
    val query: String = "",
    val items: List<VaultItem> = emptyList(),
    /** False until the first list arrived, so "no logins yet" does not flash before it. */
    val loaded: Boolean = false,
    val dialog: VaultDialog? = null,
    /** A one-off line for the snackbar; [VaultUiState.noticeShown] takes it away. */
    val notice: VaultText? = null,
)

/**
 * The vault screen's logic, free of Compose and Android so it runs in JVM tests. Everything
 * here is in memory only and goes at a lock: the list, the search, an open entry with its
 * revealed password, an open editor. Passwords the user types never come to rest here; they
 * pass through as arguments.
 */
class VaultUiState(
    private val port: VaultPort,
    private val scope: CoroutineScope,
    private val clock: () -> Long = System::currentTimeMillis,
) {
    private val _model = MutableStateFlow(VaultUiModel(vault = port.state.value))
    val model: StateFlow<VaultUiModel> = _model.asStateFlow()

    /** The unlock form of the LOCKED state. */
    val unlock = UnlockUiState(port, scope)

    private var searchJob: Job? = null
    private var hideJob: Job? = null

    init {
        scope.launch { port.state.collect(::onVaultState) }
        scope.launch { port.lockEvents.collect(::onLock) }
    }

    // ----- state and lock -----

    private fun onVaultState(state: VaultState) {
        if (state == VaultState.UNLOCKED) {
            _model.update { it.copy(vault = state, busy = false, createError = null) }
            refresh()
        } else {
            clearSecrets()
            _model.update { VaultUiModel(vault = state, notice = it.notice) }
        }
    }

    private fun onLock(reason: LockReason) {
        clearSecrets()
        unlock.reset()
        if (reason == LockReason.TIMER) _model.update { it.copy(notice = VaultText(R.string.vault_locked_timer)) }
        // Unlocked again before the event was seen (or the state never showed the lock): the
        // list was dropped above and has to come back.
        if (port.state.value == VaultState.UNLOCKED) refresh()
    }

    /** Leaving the screen, and every lock: nothing of the vault's content stays. */
    fun clearSecrets() {
        searchJob?.cancel()
        hideJob?.cancel()
        _model.update { it.copy(query = "", items = emptyList(), loaded = false, dialog = null, busy = false) }
    }

    fun noticeShown() = _model.update { it.copy(notice = null) }

    fun dismissDialog() {
        hideJob?.cancel()
        _model.update { it.copy(dialog = null) }
    }

    // ----- create -----

    fun create(password: String, repeat: String) {
        val m = _model.value
        if (m.busy || m.vault != VaultState.NONE) return
        val problem = newPasswordError(password, repeat, port.minPasswordLength)
        if (problem != null) {
            _model.update { it.copy(createError = problem) }
            return
        }
        _model.update { it.copy(busy = true, createError = null) }
        scope.launch {
            val failure = failureOf { port.create(password) }
            val offer = failure == null && offersBiometric(port.biometric)
            _model.update {
                it.copy(
                    busy = false,
                    createError = failure?.let(::failureText),
                    dialog = if (offer &&
                        port.state.value == VaultState.UNLOCKED
                    ) {
                        VaultDialog.OfferBiometric
                    } else {
                        it.dialog
                    },
                )
            }
        }
    }

    /** Typing again takes the last error away. */
    fun createEdited() = _model.update { if (it.createError == null) it else it.copy(createError = null) }

    // ----- list and search -----

    fun setQuery(query: String) {
        if (_model.value.vault != VaultState.UNLOCKED) return
        _model.update { it.copy(query = query) }
        refresh()
    }

    fun refresh() {
        searchJob?.cancel()
        searchJob = scope.launch {
            val query = _model.value.query.trim()
            var found: List<VaultItem> = emptyList()
            val failure = failureOf { found = port.use { it.search(query) } }
            when (failure) {
                null -> _model.update { it.copy(items = found, loaded = true) }

                is VaultFailure.Locked -> Unit

                // the lock itself clears the list
                else -> notify(failureText(failure))
            }
        }
    }

    // ----- one login -----

    fun open(id: String) {
        scope.launch {
            var detail: VaultItemDetail? = null
            val failure = failureOf { detail = port.use { it.entry(id) } }
            val d = detail
            when {
                d != null -> showEntry(d)
                failure != null -> failed(failure)
            }
        }
    }

    private fun showEntry(detail: VaultItemDetail) {
        hideJob?.cancel()
        _model.update { if (it.vault == VaultState.UNLOCKED) it.copy(dialog = VaultDialog.Entry(detail)) else it }
    }

    /** "Show": only now is the password fetched; it hides itself after [REVEAL_MS]. */
    fun reveal() {
        val entry = _model.value.dialog as? VaultDialog.Entry ?: return
        if (entry.revealed != null) return
        scope.launch {
            var password: String? = null
            val failure = failureOf { password = port.use { it.reveal(entry.detail.id) } }
            val p = password
            if (p == null) {
                if (failure != null) failed(failure)
                return@launch
            }
            var shown = false
            _model.update {
                val open = it.dialog as? VaultDialog.Entry
                // Locked, closed or another entry by now: the password is dropped.
                shown = it.vault == VaultState.UNLOCKED && open?.detail?.id == entry.detail.id
                if (shown && open != null) it.copy(dialog = open.copy(revealed = Secret(p))) else it
            }
            if (shown) {
                hideJob?.cancel()
                hideJob = scope.launch {
                    delay(REVEAL_MS)
                    hideNow()
                }
            }
        }
    }

    fun hide() {
        hideJob?.cancel()
        hideNow()
    }

    private fun hideNow() = _model.update {
        val open = it.dialog as? VaultDialog.Entry
        if (open?.revealed == null) it else it.copy(dialog = open.copy(revealed = null))
    }

    /**
     * Fetches the open entry's password for a copy and hands it to [onPassword]; nothing of it
     * stays here.
     */
    fun withPassword(onPassword: (String) -> Unit) {
        val entry = _model.value.dialog as? VaultDialog.Entry ?: return
        scope.launch {
            var password: String? = null
            val failure = failureOf { password = port.use { it.reveal(entry.detail.id) } }
            val p = password
            if (p != null) {
                onPassword(p)
            } else if (failure != null) {
                failed(failure)
            }
        }
    }

    // ----- add and edit -----

    fun add() {
        if (_model.value.vault != VaultState.UNLOCKED) return
        hideJob?.cancel()
        _model.update { it.copy(dialog = VaultDialog.Editor(id = null)) }
    }

    fun edit() {
        val entry = _model.value.dialog as? VaultDialog.Entry ?: return
        hideJob?.cancel()
        val d = entry.detail
        _model.update { it.copy(dialog = VaultDialog.Editor(d.id, d.origin, d.username, d.note)) }
    }

    /** Back from the editor or the delete question to the entry they were opened from. */
    fun backToEntry() {
        when (val dialog = _model.value.dialog) {
            is VaultDialog.Editor -> if (dialog.id == null) dismissDialog() else open(dialog.id)
            is VaultDialog.ConfirmDelete -> showEntry(dialog.detail)
            else -> Unit
        }
    }

    /** An empty [password] keeps the stored one when editing; a new login needs one. */
    fun save(url: String, username: String, password: String, note: String) {
        val editor = _model.value.dialog as? VaultDialog.Editor ?: return
        if (editor.busy) return
        setEditor { it.copy(busy = true, error = null, errorField = null) }
        scope.launch {
            var saved: VaultItemDetail? = null
            val failure = failureOf {
                saved = port.use {
                    val id = it.upsert(editor.id, url.trim(), username, password.ifEmpty { null }, note, clock())
                    it.entry(id)
                }
            }
            val detail = saved
            when {
                detail != null -> {
                    _model.update {
                        if (it.dialog !is VaultDialog.Editor) {
                            it
                        } else {
                            it.copy(
                                dialog = if (editor.id == null) null else VaultDialog.Entry(detail),
                                notice = VaultText(R.string.vault_saved),
                            )
                        }
                    }
                    refresh()
                }

                failure is VaultFailure.InvalidEntry -> setEditor {
                    it.copy(busy = false, error = failureText(failure), errorField = invalidEntryField(failure.message))
                }

                failure != null -> setEditor { it.copy(busy = false, error = failureText(failure)) }
            }
        }
    }

    private fun setEditor(change: (VaultDialog.Editor) -> VaultDialog.Editor) = _model.update {
        val editor = it.dialog as? VaultDialog.Editor
        if (editor == null) it else it.copy(dialog = change(editor))
    }

    // ----- delete -----

    fun askDelete() {
        val entry = _model.value.dialog as? VaultDialog.Entry ?: return
        hideJob?.cancel()
        _model.update { it.copy(dialog = VaultDialog.ConfirmDelete(entry.detail)) }
    }

    /** Deletes only what the open question names: without [askDelete] first this does nothing. */
    fun confirmDelete() {
        val asked = _model.value.dialog as? VaultDialog.ConfirmDelete ?: return
        _model.update { it.copy(dialog = null) }
        scope.launch {
            val failure = failureOf { port.use { it.delete(asked.detail.id) } }
            if (failure == null) notify(VaultText(R.string.vault_deleted)) else failed(failure)
            refresh()
        }
    }

    // ----- lock, password, settings -----

    fun lockNow() = port.lock(LockReason.MANUAL)

    fun askChangePassword() {
        if (_model.value.vault != VaultState.UNLOCKED) return
        hideJob?.cancel()
        _model.update { it.copy(dialog = VaultDialog.ChangePassword()) }
    }

    fun changePassword(old: String, new: String, repeat: String) {
        val dialog = _model.value.dialog as? VaultDialog.ChangePassword ?: return
        if (dialog.busy) return
        val problem = newPasswordError(new, repeat, port.minPasswordLength)
        if (problem != null) {
            _model.update { it.copy(dialog = VaultDialog.ChangePassword(error = problem)) }
            return
        }
        _model.update { it.copy(dialog = VaultDialog.ChangePassword(busy = true)) }
        scope.launch {
            val failure = failureOf { port.changePassword(old, new) }
            _model.update {
                when {
                    failure == null -> it.copy(
                        dialog = if (it.dialog is VaultDialog.ChangePassword) null else it.dialog,
                        notice = VaultText(R.string.vault_password_changed),
                    )

                    it.dialog is VaultDialog.ChangePassword ->
                        it.copy(dialog = VaultDialog.ChangePassword(error = failureText(failure)))

                    else -> it
                }
            }
        }
    }

    /** Also without a vault: the lock choices and "offer to save" do not need one. */
    fun openSettings() {
        val vault = _model.value.vault
        if (vault != VaultState.UNLOCKED && vault != VaultState.NONE) return
        hideJob?.cancel()
        _model.update { it.copy(dialog = VaultDialog.Settings) }
    }

    fun setLockMinutes(minutes: Int) {
        if (minutes in LOCK_MINUTE_CHOICES) port.setLockMinutes(minutes)
    }

    fun setBackgroundGraceSeconds(seconds: Int) {
        if (seconds in BACKGROUND_GRACE_CHOICES) port.setBackgroundGraceSeconds(seconds)
    }

    fun setOfferToSave(on: Boolean) = port.setOfferToSave(on)

    fun setSuggestPasswords(on: Boolean) = port.setSuggestPasswords(on)

    /** [enable] shows the system's biometric prompt (it needs the activity). */
    fun setBiometric(on: Boolean, enable: suspend () -> Unit) {
        if (!on) {
            port.biometric.disable()
            return
        }
        scope.launch {
            if (failureOf(enable) != null) notify(VaultText(R.string.vault_bio_enable_failed))
        }
    }

    /** "Use fingerprint" in the offer after creating the vault. */
    fun acceptBiometricOffer(enable: suspend () -> Unit) {
        if (_model.value.dialog != VaultDialog.OfferBiometric) return
        _model.update { it.copy(dialog = null) }
        setBiometric(true, enable)
    }

    // ----- key lost -----

    fun askDestroy() {
        if (_model.value.vault != VaultState.KEY_LOST) return
        _model.update { it.copy(dialog = VaultDialog.ConfirmDestroy()) }
    }

    /** "Try again" on the key-lost screen. */
    fun recheckKey() = port.recheckKey()

    /** Deletes the vault only when [typed] is the word asked for. */
    fun destroy(typed: String) {
        val dialog = _model.value.dialog as? VaultDialog.ConfirmDestroy ?: return
        if (dialog.busy) return
        if (!destroyConfirmed(typed)) {
            _model.update { it.copy(dialog = VaultDialog.ConfirmDestroy(refused = true)) }
            return
        }
        _model.update { it.copy(dialog = VaultDialog.ConfirmDestroy(busy = true)) }
        scope.launch {
            val failure = failureOf { port.destroy() }
            _model.update {
                it.copy(
                    dialog = null,
                    notice = if (failure == null) VaultText(R.string.vault_destroyed) else failureText(failure),
                )
            }
        }
    }

    private fun failed(failure: Throwable) {
        // A lock has its own screen; "the vault is locked" on top of it says nothing.
        if (failure !is VaultFailure.Locked) notify(failureText(failure))
    }

    private fun notify(text: VaultText) = _model.update { it.copy(notice = text) }

    companion object {
        /** How long a revealed password stays on screen. */
        const val REVEAL_MS = 30_000L
    }
}

/** Whether a just-made vault should ask to switch fingerprint unlock on. */
fun offersBiometric(bio: BiometricPort): Boolean = bio.availability() == BioAvailability.AVAILABLE && !bio.enabled.value
