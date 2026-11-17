package org.krunk.browser.vpn

import java.io.EOFException
import java.io.IOException
import java.io.InputStream
import java.net.ConnectException
import java.net.IDN
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.Socket
import java.net.SocketTimeoutException
import java.net.UnknownHostException

/** The VPN's proxy refused a destination as outside the profile's routes (SOCKS reply 2). */
class VpnNotReachable : IOException("Not reachable through this VPN")

/**
 * Krunk's own connections through the VPN's proxy: SOCKS5 (RFC 1928) with username/password
 * (RFC 1929), written out by hand — never `java.net.Authenticator`, which is process-wide and
 * could hand the token to other hosts, and never the platform's proxy support.
 *
 * The target's name goes to the proxy unresolved (ATYP 3), so the lookup happens through the
 * tunnel; only address literals are sent as addresses. Nothing here asks the system's resolver.
 *
 * The proxy closes a connection with a missing or wrong credential without a reply: an EOF
 * anywhere in the handshake is [VpnNotConnected], never a reason to go direct.
 */
object VpnSocks {
    const val USER = "krunk"
    private val LOOPBACK: InetAddress = InetAddress.getByAddress(byteArrayOf(127, 0, 0, 1))

    /** [connect] through the proxy of [VpnRoute.state]; [VpnNotConnected] when there is none now. */
    @Throws(IOException::class)
    fun connect(host: String, port: Int, timeoutMs: Int): Socket {
        val s = VpnRoute.state
        val proxyPort = s.port ?: throw VpnNotConnected()
        val pass = s.tokenHex ?: throw VpnNotConnected()
        return connect(proxyPort, pass, host, port, timeoutMs)
    }

    /**
     * A socket to [host]:[port] through the SOCKS5 proxy on 127.0.0.1:[proxyPort], ready for
     * TLS or plain HTTP; the caller closes it. [passHex] is the session token (64 hex chars).
     */
    @Throws(IOException::class)
    fun connect(proxyPort: Int, passHex: String, host: String, port: Int, timeoutMs: Int): Socket {
        require(port in 1..65535) { "port" }
        val target = targetAddress(host)
        val socket = Socket()
        val pass = passHex.toByteArray(Charsets.US_ASCII)
        try {
            socket.soTimeout = timeoutMs
            socket.tcpNoDelay = true
            try {
                socket.connect(InetSocketAddress(LOOPBACK, proxyPort), timeoutMs)
            } catch (_: ConnectException) {
                throw VpnNotConnected()
            }
            val out = socket.getOutputStream()
            val input = socket.getInputStream()
            // Greeting: version 5, one method, username/password.
            out.write(byteArrayOf(5, 1, 2))
            out.flush()
            val method = readOrNotConnected(input, 2)
            if (method[0] != 5.toByte() || method[1] != 2.toByte()) throw VpnNotConnected()
            // RFC 1929: version 1, user, password.
            val user = USER.toByteArray(Charsets.US_ASCII)
            require(pass.size in 1..255) { "token" }
            val auth = byteArrayOf(1, user.size.toByte()) + user + byteArrayOf(pass.size.toByte()) + pass
            out.write(auth)
            out.flush()
            auth.fill(0)
            val authReply = readOrNotConnected(input, 2)
            if (authReply[0] != 1.toByte() || authReply[1] != 0.toByte()) throw VpnNotConnected()
            // CONNECT.
            out.write(byteArrayOf(5, 1, 0) + target + byteArrayOf((port shr 8).toByte(), port.toByte()))
            out.flush()
            val head = readOrNotConnected(input, 4)
            if (head[0] != 5.toByte()) throw IOException("bad SOCKS reply")
            val rep = head[1].toInt() and 0xff
            if (rep != 0) throw failure(rep, host)
            // The bound address (unused) and port.
            val skip = when (head[3].toInt()) {
                1 -> 4
                4 -> 16
                3 -> readOrNotConnected(input, 1)[0].toInt() and 0xff
                else -> throw IOException("bad SOCKS reply")
            }
            readOrNotConnected(input, skip + 2)
            return socket
        } catch (e: IOException) {
            runCatching { socket.close() }
            throw e
        } catch (e: IllegalArgumentException) {
            runCatching { socket.close() }
            throw IOException(e.message)
        } finally {
            pass.fill(0)
        }
    }

    /** What a non-zero SOCKS reply means for Krunk's own requests. */
    fun failure(rep: Int, host: String): IOException = when (rep) {
        1 -> VpnNotConnected()
        2 -> VpnNotReachable()
        4 -> UnknownHostException(host)
        5 -> ConnectException("refused by $host")
        6 -> SocketTimeoutException("no answer from $host")
        else -> IOException("SOCKS reply $rep")
    }

    /**
     * ATYP and address of [host]: an IPv4 literal (1), an IPv6 literal (4), else the name in
     * ASCII form (3). Literals are parsed here, never looked up.
     */
    fun targetAddress(host: String): ByteArray {
        val h = host.removePrefix("[").removeSuffix("]")
        ipv4(h)?.let { return byteArrayOf(1) + it }
        if (':' in h) {
            // An IPv6 literal (a name never has a colon): parsed, not resolved.
            val v6 = ipv6(h) ?: throw IOException("bad address")
            return byteArrayOf(4) + v6
        }
        val ascii = try {
            IDN.toASCII(h.trimEnd('.'))
        } catch (_: IllegalArgumentException) {
            throw UnknownHostException(host)
        }.toByteArray(Charsets.US_ASCII)
        if (ascii.isEmpty() || ascii.size > 255) throw UnknownHostException(host)
        return byteArrayOf(3, ascii.size.toByte()) + ascii
    }

    private fun ipv4(h: String): ByteArray? {
        val parts = h.split('.')
        if (parts.size != 4) return null
        val out = ByteArray(4)
        for ((i, p) in parts.withIndex()) {
            if (p.isEmpty() || p.length > 3 || !p.all { it in '0'..'9' }) return null
            val n = p.toInt()
            if (n > 255) return null
            out[i] = n.toByte()
        }
        return out
    }

    private fun ipv6(h: String): ByteArray? {
        val text = h.substringBefore('%')
        if (text.count { it == ':' } < 2 || !text.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' || it == ':' || it == '.' }) {
            return null
        }
        // Only hex digits, colons and dots: the platform parses a literal without any lookup.
        return runCatching { InetAddress.getByName(text).address.takeIf { it.size == 16 } }.getOrNull()
    }

    private fun readOrNotConnected(input: InputStream, n: Int): ByteArray {
        val buf = ByteArray(n)
        var got = 0
        while (got < n) {
            val r = try {
                input.read(buf, got, n - got)
            } catch (_: EOFException) {
                -1
            }
            if (r < 0) throw VpnNotConnected()
            got += r
        }
        return buf
    }
}
