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
import org.krunk.browser.vault.BioOffReason
import org.krunk.browser.vault.VaultFailure

/** What the unlock form shows. The typed password is not in here: it stays in the form. */
data class UnlockUiModel(
    /** An unlock is running (the password derivation takes about a second). */
    val busy: Boolean = false,
    val error: VaultText? = null,
    /** Seconds until the next attempt is allowed; 0 = now. */
    val waitSeconds: Int = 0,
    /** Biometric unlock is switched on and usable: show its button. */
    val biometricOffered: Boolean = false,
    /** Biometric unlock was switched off behind the user's back. */
    val offReason: BioOffReason? = null,
) {
    val waitText: VaultText? get() = if (waitSeconds >
        0
    ) {
        VaultText(R.string.vault_error_rate_limited, waitSeconds)
    } else {
        null
    }
    val canSubmit: Boolean get() = !busy && waitSeconds == 0
}

/**
 * The unlock form's logic, for the vault screen and for [UnlockVaultDialog]: one attempt at a
 * time, the wait after repeated failures counted down, the biometric way. Whether the vault is
 * open is read from the port's state, not from here.
 */
class UnlockUiState(private val port: VaultPort, private val scope: CoroutineScope) {
    private val _model = MutableStateFlow(UnlockUiModel(offReason = port.biometric.offReason.value))
    val model: StateFlow<UnlockUiModel> = _model.asStateFlow()

    private var countdown: Job? = null

    init {
        scope.launch {
            port.biometric.enabled.collect { on ->
                val offered = on && port.biometric.availability() == BioAvailability.AVAILABLE
                _model.update { it.copy(biometricOffered = offered) }
            }
        }
        scope.launch { port.biometric.offReason.collect { reason -> _model.update { it.copy(offReason = reason) } } }
        startCountdown()
    }

    fun submit(password: String) {
        if (password.isEmpty() || !_model.value.canSubmit) return
        _model.update { it.copy(busy = true, error = null) }
        scope.launch {
            val failure = failureOf { port.unlock(password) }
            _model.update {
                it.copy(
                    busy = false,
                    // The countdown line says it; no second sentence beside it.
                    error = failure?.takeIf { f -> f !is VaultFailure.RateLimited }?.let(::failureText),
                )
            }
            if (failure != null) startCountdown()
        }
    }

    /** [prompt] shows the system's biometric prompt (it needs the activity) and unlocks. */
    fun biometric(prompt: suspend () -> BioUnlockOutcome) {
        val now = _model.value
        if (now.busy || !now.biometricOffered) return
        _model.update { it.copy(busy = true, error = null) }
        scope.launch {
            var result: BioUnlockOutcome? = null
            val failure = failureOf { result = prompt() }
            val error = when {
                failure != null -> failureText(failure)
                result == BioUnlockOutcome.FAILED -> VaultText(R.string.vault_bio_failed)
                else -> null
            }
            _model.update { it.copy(busy = false, error = error) }
        }
    }

    /** A new lock: the last attempt's error is about a vault state that is gone. */
    fun reset() {
        _model.update { it.copy(error = null) }
        startCountdown()
    }

    private fun startCountdown() {
        countdown?.cancel()
        countdown = scope.launch {
            while (true) {
                val waitMs = port.retryWaitMs()
                val seconds = waitSeconds(waitMs)
                _model.update { it.copy(waitSeconds = seconds) }
                if (seconds == 0) break
                // To the next whole second, so the text changes when the number does.
                delay(waitMs - (seconds - 1) * MILLIS)
            }
        }
    }

    private companion object {
        const val MILLIS = 1000L
    }
}
