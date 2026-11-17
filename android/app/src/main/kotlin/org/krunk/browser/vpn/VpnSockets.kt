package org.krunk.browser.vpn

import java.io.ByteArrayOutputStream
import java.io.FilterInputStream
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.Socket
import javax.net.ssl.SSLSocket
import javax.net.ssl.SSLSocketFactory

/**
 * Krunk's own CONNECT through the VPN's proxy, for what does not go through HttpURLConnection
 * (the certificate probe) and for [VpnTunnelConnection].
 *
 * The proxy closes a connection with a missing or wrong credential without any answer: an EOF
 * before the reply is [VpnNotConnected], never a reason to go direct.
 */
object VpnSockets {
    const val LOOPBACK = "127.0.0.1"
    private const val MAX_HEAD = 8 * 1024

    /**
     * A socket to [host]:[port] through the proxy of [VpnRoute.state], ready for TLS; the caller
     * closes it. [VpnNotConnected] when there is no proxy now.
     */
    @Throws(IOException::class)
    fun connectThrough(host: String, port: Int, timeoutMs: Int): Socket {
        val s = VpnRoute.state
        val proxyPort = s.port ?: throw VpnNotConnected()
        val token = s.tokenHex ?: throw VpnNotConnected()
        return connectThrough(host, port, timeoutMs, proxyPort, token)
    }

    @Throws(IOException::class)
    fun connectThrough(host: String, port: Int, timeoutMs: Int, proxyPort: Int, tokenHex: String): Socket {
        val socket = Socket()
        try {
            socket.soTimeout = timeoutMs
            socket.tcpNoDelay = true
            socket.connect(InetSocketAddress(InetAddress.getByName(LOOPBACK), proxyPort), timeoutMs)
            val target = authority(host, port)
            val head = "CONNECT $target HTTP/1.1\r\n" +
                "Host: $target\r\n" +
                "Proxy-Authorization: ${VpnRoute.proxyAuthorization(tokenHex)}\r\n" +
                "\r\n"
            socket.getOutputStream().apply {
                write(head.toByteArray(Charsets.US_ASCII))
                flush()
            }
            val reply = readHead(socket.getInputStream()) ?: throw VpnNotConnected()
            val code = statusCode(reply.firstOrNull()) ?: throw IOException("bad proxy reply")
            when (code) {
                200 -> return socket
                503 -> throw VpnNotConnected()
                else -> throw IOException("proxy answered $code")
            }
        } catch (e: IOException) {
            runCatching { socket.close() }
            throw e
        }
    }

    /** `host:port`, IPv6 literals in brackets. */
    fun authority(host: String, port: Int): String {
        val h = host.removePrefix("[").removeSuffix("]")
        return if (':' in h) "[$h]:$port" else "$h:$port"
    }

    /** "HTTP/1.1 200 Connection established" → 200. */
    fun statusCode(line: String?): Int? {
        val parts = line?.split(' ') ?: return null
        if (parts.size < 2 || !parts[0].startsWith("HTTP/1.")) return null
        return parts[1].toIntOrNull()?.takeIf { it in 100..999 }
    }

    /**
     * The lines of an HTTP head, read byte by byte (nothing after it is consumed: TLS follows).
     * Null on EOF before the first byte.
     */
    @Throws(IOException::class)
    fun readHead(input: InputStream): List<String>? {
        val buf = ByteArrayOutputStream()
        while (true) {
            val b = input.read()
            if (b < 0) {
                if (buf.size() == 0) return null
                throw IOException("proxy closed the connection")
            }
            buf.write(b)
            if (buf.size() > MAX_HEAD) throw IOException("proxy reply too long")
            val bytes = buf.toByteArray()
            val n = bytes.size
            if (n >= 4 && bytes[n - 4] == '\r'.code.toByte() && bytes[n - 3] == '\n'.code.toByte() &&
                bytes[n - 2] == '\r'.code.toByte() && bytes[n - 1] == '\n'.code.toByte()
            ) {
                return String(bytes, 0, n - 4, Charsets.ISO_8859_1).split("\r\n")
            }
        }
    }
}

/**
 * An [SSLSocketFactory] whose sockets reach their host through [VpnSockets.connectThrough]. Only
 * the `createSocket(host, port)` forms make sense; wrapping an already connected socket would
 * mean a connection was opened outside the VPN, so that form is refused.
 */
class TunnelSslSocketFactory(
    private val timeoutMs: Int,
    private val tls: SSLSocketFactory = SSLSocketFactory.getDefault() as SSLSocketFactory,
) : SSLSocketFactory() {
    override fun getDefaultCipherSuites(): Array<String> = tls.defaultCipherSuites

    override fun getSupportedCipherSuites(): Array<String> = tls.supportedCipherSuites

    override fun createSocket(host: String, port: Int): Socket {
        val plain = VpnSockets.connectThrough(host, port, timeoutMs)
        return try {
            (tls.createSocket(plain, host, port, true) as SSLSocket).apply {
                // The name is checked as the platform's HttpsURLConnection does.
                sslParameters = sslParameters.apply { endpointIdentificationAlgorithm = "HTTPS" }
            }
        } catch (e: IOException) {
            runCatching { plain.close() }
            throw e
        }
    }

    override fun createSocket(s: Socket?, host: String?, port: Int, autoClose: Boolean): Socket =
        throw IOException("refused: a socket opened outside the VPN")

    override fun createSocket(host: String, port: Int, localHost: InetAddress?, localPort: Int): Socket =
        createSocket(host, port)

    override fun createSocket(host: InetAddress, port: Int): Socket = createSocket(host.hostAddress ?: "", port)

    override fun createSocket(address: InetAddress, port: Int, localAddress: InetAddress?, localPort: Int): Socket =
        createSocket(address, port)
}

/**
 * A minimal HTTP/1.1 client over Krunk's own CONNECT ([VpnRoute.useConnectHelper]): one request
 * per connection (`Connection: close`), no redirects, no cache, no compression, answers with
 * Content-Length, chunked or until close. It covers what Krunk's own call sites use of
 * HttpURLConnection. [tlsSocket] is replaced in JVM tests (plain HTTP behind the CONNECT).
 */
class VpnTunnelConnection internal constructor(
    url: java.net.URL,
    private val proxyPort: Int,
    private val tokenHex: String,
    private val tlsSocket: (Socket, String, Int) -> Socket,
) : java.net.HttpURLConnection(url) {
    constructor(url: java.net.URL, proxyPort: Int, tokenHex: String) : this(url, proxyPort, tokenHex, ::defaultTls)

    private val requestProps = LinkedHashMap<String, String>()
    private var socket: Socket? = null
    private var body: ByteArrayOutputStream? = null
    private var responseHeaders: List<Pair<String, String>> = emptyList()
    private var responseStream: InputStream? = null
    private var answered = false
    private var status = 0
    private var statusText = ""

    init {
        instanceFollowRedirects = false
        useCaches = false
    }

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
        val host = url.host
        val port = if (url.port > 0) url.port else url.defaultPort
        val timeout = if (connectTimeout > 0) connectTimeout else 30_000
        val plain = VpnSockets.connectThrough(host, port, timeout, proxyPort, tokenHex)
        val s = try {
            tlsSocket(plain, host, port)
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
        connect()
        val s = socket ?: throw IOException("not connected")
        val payload = body?.toByteArray()
        val path = (url.path.ifEmpty { "/" }) + (url.query?.let { "?$it" } ?: "")
        val head = StringBuilder()
        head.append(method).append(' ').append(path).append(" HTTP/1.1\r\n")
        head.append("Host: ").append(if (url.port > 0) "${url.host}:${url.port}" else url.host).append("\r\n")
        head.append("Connection: close\r\n")
        for ((k, v) in requestProps) {
            val lower = k.lowercase()
            if (lower == "host" || lower == "connection" || lower == "proxy-authorization" || lower == "content-length") continue
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
            lines = VpnSockets.readHead(input) ?: throw IOException("connection closed without an answer")
            code = VpnSockets.statusCode(lines.first()) ?: throw IOException("bad answer")
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
            noBody -> java.io.ByteArrayInputStream(ByteArray(0))
            header("Transfer-Encoding")?.contains("chunked", ignoreCase = true) == true -> ChunkedStream(input)
            length != null -> LimitedStream(input, length)
            else -> input
        }
        answered = true
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
            if (status == 404 || status == 410) throw java.io.FileNotFoundException(url.toString())
            throw IOException("Server returned HTTP response code: $status")
        }
        return responseStream ?: java.io.ByteArrayInputStream(ByteArray(0))
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
                while (line().isNotEmpty()) Unit
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
        private fun defaultTls(plain: Socket, host: String, port: Int): Socket {
            val factory = SSLSocketFactory.getDefault() as SSLSocketFactory
            val tls = factory.createSocket(plain, host, port, true) as SSLSocket
            tls.sslParameters = tls.sslParameters.apply { endpointIdentificationAlgorithm = "HTTPS" }
            tls.startHandshake()
            if (!javax.net.ssl.HttpsURLConnection.getDefaultHostnameVerifier().verify(host, tls.session)) {
                runCatching { tls.close() }
                throw javax.net.ssl.SSLPeerUnverifiedException("certificate does not match $host")
            }
            return tls
        }
    }
}
