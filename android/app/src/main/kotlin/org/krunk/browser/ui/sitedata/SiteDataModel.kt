package org.krunk.browser.ui.sitedata

import java.net.URI
import org.krunk.browser.engine.EngineSiteData
import org.krunk.ffi.SiteSummary

/** One site (registrable domain) on the site data screen. */
data class SiteRow(
    val site: String,
    /** Its cookies; null when the engine could not list cookies. */
    val cookies: Int?,
    /** Bytes of stored data (not cookies, not the cache); null when sizes are not available. */
    val bytes: Long?,
    /** Its web origins Krunk knows: visited, holding a permission, holding data, open in a tab. */
    val origins: List<String>,
    /** Unix ms of the last visit in the history, when there is one. */
    val lastUsed: Long?,
    /** A permission answer is stored for one of its origins. */
    val hasPermissions: Boolean,
)

/** Where the list comes from, which decides what it can say. */
enum class SiteSource {
    /** The engine listed cookies and stored data: the list is the sites that hold something. */
    ENGINE,

    /** The engine listed only one of the two; the sites of the history are listed as well. */
    PARTIAL,

    /** Nothing from the engine: the sites of the history, the permissions and the open tabs. */
    HISTORY,
}

data class SiteList(val rows: List<SiteRow>, val source: SiteSource)

enum class SiteOrder { NAME, LAST_USED }

/**
 * The list of the site data screen: what the engine listed merged with Krunk's own records.
 * Which host belongs to which site is the shared core's decision (`siteSummaries`, `siteOfHost`:
 * the public-suffix rules live there), never made here.
 *
 * What is listed. With the engine's full answer ([SiteSource.ENGINE]) a site is listed when it
 * holds cookies, stored data or a permission answer; a site that is only in the history holds
 * nothing this screen could clear and is left out, so a cleared site leaves the list. Without
 * the full answer every known site is listed (it may hold something Krunk cannot see), and a
 * cleared site stays listed, without counts, for as long as it is in the history.
 */
object SiteDataModel {
    /** The engine keeps at most a few thousand cookies; more than this is not a real answer. */
    const val MAX_COOKIES = 20_000

    /** One entry per cookie, as `siteSummaries` counts them. */
    fun cookieDomains(engine: EngineSiteData?): List<String> {
        val out = ArrayList<String>()
        for ((host, count) in engine?.cookies.orEmpty()) {
            if (host.isBlank()) continue
            repeat(count.coerceIn(0, MAX_COOKIES - out.size)) { out += host }
        }
        return out
    }

    /** Origins `siteSummaries` cannot know from the database: the ones with data, the open tabs. */
    fun extraOrigins(engine: EngineSiteData?, openTabs: List<String>): List<String> =
        (engine?.usage.orEmpty().filter { it.topHost.isEmpty() }.map { it.origin } + openTabs).distinct()

    /** The host of `scheme://host[:port]` (or of a page address), null when there is none. */
    fun hostOf(origin: String): String? = try {
        URI(origin).host?.takeIf { it.isNotEmpty() }
    } catch (e: java.net.URISyntaxException) {
        null
    }

    /**
     * [summaries]: `siteSummaries(cookieDomains(engine), extraOrigins(engine, tabs))`.
     * [permissionOrigins]: the origins with a stored permission answer. [siteOfHost]: the core's.
     */
    fun build(
        summaries: List<SiteSummary>,
        engine: EngineSiteData?,
        permissionOrigins: List<String>,
        siteOfHost: (String) -> String,
    ): SiteList {
        val cookiesKnown = engine?.cookies != null
        val usage = engine?.usage
        val source = when {
            cookiesKnown && usage != null -> SiteSource.ENGINE
            cookiesKnown || usage != null -> SiteSource.PARTIAL
            else -> SiteSource.HISTORY
        }
        val bytes = HashMap<String, Long>()
        for (u in usage.orEmpty()) {
            val host = u.topHost.ifEmpty { hostOf(u.origin) ?: continue }
            val site = siteOfHost(host)
            if (site.isNotEmpty()) bytes[site] = (bytes[site] ?: 0L) + u.bytes.coerceAtLeast(0L)
        }
        val withPermission = permissionOrigins.mapNotNull { o -> hostOf(o)?.let(siteOfHost) }.toSet()
        val rows = summaries.map { s ->
            SiteRow(
                site = s.site,
                cookies = if (cookiesKnown) s.cookies.toInt() else null,
                bytes = if (usage != null) bytes[s.site] ?: 0L else null,
                origins = s.origins,
                lastUsed = s.lastUsed,
                hasPermissions = s.site in withPermission,
            )
        }
        // Data kept under a site Krunk has no origin of (its history was cleared).
        val known = rows.mapTo(HashSet()) { it.site }
        val unknown = bytes.filterKeys { it !in known }.map { (site, size) ->
            SiteRow(site, if (cookiesKnown) 0 else null, size, emptyList(), null, site in withPermission)
        }
        val all = rows + unknown
        return SiteList(if (source == SiteSource.ENGINE) all.filter(::holdsSomething) else all, source)
    }

    /** Cookies, stored data or a permission answer, as far as known. */
    fun holdsSomething(row: SiteRow): Boolean = (row.cookies ?: 0) > 0 || (row.bytes ?: 0L) > 0L || row.hasPermissions

    fun sorted(rows: List<SiteRow>, order: SiteOrder): List<SiteRow> = when (order) {
        SiteOrder.NAME -> rows.sortedBy { it.site }

        // Most recent first; sites without a visit after them, by name.
        SiteOrder.LAST_USED -> rows.sortedWith(
            compareByDescending<SiteRow> { it.lastUsed ?: Long.MIN_VALUE }.thenBy { it.site },
        )
    }

    /** The sites whose name or one of whose origins contains [query]; all of them for a blank one. */
    fun search(rows: List<SiteRow>, query: String): List<SiteRow> {
        val q = query.trim().lowercase()
        if (q.isEmpty()) return rows
        return rows.filter { r -> r.site.lowercase().contains(q) || r.origins.any { it.lowercase().contains(q) } }
    }

    /**
     * The list right after [site] was cleared, until the engine is asked again: its counts are
     * zero where they were known, its permissions gone when [withPermissions]. See the class
     * comment for whether it stays listed.
     */
    fun cleared(list: SiteList, site: String, withPermissions: Boolean): SiteList {
        val rows = list.rows.mapNotNull { row ->
            if (row.site != site) return@mapNotNull row
            val after = row.copy(
                cookies = row.cookies?.let { 0 },
                bytes = row.bytes?.let { 0L },
                hasPermissions = row.hasPermissions && !withPermissions,
            )
            if (list.source == SiteSource.ENGINE && !holdsSomething(after)) null else after
        }
        return list.copy(rows = rows)
    }

    /** Whether a page at [url] belongs to [site] (open tabs of a cleared site are reloaded). */
    fun isOfSite(url: String, site: String, siteOfHost: (String) -> String): Boolean {
        if (!url.startsWith("http://", ignoreCase = true) &&
            !url.startsWith("https://", ignoreCase = true)
        ) {
            return false
        }
        val host = hostOf(url) ?: return false
        return siteOfHost(host) == site
    }
}
