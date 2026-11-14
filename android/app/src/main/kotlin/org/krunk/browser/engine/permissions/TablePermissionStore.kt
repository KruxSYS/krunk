package org.krunk.browser.engine.permissions

import org.krunk.ffi.FfiException
import org.krunk.ffi.KrunkStore
import org.krunk.ffi.SitePermission

/** The database calls of the `site_permissions` table (the `ffi` methods), as a seam for tests. */
interface PermissionRows {
    fun all(): List<SitePermission>

    fun set(origin: String, kind: String, allow: Boolean)

    fun remove(origin: String, kind: String)

    fun clearOrigin(origin: String)

    fun clear()

    companion object {
        fun of(store: KrunkStore, now: () -> Long) = object : PermissionRows {
            override fun all() = store.sitePermissionsList()

            override fun set(origin: String, kind: String, allow: Boolean) =
                store.sitePermissionSet(origin, kind, allow, now())

            override fun remove(origin: String, kind: String) {
                store.sitePermissionRemove(origin, kind)
            }

            override fun clearOrigin(origin: String) {
                store.sitePermissionsClearOrigin(origin)
            }

            override fun clear() = store.sitePermissionsClear()
        }
    }
}

/**
 * The shared `site_permissions` table as the policy's store. The rows are read once and kept in
 * memory: a permission request (autoplay asks on ordinary page loads) is answered on the main
 * thread and must not wait for the database, which a vacuum on another thread may hold for a
 * while. Every change made through this object updates the copy in the same step (only once the
 * database took it); a change made around it (none in the app today) must call [invalidate].
 * A failing database reads as "no answer": the site is asked, and the next request tries again.
 *
 * [normalize] turns an address into the key the table uses (the core's `webOrigin`).
 */
class TablePermissionStore(private val rows: PermissionRows, private val normalize: (String) -> String?) :
    PermissionStore {
    private val lock = Any()
    private var cache: HashMap<Pair<String, String>, Boolean>? = null

    private fun key(origin: String, kind: String) = (normalize(origin) ?: origin) to kind

    private fun loaded(): HashMap<Pair<String, String>, Boolean>? {
        cache?.let { return it }
        val all = try {
            rows.all()
        } catch (_: FfiException) {
            return null
        }
        return HashMap<Pair<String, String>, Boolean>().also { map ->
            for (p in all) map[p.origin to p.kind] = p.allow
            cache = map
        }
    }

    /** Reads the rows now (at start, off the main thread), so the first request finds them. */
    fun warm() {
        synchronized(lock) { loaded() }
    }

    fun invalidate() {
        synchronized(lock) { cache = null }
    }

    override fun get(origin: String, kind: String): Boolean? = synchronized(lock) { loaded()?.get(key(origin, kind)) }

    override fun set(origin: String, kind: String, allow: Boolean) {
        synchronized(lock) {
            try {
                rows.set(origin, kind, allow)
                cache?.put(key(origin, kind), allow)
            } catch (_: FfiException) {
                // Not stored: the site asks again next time.
            }
        }
    }

    override fun remove(origin: String, kind: String) {
        synchronized(lock) {
            try {
                rows.remove(origin, kind)
                cache?.remove(key(origin, kind))
            } catch (_: FfiException) {
                // Still stored: the screen shows it on its next reload.
            }
        }
    }

    /** Every row, by origin and kind (also kinds this version does not know), from the database. */
    fun list(): List<SitePermission> = try {
        rows.all()
    } catch (_: FfiException) {
        emptyList()
    }

    fun clearOrigin(origin: String) {
        synchronized(lock) {
            try {
                rows.clearOrigin(origin)
                val o = normalize(origin) ?: origin
                cache?.keys?.removeAll { it.first == o }
            } catch (_: FfiException) {
                // As in remove.
            }
        }
    }

    fun clear() {
        synchronized(lock) {
            try {
                rows.clear()
                cache = HashMap()
            } catch (_: FfiException) {
                cache = null
            }
        }
    }

    companion object {
        fun of(store: KrunkStore, now: () -> Long, normalize: (String) -> String?) =
            TablePermissionStore(PermissionRows.of(store, now), normalize)
    }
}
