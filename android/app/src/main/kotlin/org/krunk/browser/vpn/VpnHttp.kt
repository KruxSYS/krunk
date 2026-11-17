package org.krunk.browser.vpn

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.FileNotFoundException
import java.io.FilterInputStream
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.HttpURLConnection
import java.net.Socket
import java.net.URL
import javax.net.ssl.HttpsURLConnection
import javax.net.ssl.SNIHostName
import javax.net.ssl.SSLPeerUnverifiedException
import javax.net.ssl.SSLSocket
import javax.net.ssl.SSLSocketFactory

/**
 * Krunk's own HTTP/1.1 client through the VPN's SOCKS5 proxy ([VpnSocks]), as an
 * [HttpURLConnection] so that the call sites keep their code: one request per connection
 * (`Connection: close`), no cache, no compression, answers with Content-Length, chunked or until
 * close. `https` gets the platform's TLS with SNI and host-name verification. Redirects are
 * followed (at most [MAX_REDIRECTS], GET/HEAD only, http/https only, always through the VPN)
 * when [getInstanceFollowRedirects] is set, as the platform does. [tlsSocket] is replaced in JVM
 * tests.
 */
class VpnHttp internal constructor(
    url: URL,
    private val proxyPort: Int,
    private val tokenHex: String,
    private val tlsSocket: (Socket, String, Int) -> Socket,
) : HttpURLConnection(url) {
    constructor(url: URL, proxyPort: Int, tokenHex: String) : this(url, proxyPort, tokenHex, ::defaultTls)

    /** Where the request goes now (changes with redirects). */
    private var target: URL = url
    private val requestProps = LinkedHashMap<String, String>()
    private var socket: Socket? = null
    private var body: ByteArrayOutputStream? = null
    private var responseHeaders: List<Pair<String, String>> = emptyList()
    private var responseStream: InputStream? = null
    private var answered = false
    private var status = 0
    private var statusText = ""

    init {
        useCaches = false
    }

    override fun getURL(): URL = target

    override fun setRequestProperty(key: String, value: String?) {
        super.setRequestProperty(key, value)
        requestProps.keys.firstOrNull { it.equals(key, ignoreCase = true) }?.let(requestProps::remove)
        if (value != null) requestProps[key] = value
    }

    override fun addRequestProperty(key: String, value: String?) = setRequestProperty(key, value)

    override fun getRequestProperty(key: String): String? =
        requestProps.entries.firstOrNull { it.key.equals(key, ignoreCase = true) }?.value

    override fun usingProxy(): Boolean = true

    override fun connect() {
        if (connected) return
        val u = target
        val https = u.protocol.equals("https", ignoreCase = true)
        if (!https && !u.protocol.equals("http", ignoreCase = true)) throw IOException("not http")
        val host = u.host
        val port = if (u.port > 0) u.port else u.defaultPort
        val timeout = if (connectTimeout > 0) connectTimeout else 30_000
        val plain = VpnSocks.connect(proxyPort, tokenHex, host, port, timeout)
        val s = try {
            if (https) tlsSocket(plain, host, port) else plain
        } catch (e: IOException) {
            runCatching { plain.close() }
            throw e
        }
        s.soTimeout = readTimeout
        socket = s
        connected = true
    }

    override fun getOutputStream(): OutputStream {
        if (answered) throw IOException("request already sent")
        if (!doOutput) throw IOException("doOutput is false")
        if (method == "GET") method = "POST"
        return body ?: ByteArrayOutputStream().also { body = it }
    }

    private fun answer() {
        if (answered) return
        var hops = 0
        while (true) {
            exchange()
            val location = header("Location")
            val redirect = instanceFollowRedirects && status in REDIRECTS && location != null &&
                (method == "GET" || method == "HEAD") && hops < MAX_REDIRECTS
            if (!redirect) break
            val next = runCatching { URL(target, location) }.getOrNull()
            if (next == null || !(next.protocol == "https" || next.protocol == "http")) break
            hops++
            disconnect()
            connected = false
            target = next
        }
        answered = true
    }

    /** One request and the head of its answer. */
    private fun exchange() {
        connect()
        val s = socket ?: throw IOException("not connected")
        val payload = body?.toByteArray()
        val u = target
        val path = (u.path.ifEmpty { "/" }) + (u.query?.let { "?$it" } ?: "")
        val head = StringBuilder()
        head.append(method).append(' ').append(path).append(" HTTP/1.1\r\n")
        head.append("Host: ").append(if (u.port > 0) "${u.host}:${u.port}" else u.host).append("\r\n")
        head.append("Connection: close\r\n")
        for ((k, v) in requestProps) {
            val lower = k.lowercase()
            if (lower in SKIPPED) continue
            if ('\r' in v || '\n' in v || '\r' in k || '\n' in k) continue
            head.append(k).append(": ").append(v).append("\r\n")
        }
        if (payload != null) head.append("Content-Length: ").append(payload.size).append("\r\n")
        head.append("\r\n")
        val out = s.getOutputStream()
        out.write(head.toString().toByteArray(Charsets.ISO_8859_1))
        payload?.let(out::write)
        out.flush()
        val input = s.getInputStream().buffered()
        var lines: List<String>
        var code: Int
        while (true) {
            lines = readHead(input) ?: throw IOException("connection closed without an answer")
            code = statusCode(lines.first()) ?: throw IOException("bad answer")
            // Interim answers (100 Continue, 103 Early Hints) are skipped.
            if (code !in 100..199) break
        }
        status = code
        statusText = lines.first().split(' ', limit = 3).getOrNull(2) ?: ""
        responseHeaders = lines.drop(1).mapNotNull { line ->
            val i = line.indexOf(':')
            if (i <= 0) null else line.substring(0, i).trim() to line.substring(i + 1).trim()
        }
        val noBody = method == "HEAD" || code == 204 || code == 304
        val length = header("Content-Length")?.toLongOrNull()
        responseStream = when {
            noBody -> ByteArrayInputStream(ByteArray(0))
            header("Transfer-Encoding")?.contains("chunked", ignoreCase = true) == true -> ChunkedStream(input)
            length != null -> LimitedStream(input, length)
            else -> input
        }
    }

    private fun header(name: String): String? =
        responseHeaders.lastOrNull { it.first.equals(name, ignoreCase = true) }?.second

    override fun getResponseCode(): Int {
        answer()
        return status
    }

    override fun getResponseMessage(): String {
        answer()
        return statusText
    }

    override fun getHeaderField(name: String?): String? {
        if (name == null) return getHeaderField(0)
        return runCatching { answer() }.map { header(name) }.getOrNull()
    }

    override fun getHeaderField(n: Int): String? {
        if (runCatching { answer() }.isFailure) return null
        if (n == 0) return "HTTP/1.1 $status $statusText"
        return responseHeaders.getOrNull(n - 1)?.second
    }

    override fun getHeaderFieldKey(n: Int): String? {
        if (n == 0 || runCatching { answer() }.isFailure) return null
        return responseHeaders.getOrNull(n - 1)?.first
    }

    override fun getHeaderFields(): Map<String?, List<String>> {
        if (runCatching { answer() }.isFailure) return emptyMap()
        return responseHeaders.groupBy({ it.first }, { it.second })
    }

    override fun getInputStream(): InputStream {
        answer()
        if (status >= 400) {
            if (status == 404 || status == 410) throw FileNotFoundException(target.toString())
            throw IOException("Server returned HTTP response code: $status")
        }
        return responseStream ?: ByteArrayInputStream(ByteArray(0))
    }

    override fun getErrorStream(): InputStream? = if (answered && status >= 400) responseStream else null

    override fun disconnect() {
        runCatching { socket?.close() }
        socket = null
    }

    private class LimitedStream(input: InputStream, private var left: Long) : FilterInputStream(input) {
        override fun read(): Int {
            if (left <= 0) return -1
            val b = super.read()
            if (b >= 0) left-- else throw IOException("answer ended early")
            return b
        }

        override fun read(b: ByteArray, off: Int, len: Int): Int {
            if (left <= 0) return -1
            val n = super.read(b, off, minOf(len.toLong(), left).toInt())
            if (n < 0) throw IOException("answer ended early")
            left -= n
            return n
        }
    }

    private class ChunkedStream(private val input: InputStream) : InputStream() {
        private var left = 0L
        private var done = false

        private fun line(): String {
            val sb = StringBuilder()
            while (true) {
                val b = input.read()
                if (b < 0) throw IOException("answer ended early")
                if (b == '\n'.code) return sb.toString().trimEnd('\r')
                sb.append(b.toChar())
                if (sb.length > 1024) throw IOException("bad chunk")
            }
        }

        private fun next(): Boolean {
            if (done) return false
            if (left > 0) return true
            val size = line().substringBefore(';').trim().toLongOrNull(16) ?: throw IOException("bad chunk")
            if (size == 0L) {
                // Trailer fields, up to the empty line.
                var trailer = line()
                while (trailer.isNotEmpty()) trailer = line()
                done = true
                return false
            }
            left = size
            return true
        }

        override fun read(): Int {
            val one = ByteArray(1)
            return if (read(one, 0, 1) < 0) -1 else one[0].toInt() and 0xff
        }

        override fun read(b: ByteArray, off: Int, len: Int): Int {
            if (len == 0) return 0
            if (!next()) return -1
            val n = input.read(b, off, minOf(len.toLong(), left).toInt())
            if (n < 0) throw IOException("answer ended early")
            left -= n
            if (left == 0L) line()
            return n
        }
    }

    companion object {
        const val MAX_REDIRECTS = 5
        private val REDIRECTS = setOf(301, 302, 303, 307, 308)
        private val SKIPPED = setOf("host", "connection", "content-length", "transfer-encoding")
        private const val MAX_HEAD = 64 * 1024

        /** TLS over a tunnel socket with [factory]: SNI for names, the platform's name check. */
        fun tls(plain: Socket, host: String, port: Int, factory: SSLSocketFactory = defaultFactory()): SSLSocket {
            val tls = factory.createSocket(plain, host, port, true) as SSLSocket
            try {
                tls.sslParameters = tls.sslParameters.apply {
                    endpointIdentificationAlgorithm = "HTTPS"
                    if (!isAddress(host)) serverNames = listOf(SNIHostName(host))
                }
                tls.startHandshake()
                return tls
            } catch (e: IOException) {
                runCatching { tls.close() }
                throw e
            }
        }

        private fun defaultFactory() = SSLSocketFactory.getDefault() as SSLSocketFactory

        private fun defaultTls(plain: Socket, host: String, port: Int): Socket {
            val tls = tls(plain, host, port)
            if (!HttpsURLConnection.getDefaultHostnameVerifier().verify(host, tls.session)) {
                runCatching { tls.close() }
                throw SSLPeerUnverifiedException("certificate does not match $host")
            }
            return tls
        }

        private fun isAddress(host: String) = ':' in host || host.all { it in '0'..'9' || it == '.' }

        /** "HTTP/1.1 200 OK" → 200. */
        fun statusCode(line: String?): Int? {
            val parts = line?.split(' ') ?: return null
            if (parts.size < 2 || !parts[0].startsWith("HTTP/1.")) return null
            return parts[1].toIntOrNull()?.takeIf { it in 100..999 }
        }

        /** The lines of an HTTP head; null on EOF before its first byte. */
        fun readHead(input: InputStream): List<String>? {
            val buf = ByteArrayOutputStream()
            var last = 0
            while (true) {
                val b = input.read()
                if (b < 0) {
                    if (buf.size() == 0) return null
                    throw IOException("connection closed in the answer's head")
                }
                buf.write(b)
                last = (last shl 8) or b
                if (buf.size() > MAX_HEAD) throw IOException("answer head too long")
                if (last == 0x0d0a0d0a) {
                    val bytes = buf.toByteArray()
                    return String(bytes, 0, bytes.size - 4, Charsets.ISO_8859_1).split("\r\n")
                }
            }
        }
    }
}
