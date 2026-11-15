package org.krunk.browser.sync

import android.util.Log
import java.io.File
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.krunk.ffi.DeviceTabs
import org.krunk.ffi.KrunkSync
import org.krunk.ffi.LinkInfo
import org.krunk.ffi.LinkPoll
import org.krunk.ffi.PasswordsAsk
import org.krunk.ffi.PasswordsSync
import org.krunk.ffi.RunReport
import org.krunk.ffi.SyncChanged
import org.krunk.ffi.SyncFfiException
import org.krunk.ffi.SyncStage
import org.krunk.ffi.SyncStatus
import org.krunk.ffi.TurnOn

/** What the app does after a run brought changes (reload the repositories that show them). */
fun interface SyncChanges {
    fun applied(changed: SyncChanged)
}

/**
 * Krunk account and sync for the app (0.6.0): one [KrunkSync] (the shared core's steps), its
 * [status] for the screen, and the schedule.
 *
 * **Schedule** (Windows parity, D5): only while Krunk is in the foreground ([onForeground] /
 * [onBackground]): a run a minute after start, every 5 minutes, 30 s after a local change (seen
 * as queued changes in the local database, no network), and [syncNow]. There is no background
 * work and nothing runs before the user signs in.
 */
class SyncController(
    private val sync: KrunkSync,
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val clock: () -> Long,
    private val changes: SyncChanges,
) {
    private val _status = MutableStateFlow<SyncStatus?>(null)
    val status: StateFlow<SyncStatus?> = _status.asStateFlow()

    /** One core call at a time (the core serialises too; this keeps the UI's order). */
    private val turn = Mutex()
    private var ticker: Job? = null

    /** When queued local changes were first seen (for "30 s after a change"). */
    private var pendingSince: Long? = null

    private suspend fun <T> call(block: (KrunkSync) -> T): T = turn.withLock {
        withContext(io) {
            try {
                block(sync)
            } finally {
                _status.value = sync.status()
            }
        }
    }

    /** Reads the local state (no network). */
    suspend fun refresh() {
        withContext(io) { _status.value = sync.status() }
    }

    suspend fun linkStart(): Result<LinkInfo> = runCatchingSync { call { it.linkStart() } }

    suspend fun linkPoll(): Result<LinkPoll> = runCatchingSync { call { it.linkPoll() } }

    suspend fun linkCancel() = call { it.linkCancel() }

    /** `true` = the server confirmed the sign-out. */
    suspend fun signOut(removeSyncedData: Boolean): Boolean {
        val reached = call { it.signOut(removeSyncedData) }
        if (removeSyncedData) changes.applied(ALL_CHANGED)
        return reached
    }

    suspend fun turnOn(): Result<TurnOn> = runCatchingSync { call { it.turnOn() } }

    suspend fun setupConfirm(group: String): Result<Unit> = runCatchingSync {
        call { it.setupConfirm(group) }
        syncNowLater()
    }

    suspend fun restoreBegin(): Result<Unit> = runCatchingSync { call { it.restoreBegin() } }

    suspend fun restoreTry(secret: String, recoveryKey: Boolean): Result<SyncStage> =
        runCatchingSync { call { it.restoreTry(secret, recoveryKey) } }

    suspend fun restoreConfirm(): Result<Unit> = runCatchingSync {
        call { it.restoreConfirm() }
        syncNowLater()
    }

    suspend fun joinStart(sharePasswords: Boolean): Result<Unit> = runCatchingSync {
        call { it.joinStart(sharePasswords) }
    }

    suspend fun joinPoll(): Result<SyncStage> = runCatchingSync {
        val stage = call { it.joinPoll() }
        if (stage == SyncStage.On) syncNowLater()
        stage
    }

    suspend fun joinCode(code: String): Result<Unit> = runCatchingSync { call { it.joinCode(code) } }

    suspend fun cancel() = call { it.cancel() }

    /** "Add passwords" (0.6.1): the passphrase or recovery key gives this phone the passwords key. */
    suspend fun addPasswords(secret: String, recoveryKey: Boolean): Result<Unit> = runCatchingSync {
        call { it.addPasswords(secret, recoveryKey) }
        syncNowLater()
    }

    /** "Ask my PC" (0.6.2): another device that holds the passwords key gives it to this phone. */
    suspend fun passwordsAskStart(): Result<Unit> = runCatchingSync { call { it.passwordsAskStart() } }

    suspend fun passwordsAskPoll(): Result<PasswordsAsk> = runCatchingSync {
        val before = _status.value?.passwords
        val r = call { it.passwordsAskPoll() }
        if (before != PasswordsSync.ON && _status.value?.passwords == PasswordsSync.ON) syncNowLater()
        r
    }

    suspend fun passwordsAskCode(code: String): Result<Unit> = runCatchingSync { call { it.passwordsAskCode(code) } }

    suspend fun setCollection(name: String, on: Boolean): Result<Unit> =
        runCatchingSync { call { it.setCollection(name, on) } }

    suspend fun remoteTabs(): List<DeviceTabs> = withContext(io) { sync.remoteTabs() }

    /** "Sync now", and the schedule's runs. */
    suspend fun syncNow(): Result<RunReport> = runCatchingSync {
        val report = call { it.runNow() }
        if (report.changed.any()) changes.applied(report.changed)
        report
    }

    private fun syncNowLater() {
        scope.launch { syncNow() }
    }

    /** The vault was unlocked: passwords that waited come now (and a stale sync part goes). */
    fun vaultUnlocked() {
        scope.launch { call { it.vaultUnlocked() } }
    }

    fun onForeground() {
        if (ticker?.isActive == true) return
        ticker = scope.launch {
            refresh()
            while (isActive) {
                delay(TICK_MS)
                tick()
            }
        }
    }

    fun onBackground() {
        ticker?.cancel()
        ticker = null
    }

    private suspend fun tick() {
        val st = withContext(io) { sync.status() }
        _status.value = st
        if (st.stage != SyncStage.On) return
        val now = clock()
        pendingSince = when {
            st.pending <= 0 -> null
            else -> pendingSince ?: now
        }
        if (due(now, st.nextRunMs, pendingSince)) syncNow()
    }

    companion object {
        const val TAG = "KrunkSync"
        const val TICK_MS = 10_000L
        const val AFTER_CHANGE_MS = 30_000L

        /** Is a run due: the core's next run, or 30 s after local changes were first seen. */
        fun due(now: Long, nextRunMs: Long?, pendingSince: Long?): Boolean =
            (nextRunMs != null && now >= nextRunMs) || (pendingSince != null && now - pendingSince >= AFTER_CHANGE_MS)

        val ALL_CHANGED = SyncChanged(
            bookmarks = true,
            history = true,
            settings = true,
            pins = true,
            allowlist = true,
            tabs = true,
            passwords = true,
            neverSave = true,
        )

        /** The app-private folder of account.json and sync.json (excluded from every backup). */
        fun dir(filesDir: File): File = File(filesDir, "krunk-sync")
    }
}

private fun SyncChanged.any(): Boolean =
    bookmarks || history || settings || pins || allowlist || tabs || passwords || neverSave

/** Runs [block]; a [SyncFfiException] becomes a failure (its text is for the screen). */
private inline fun <T> runCatchingSync(block: () -> T): Result<T> = try {
    Result.success(block())
} catch (e: SyncFfiException) {
    Log.i(SyncController.TAG, "sync step failed: ${e.javaClass.simpleName}")
    Result.failure(e)
}

/** The sentence a [SyncFfiException] carries. */
val SyncFfiException.sentence: String
    get() = when (this) {
        is SyncFfiException.WrongSecret -> text
        is SyncFfiException.WrongCode -> text
        is SyncFfiException.Revoked -> text
        is SyncFfiException.SignedOut -> text
        is SyncFfiException.Offline -> text
        is SyncFfiException.Server -> text
        is SyncFfiException.RateLimited -> text
        is SyncFfiException.Alarm -> text
        is SyncFfiException.Reset -> text
        is SyncFfiException.Local -> text
        is SyncFfiException.NotNow -> text
    }
