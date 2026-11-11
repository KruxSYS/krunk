package org.krunk.browser.tabs

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.Log
import android.util.LruCache
import java.io.ByteArrayOutputStream
import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.krunk.browser.KrunkApp
import org.krunk.browser.core.KrunkCore
import org.krunk.browser.core.readCapped
import org.krunk.browser.engine.IconCandidate
import org.krunk.ffi.hostOf
import org.krunk.ffi.nowMs

/**
 * Favicons by host: stored as 32 px PNG in the core `favicons` table (same as Windows), decoded
 * into an in-memory cache. Fetching happens only for pages the user loaded (the icon URL the
 * page itself declared, or `/favicon.ico`), never in the background.
 */
class Favicons(private val core: KrunkCore, private val userAgent: () -> String = { "" }) {
    private val cache = LruCache<String, Bitmap>(128)
    private val missing = HashSet<String>()
    private val inFlight = HashSet<String>()

    /** Hosts of addresses already asked about (the tab list asks for every tab on every refresh). */
    private val hosts = LruMap<String, String>(256)

    private fun hostFor(url: String): String? {
        hosts[url]?.let { return it.ifEmpty { null } }
        val host = hostOf(url)
        if (url.length <= 2048) hosts[url] = host ?: ""
        return host
    }

    /** The icon held in memory for the address's host, if any. */
    fun cached(url: String): Bitmap? = hostFor(url)?.let { cache.get(it) }

    /** Forgets every icon held in memory (the stored ones are deleted by the caller). */
    fun clear() {
        cache.evictAll()
        missing.clear()
    }

    /** Loads a host's icon from the store into the cache (IO). */
    suspend fun load(host: String): Bitmap? {
        cache.get(host)?.let { return it }
        if (host in missing) return null
        val png = withContext(Dispatchers.IO) { runCatching { core.store.favicon(host) }.getOrNull() }
        if (png == null) {
            missing += host
            return null
        }
        val bmp = BitmapFactory.decodeByteArray(png, 0, png.size) ?: return null
        cache.put(host, bmp)
        return bmp
    }

    suspend fun fetch(pageUrl: String, icons: List<IconCandidate>, onDone: () -> Unit) {
        val host = hostOf(pageUrl) ?: return
        // A host without a usable icon is not asked again on every page load.
        if (cache.get(host) != null || host in inFlight || host in missing) return
        if (load(host) != null) {
            onDone()
            return
        }
        inFlight += host
        try {
            val candidates = (
                icons.sortedByDescending {
                    score(it)
                }.map { it.href } + listOf(iconFallback(pageUrl))
                ).distinct()
            for (href in candidates.take(3)) {
                // Only the small copy is kept, in memory as on disk.
                val bmp = withContext(Dispatchers.IO) { download(href)?.let(::scaled) } ?: continue
                val png = withContext(Dispatchers.IO) { encode(bmp) }
                if (png.size <= MAX_PNG) {
                    withContext(Dispatchers.IO) { runCatching { core.store.setFavicon(host, png, nowMs()) } }
                }
                cache.put(host, bmp)
                missing -= host
                onDone()
                return
            }
            missing += host
        } finally {
            inFlight -= host
        }
    }

    private fun iconFallback(pageUrl: String): String {
        val u = runCatching { URL(pageUrl) }.getOrNull() ?: return ""
        return "${u.protocol}://${u.authority}/favicon.ico"
    }

    private fun score(c: IconCandidate): Int {
        val size = c.sizes.substringBefore('x').toIntOrNull() ?: 0
        var s = if (size in 32..128) {
            100 + size
        } else if (size > 128) {
            60
        } else {
            50
        }
        if (c.type.contains("svg")) s -= 100 // BitmapFactory can't decode SVG
        if (c.rel.contains("apple")) s -= 20
        return s
    }

    private fun download(href: String): Bitmap? {
        if (href.isEmpty() || !(href.startsWith("https://") || href.startsWith("http://"))) return null
        return try {
            val conn = URL(href).openConnection() as HttpURLConnection
            conn.connectTimeout = 5000
            conn.readTimeout = 5000
            conn.instanceFollowRedirects = true
            conn.setRequestProperty("Accept", "image/*")
            // The browser's own user agent, not the platform's (which names the device model).
            userAgent().takeIf { it.isNotEmpty() }?.let { conn.setRequestProperty("User-Agent", it) }
            conn.useCaches = false
            try {
                if (conn.responseCode != 200) return null
                // The page chose this address: never trust the size of what comes back.
                if (conn.contentLengthLong > MAX_DOWNLOAD) return null
                val bytes = conn.inputStream.use { it.readCapped(MAX_DOWNLOAD) } ?: return null
                // A few kilobytes can describe a picture of hundreds of megabytes: look at the
                // size first, refuse the absurd and decode the rest scaled down.
                val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
                val sample = iconSampleSize(bounds.outWidth, bounds.outHeight) ?: return null
                BitmapFactory.decodeByteArray(
                    bytes,
                    0,
                    bytes.size,
                    BitmapFactory.Options().apply { inSampleSize = sample },
                )
            } finally {
                conn.disconnect()
            }
        } catch (e: Exception) {
            Log.d(KrunkApp.TAG, "favicon $href: ${e.message}")
            null
        }
    }

    private fun scaled(bmp: Bitmap): Bitmap =
        if (bmp.width > SIZE || bmp.height > SIZE) Bitmap.createScaledBitmap(bmp, SIZE, SIZE, true) else bmp

    private fun encode(bmp: Bitmap): ByteArray {
        val out = ByteArrayOutputStream()
        bmp.compress(Bitmap.CompressFormat.PNG, 100, out)
        return out.toByteArray()
    }

    companion object {
        const val SIZE = 32
        const val MAX_PNG = 48 * 1024
        const val MAX_DOWNLOAD = 512 * 1024

        /** Icons larger than this on a side are refused undecoded. */
        const val MAX_SIDE = 2048

        /**
         * How much to shrink an icon of [width] x [height] while decoding (a power of two, as
         * the decoder wants it) so that it stays at or above twice the stored size, or null when
         * the picture is not an icon at all (no size, or larger than [MAX_SIDE] on a side).
         */
        fun iconSampleSize(width: Int, height: Int): Int? {
            if (width <= 0 || height <= 0 || width > MAX_SIDE || height > MAX_SIDE) return null
            var sample = 1
            while (width / (sample * 2) >= SIZE * 2 && height / (sample * 2) >= SIZE * 2) sample *= 2
            return sample
        }
    }
}
