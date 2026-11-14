package org.krunk.browser.update

import java.io.File
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import kotlin.coroutines.coroutineContext
import kotlinx.coroutines.ensureActive

/**
 * Krunk's own update requests, through the platform's HTTP stack (not the engine: no cookies, no
 * site data, no Gecko profile). Each request carries only what HTTP needs and the User-Agent
 * `Krunk-Android/<version>`: no query, no identifiers, no cookies. Redirects are not followed
 * and nothing is cached. Only [UpdateController] calls this, and only after a tap.
 */
class UpdateHttp(private val userAgent: String) {
    class HttpFailure(message: String) : IOException(message)

    private fun open(url: String): HttpURLConnection {
        val conn = URL(url).openConnection() as HttpURLConnection
        conn.instanceFollowRedirects = false
        conn.useCaches = false
        conn.connectTimeout = 15_000
        conn.readTimeout = 30_000
        conn.setRequestProperty("User-Agent", userAgent)
        conn.setRequestProperty("Accept-Encoding", "identity")
        conn.setRequestProperty("Cache-Control", "no-cache")
        return conn
    }

    /** A small file (the manifest or its signature), at most [limit] bytes. */
    suspend fun fetchSmall(url: String, limit: Int): ByteArray {
        val conn = open(url)
        try {
            val code = conn.responseCode
            if (code != 200) throw HttpFailure("HTTP $code")
            val out = java.io.ByteArrayOutputStream()
            conn.inputStream.use { input ->
                val buf = ByteArray(8192)
                while (true) {
                    coroutineContext.ensureActive()
                    val n = input.read(buf)
                    if (n < 0) break
                    out.write(buf, 0, n)
                    // One byte over the limit is enough for the core to refuse it as too large.
                    if (out.size() > limit) break
                }
            }
            return out.toByteArray()
        } finally {
            conn.disconnect()
        }
    }

    /** What a finished download had. */
    data class Downloaded(val bytes: Long, val sha256: String)

    /**
     * The APK into [dest]: never more than [expectedSize] bytes are kept (a longer answer is cut
     * off and then fails the size check), hashed while it arrives, [progress] after each piece.
     * Cancelling the coroutine stops it; the caller deletes the file.
     */
    suspend fun download(url: String, dest: File, expectedSize: Long, progress: (Long) -> Unit): Downloaded {
        val conn = open(url)
        try {
            val code = conn.responseCode
            if (code != 200) throw HttpFailure("HTTP $code")
            val md = MessageDigest.getInstance("SHA-256")
            var total = 0L
            conn.inputStream.use { input ->
                dest.outputStream().use { out ->
                    val buf = ByteArray(1 shl 16)
                    var lastReport = 0L
                    while (true) {
                        coroutineContext.ensureActive()
                        val n = input.read(buf)
                        if (n < 0) break
                        if (total + n > expectedSize) {
                            // Too long: keep one byte more than allowed, so the check names it.
                            val keep = (expectedSize - total + 1).toInt().coerceAtMost(n)
                            md.update(buf, 0, keep)
                            out.write(buf, 0, keep)
                            total += keep
                            break
                        }
                        md.update(buf, 0, n)
                        out.write(buf, 0, n)
                        total += n
                        if (total - lastReport >= 256 * 1024) {
                            lastReport = total
                            progress(total)
                        }
                    }
                    out.fd.sync()
                }
            }
            progress(total)
            return Downloaded(total, UpdateRules.hex(md.digest()))
        } finally {
            conn.disconnect()
        }
    }
}
