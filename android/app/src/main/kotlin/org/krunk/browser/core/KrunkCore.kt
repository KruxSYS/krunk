package org.krunk.browser.core

import android.content.Context
import android.util.Log
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext
import org.krunk.ffi.FfiException
import org.krunk.ffi.KrunkStore

/**
 * The shared Rust core as the app sees it: one [KrunkStore] on `filesDir/krunk/browser.db`
 * (the same SQLite schema as Windows Krunk) and the settings built on it.
 *
 * Store calls are synchronous and serialised inside the core; call them through [io] from the
 * UI so the main thread never waits on SQLite.
 */
class KrunkCore(context: Context) {
    val dataDir: File = File(context.filesDir, DATA_DIR).apply { mkdirs() }
    val databaseFile: File = File(dataDir, DB_FILE)
    private val opened = openStore(databaseFile.path)
    val store: KrunkStore = opened.store

    /**
     * Why the database could not be opened, else null. With an error [store] is a temporary
     * in-memory one: Krunk works, nothing is kept, and the file is left as it is.
     */
    val storageError: String? = opened.error
    val settings: SettingsRepository = SettingsRepository(store)

    private val _historySyncs = MutableStateFlow(0)

    /** Counts sync runs that changed history, so an open History screen reads it again. */
    val historySyncs: StateFlow<Int> = _historySyncs.asStateFlow()

    fun historySynced() {
        _historySyncs.value++
    }

    suspend fun <T> io(block: (KrunkStore) -> T): T = withContext(Dispatchers.IO) { block(store) }

    companion object {
        const val DATA_DIR = "krunk"
        const val DB_FILE = "browser.db"
    }
}

/** A store and, when the database file could not be opened, why. */
class OpenedStore(val store: KrunkStore, val error: String?)

/**
 * Opens the database at [path]. A file that cannot be opened (corrupt, disk full, no
 * permission) must not keep the browser from starting on every launch: the session then runs
 * on an in-memory store and the caller tells the user. The file itself is not touched.
 */
fun openStore(path: String): OpenedStore = try {
    OpenedStore(KrunkStore.open(path), null)
} catch (e: FfiException) {
    Log.e("Krunk", "could not open the database, using a temporary one", e)
    OpenedStore(KrunkStore.inMemory(), e.message ?: "database error")
}
