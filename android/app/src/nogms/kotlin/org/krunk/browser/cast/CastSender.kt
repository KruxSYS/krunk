package org.krunk.browser.cast

import android.content.Context

/**
 * The Cast sender of a build without Google Play Services (`-Pkrunk.noGms=true`): the same API as
 * src/gms, always unavailable, so the page menu shows "Cast screen" only. No Cast class is in this
 * build.
 */
object CastSender {
    @Suppress("UNUSED_PARAMETER")
    fun available(context: Context): Boolean = false

    @Suppress("UNUSED_PARAMETER")
    fun cast(context: Context, media: CastMedia, say: (String) -> Unit) = Unit
}
