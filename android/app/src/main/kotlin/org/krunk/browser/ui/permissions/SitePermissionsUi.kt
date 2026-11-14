package org.krunk.browser.ui.permissions

import org.krunk.browser.engine.permissions.PermissionDefault
import org.krunk.browser.engine.permissions.PermissionKind
import org.krunk.ffi.SitePermission

/**
 * What the site-permission screens show, worked out from the table's rows. Pure. A kind this
 * version does not know (Windows stores `sound`, for one) is listed under its key and can be
 * changed or removed like any other.
 */
object SitePermissionsUi {
    /** One site of the list: its origin and the kinds it is allowed and denied (keys, in display order). */
    data class SiteRow(val origin: String, val allowed: List<String>, val denied: List<String>) {
        /** The origin without `https://`; a plain-http origin keeps its scheme, so the two differ. */
        val label: String get() = origin.removePrefix("https://")
    }

    /** The three states of a kind. [DEFAULT] is "no row": ask, or what the kind does by default. */
    enum class Choice { ALLOW, DENY, DEFAULT }

    /** One line of a site's page. [kind] is null for a key this version does not know. */
    data class KindRow(
        val key: String,
        val kind: PermissionKind?,
        val choice: Choice,
        val default: PermissionDefault,
        /** The choices offered (an allow that cannot be remembered is not, unless it is the current one). */
        val choices: List<Choice>,
    )

    private val order: Map<String, Int> = PermissionKind.entries.withIndex().associate { (i, k) -> k.key to i }

    private fun sortedKinds(keys: List<String>): List<String> =
        keys.distinct().sortedWith(compareBy<String> { order[it] ?: Int.MAX_VALUE }.thenBy { it })

    private fun host(origin: String): String = origin.substringAfter("://")

    /**
     * The sites with a stored answer, by host name; [query] (any case, part of the address)
     * narrows the list.
     */
    fun sites(rows: List<SitePermission>, query: String = ""): List<SiteRow> {
        val q = query.trim().lowercase()
        return rows.groupBy { it.origin }
            .filterKeys { q.isEmpty() || q in it.lowercase() }
            .map { (origin, list) ->
                SiteRow(
                    origin,
                    allowed = sortedKinds(list.filter { it.allow }.map { it.kind }),
                    denied = sortedKinds(list.filterNot { it.allow }.map { it.kind }),
                )
            }
            .sortedWith(compareBy<SiteRow> { host(it.origin) }.thenBy { it.origin })
    }

    /** Every known kind with the site's answer, then the site's rows of unknown kinds. */
    fun detail(origin: String, rows: List<SitePermission>): List<KindRow> {
        val mine = rows.filter { it.origin == origin }.associate { it.kind to it.allow }
        val known = PermissionKind.entries.map { kind ->
            val choice = choiceOf(mine[kind.key])
            val choices = buildList {
                if (kind.rememberAllow || choice == Choice.ALLOW) add(Choice.ALLOW)
                add(Choice.DENY)
                add(Choice.DEFAULT)
            }
            KindRow(kind.key, kind, choice, kind.default, choices)
        }
        val unknown = sortedKinds(mine.keys.filter { PermissionKind.ofKey(it) == null }).map { key ->
            KindRow(key, null, choiceOf(mine[key]), PermissionDefault.ASK, Choice.entries)
        }
        return known + unknown
    }

    private fun choiceOf(allow: Boolean?): Choice = when (allow) {
        true -> Choice.ALLOW
        false -> Choice.DENY
        null -> Choice.DEFAULT
    }

    /** What a tap on [choice] stores: allow, deny, or (null) no row. */
    fun valueOf(choice: Choice): Boolean? = when (choice) {
        Choice.ALLOW -> true
        Choice.DENY -> false
        Choice.DEFAULT -> null
    }

    /** Whether a site's page can be opened for [origin] (a web origin as the table keys it). */
    fun isSite(origin: String?): Boolean =
        origin != null && (origin.startsWith("https://") || origin.startsWith("http://"))
}
