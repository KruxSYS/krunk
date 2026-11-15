package org.krunk.browser.sync

import android.util.Log
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.io.InterruptedIOException
import java.net.HttpURLConnection
import java.net.SocketTimeoutException
import java.net.URL
import javax.net.ssl.SSLException
import org.krunk.ffi.HttpAnswer
import org.krunk.ffi.SyncTransport
import org.krunk.ffi.TransportException

/**
 * The transport of Krunk account and sync (0.6.0): the platform's HTTP stack, not the engine
 * (no cookies, no site data, no Gecko profile).
 *
 * - **Only** `<base>/api/v1/<path>`; paths come from the core and are checked here as well.
 * - HTTPS only unless [allowHttp] (qa and debug builds against a staging stack).
 * - The system's trust store and no certificate pinning, as Windows' WinHTTP.
 * - No redirects, no cache, answers capped at [MAX_BODY] bytes.
 * - The User-Agent is `Krunk/<version> (Android)`.
 *
 * [log] (qa and debug builds only) writes the method, the path without its query, and the
 * status to logcat under [TAG]. It never writes a token, a body or a header.
 */
class SyncHttp(
    private val base: String,
    private val allowHttp: Boolean,
    private val userAgent: String,
    private val log: Boolean,
) : SyncTransport {
    init {
        require(baseAllowed(base, allowHttp)) { "sync base must be https" }
    }

    override fun call(method: String, path: String, bearer: String?, body: ByteArray?): HttpAnswer {
        if (method !in METHODS || !pathAllowed(path)) throw TransportException.Offline("refused request")
        val url = base.trimEnd('/') + "/api/v1/" + path
        val conn = try {
            URL(url).openConnection() as HttpURLConnection
        } catch (e: IOException) {
            throw TransportException.Offline(e.javaClass.simpleName)
        }
        try {
            conn.instanceFollowRedirects = false
            conn.useCaches = false
            conn.connectTimeout = 15_000
            conn.readTimeout = 30_000
            conn.requestMethod = method
            conn.setRequestProperty("User-Agent", userAgent)
            conn.setRequestProperty("Accept", "application/json")
            conn.setRequestProperty("Accept-Encoding", "identity")
            if (bearer != null) conn.setRequestProperty("Authorization", "Bearer $bearer")
            if (body != null) {
                conn.doOutput = true
                conn.setRequestProperty("Content-Type", "application/json")
                conn.setFixedLengthStreamingMode(body.size)
                conn.outputStream.use { it.write(body) }
            }
            val status = conn.responseCode
            val stream = if (status >= 400) conn.errorStream else conn.inputStream
            val bytes = stream?.use { readCapped(it) } ?: ByteArray(0)
            if (log) Log.i(TAG, "$method ${path.substringBefore('?')} -> $status")
            return HttpAnswer(
                status = status.toUShort(),
                body = bytes,
                retryAfter = conn.getHeaderField("Retry-After"),
                date = conn.getHeaderField("Date"),
            )
        } catch (e: TransportException) {
            if (log) Log.i(TAG, "$method ${path.substringBefore('?')} -> ${e.javaClass.simpleName}")
            throw e
        } catch (e: SSLException) {
            if (log) Log.i(TAG, "$method ${path.substringBefore('?')} -> TLS")
            throw TransportException.Tls()
        } catch (e: SocketTimeoutException) {
            if (log) Log.i(TAG, "$method ${path.substringBefore('?')} -> timeout")
            throw TransportException.Timeout()
        } catch (e: InterruptedIOException) {
            throw TransportException.Timeout()
        } catch (e: IOException) {
            if (log) Log.i(TAG, "$method ${path.substringBefore('?')} -> ${e.javaClass.simpleName}")
            throw TransportException.Offline(e.javaClass.simpleName)
        } finally {
            conn.disconnect()
        }
    }

    private fun readCapped(input: java.io.InputStream): ByteArray {
        val out = ByteArrayOutputStream()
        val buf = ByteArray(16 * 1024)
        while (true) {
            val n = input.read(buf)
            if (n < 0) break
            if (out.size() + n > MAX_BODY) throw TransportException.TooLarge()
            out.write(buf, 0, n)
        }
        return out.toByteArray()
    }

    companion object {
        const val TAG = "KrunkSyncNet"
        const val MAX_BODY = 1 shl 20
        private val METHODS = setOf("GET", "POST", "PUT", "DELETE")

        /**
         * A path below `/api/v1/` as the core makes them: no scheme, host, `..` or odd characters.
         * The comma is the core's `keys/chain?from=<ak>,<pak>` (0.7.2: without it every phone
         * failed with "couldn't reach" once the account's keys had changed).
         */
        fun pathAllowed(path: String): Boolean = path.isNotEmpty() && path.length <= 512 &&
            !path.startsWith("/") && !path.contains("..") && !path.contains("//") &&
            path.all { it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' || it in "/_-.?=&%," }

        /** Release builds: `https://` only. */
        fun baseAllowed(base: String, allowHttp: Boolean): Boolean =
            base.startsWith("https://") || (allowHttp && base.startsWith("http://"))
    }
}
