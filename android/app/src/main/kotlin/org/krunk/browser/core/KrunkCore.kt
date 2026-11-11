package org.krunk.browser.core

import android.content.Context
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
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
    val store: KrunkStore = KrunkStore.open(databaseFile.path)
    val settings: SettingsRepository = SettingsRepository(store)

    suspend fun <T> io(block: (KrunkStore) -> T): T = withContext(Dispatchers.IO) { block(store) }

    companion object {
        const val DATA_DIR = "krunk"
        const val DB_FILE = "browser.db"
    }
}
