package org.krunk.browser.engine

/** Data one origin has stored; [topHost] is the site it is kept apart under, "" for its own. */
data class OriginUsage(val origin: String, val topHost: String, val bytes: Long)

/**
 * What the engine listed (see [SiteDataExtension]): cookies counted per host (a cookie kept
 * apart under a site counts for that site's host) and the stored data per origin. A part the
 * engine could not list is null, which is not the same as "none".
 */
data class EngineSiteData(val cookies: Map<String, Int>?, val usage: List<OriginUsage>?)
