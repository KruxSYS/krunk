package org.krunk.browser.engine.permissions

import org.krunk.ffi.FfiException
import org.krunk.ffi.KrunkStore
import org.krunk.ffi.SitePermission

/**
 * The shared `site_permissions` table as the policy's store. Every call goes to the database
 * (a lookup by key), so a row another part of Krunk removes (clearing a site's data) counts at
 * the very next request. A failing database reads as "no answer": the site is asked.
 */
class TablePermissionStore(private val store: KrunkStore, private val now: () -> Long) : PermissionStore {
    override fun get(origin: String, kind: String): Boolean? = try {
        store.sitePermission(origin, kind)
    } catch (_: FfiException) {
        null
    }

    override fun set(origin: String, kind: String, allow: Boolean) {
        try {
            store.sitePermissionSet(origin, kind, allow, now())
        } catch (_: FfiException) {
            // Not stored: the site asks again next time.
        }
    }

    override fun remove(origin: String, kind: String) {
        try {
            store.sitePermissionRemove(origin, kind)
        } catch (_: FfiException) {
            // Still stored: the screen shows it on its next reload.
        }
    }

    /** Every row, by origin and kind (also kinds this version does not know). */
    fun list(): List<SitePermission> = try {
        store.sitePermissionsList()
    } catch (_: FfiException) {
        emptyList()
    }

    fun clearOrigin(origin: String) {
        try {
            store.sitePermissionsClearOrigin(origin)
        } catch (_: FfiException) {
            // As in remove.
        }
    }

    fun clear() {
        try {
            store.sitePermissionsClear()
        } catch (_: FfiException) {
            // As in remove.
        }
    }
}
