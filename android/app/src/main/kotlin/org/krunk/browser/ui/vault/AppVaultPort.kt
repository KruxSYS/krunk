package org.krunk.browser.ui.vault

import android.app.Activity
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.krunk.browser.KrunkApp
import org.krunk.browser.R
import org.krunk.browser.vault.AndroidBioKeyStore
import org.krunk.browser.vault.AndroidBioPrompt
import org.krunk.browser.vault.AndroidBiometrics
import org.krunk.browser.vault.BioAvailability
import org.krunk.browser.vault.BioEnableResult
import org.krunk.browser.vault.BioOffReason
import org.krunk.browser.vault.BioPrompt
import org.krunk.browser.vault.BioPromptResult
import org.krunk.browser.vault.BioUnlockResult
import org.krunk.browser.vault.BiometricUnlock
import org.krunk.browser.vault.LockReason
import org.krunk.browser.vault.VaultBackend
import org.krunk.browser.vault.VaultFailure
import org.krunk.browser.vault.VaultState

/** The app's vault as the screens use it: the controller, its lock settings, fingerprint unlock. */
class AppVaultPort(private val app: KrunkApp) : VaultPort {
    private val controller = app.vault
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    override val state: StateFlow<VaultState> = controller.state
    override val lockEvents: SharedFlow<LockReason> = controller.lockEvents

    // 0.6.2: the vault password is also sync's one secret; the sync core gets it after each use.
    override suspend fun create(password: String) {
        controller.create(password)
        app.sync.vaultPasswordLater(password, changed = false)
    }

    override suspend fun unlock(password: String) {
        controller.unlock(password)
        app.sync.vaultPasswordLater(password, changed = false)
    }

    override fun retryWaitMs(): Long = controller.retryWaitMs()

    override fun lock(reason: LockReason) = controller.lock(reason)

    override suspend fun changePassword(old: String, new: String) {
        controller.changePassword(old, new)
        app.sync.vaultPasswordLater(new, changed = true)
    }

    override suspend fun <T> use(block: (VaultBackend) -> T): T = controller.use(block)

    override suspend fun destroy() = controller.destroy()

    override fun recheckKey() = controller.recheckKey()

    override val lockMinutes: StateFlow<Int> = app.vaultLock.minutes

    override fun setLockMinutes(minutes: Int) {
        scope.launch { withContext(Dispatchers.IO) { app.vaultLock.set(minutes) } }
    }

    override val backgroundGraceSeconds: StateFlow<Int> = app.vaultLock.graceSeconds

    override fun setBackgroundGraceSeconds(seconds: Int) {
        scope.launch { withContext(Dispatchers.IO) { app.vaultLock.setGraceSeconds(seconds) } }
    }

    private val _offerToSave = MutableStateFlow(app.loginVault.offersEnabled())
    override val offerToSave: StateFlow<Boolean> = _offerToSave.asStateFlow()

    override fun setOfferToSave(on: Boolean) {
        _offerToSave.value = on
        scope.launch { withContext(Dispatchers.IO) { app.loginVault.setOffersEnabled(on) } }
    }

    override val minPasswordLength: Int get() = app.vaultBackend.minPasswordLength()

    /**
     * One [BiometricUnlock] for the process (it owns the controller's reset hook and the "why is
     * it off" reason); the system prompt needs the activity on screen, which each call brings.
     */
    private var promptActivity: Activity? = null
    private val unlocker = BiometricUnlock(
        controller,
        app.vaultBackend,
        AndroidBioKeyStore(),
        BioPrompt { cipher ->
            val activity = promptActivity
            if (activity == null || android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.R) {
                BioPromptResult.Error(0)
            } else {
                AndroidBioPrompt(
                    activity,
                    activity.getString(R.string.vault_unlock_title),
                    activity.getString(R.string.vault_bio_use_password),
                ).authenticate(cipher)
            }
        },
        { AndroidBiometrics.availability(app) },
        Dispatchers.IO,
    )

    override val biometric: BiometricPort = object : BiometricPort {
        init {
            scope.launch { unlocker.refresh() }
        }

        override fun availability(): BioAvailability = unlocker.availability()

        override val enabled: StateFlow<Boolean> = unlocker.enabled
        override val offReason: StateFlow<BioOffReason?> = unlocker.offReason

        override suspend fun enable(activity: Activity) {
            promptActivity = activity
            val result = try {
                unlocker.enable()
            } finally {
                promptActivity = null
            }
            when (result) {
                BioEnableResult.ENABLED, BioEnableResult.CANCELLED -> Unit
                BioEnableResult.LOCKED -> throw VaultFailure.Locked()
                BioEnableResult.UNAVAILABLE, BioEnableResult.FAILED -> throw VaultFailure.Io("fingerprint unlock")
            }
        }

        override suspend fun unlock(activity: Activity): BioUnlockOutcome {
            promptActivity = activity
            val result = try {
                unlocker.refresh()
                unlocker.unlock()
            } finally {
                promptActivity = null
            }
            return when (result) {
                BioUnlockResult.Unlocked -> BioUnlockOutcome.UNLOCKED
                BioUnlockResult.Cancelled -> BioUnlockOutcome.CANCELLED
                is BioUnlockResult.FellBack -> BioUnlockOutcome.FELL_BACK
                BioUnlockResult.Failed -> BioUnlockOutcome.FAILED
            }
        }

        override fun disable() {
            scope.launch { unlocker.disable() }
        }
    }
}
