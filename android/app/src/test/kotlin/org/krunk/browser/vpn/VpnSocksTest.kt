package org.krunk.browser.vpn

import java.io.ByteArrayOutputStream
import java.io.DataInputStream
import java.io.IOException
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.net.URL
import java.net.UnknownHostException
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/** Krunk's own SOCKS5 client and HTTP client against a stand-in proxy on loopback. */
class VpnSocksTest {
    private val token = "0f".repeat(32)

    /** What the stand-in saw: the greeting, the RFC 1929 request and the CONNECT request. */
    private class Seen(val greeting: ByteArray, val auth: ByteArray?, val connect: ByteArray?)

    private enum class Stop { AFTER_GREETING, AFTER_AUTH, NEVER }

    /**
     * A one-connection SOCKS5 stand-in: checks nothing itself, records the bytes, answers [rep]
     * (or closes early per [stop]), then runs [then] on the tunnel.
     */
    private fun proxy(
        seen: LinkedBlockingQueue<Seen>,
        stop: Stop = Stop.NEVER,
        rep: Int = 0,
        then: (Socket, DataInputStream) -> Unit = { _, _ -> },
    ): Int {
        val server = ServerSocket(0, 1, InetAddress.getByName("127.0.0.1"))
        thread(isDaemon = true) {
            server.use {
                it.accept().use { c ->
                    val input = DataInputStream(c.getInputStream())
                    val out = c.getOutputStream()
                    val greeting = ByteArray(3).also(input::readFully)
                    if (stop == Stop.AFTER_GREETING) {
                        seen.put(Seen(greeting, null, null))
                        return@use
                    }
                    out.write(byteArrayOf(5, 2))
                    val auth = ByteArrayOutputStream()
                    val ver = input.readUnsignedByte()
                    val ulen = input.readUnsignedByte()
                    val user = ByteArray(ulen).also(input::readFully)
                    val plen = input.readUnsignedByte()
                    val pass = ByteArray(plen).also(input::readFully)
                    auth.write(ver)
                    auth.write(ulen)
                    auth.write(user)
                    auth.write(plen)
                    auth.write(pass)
                    if (stop == Stop.AFTER_AUTH) {
                        seen.put(Seen(greeting, auth.toByteArray(), null))
                        return@use
                    }
                    out.write(byteArrayOf(1, 0))
                    val req = ByteArrayOutputStream()
                    val head = ByteArray(4).also(input::readFully)
                    req.write(head)
                    when (head[3].toInt()) {
                        1 -> req.write(ByteArray(4).also(input::readFully))

                        4 -> req.write(ByteArray(16).also(input::readFully))

                        3 -> {
                            val n = input.readUnsignedByte()
                            req.write(n)
                            req.write(ByteArray(n).also(input::readFully))
                        }
                    }
                    req.write(ByteArray(2).also(input::readFully))
                    seen.put(Seen(greeting, auth.toByteArray(), req.toByteArray()))
                    out.write(byteArrayOf(5, rep.toByte(), 0, 1, 0, 0, 0, 0, 0, 0))
                    out.flush()
                    if (rep == 0) then(c, input)
                }
            }
        }
        return server.localPort
    }

    private fun take(q: LinkedBlockingQueue<Seen>) = q.poll(5, TimeUnit.SECONDS) ?: error("proxy saw nothing")

    @Test
    fun theHandshakeBytes() {
        val q = LinkedBlockingQueue<Seen>()
        val port = proxy(q) { c, _ ->
            c.getOutputStream().write("hello".toByteArray())
            c.getOutputStream().flush()
            Thread.sleep(200)
        }
        VpnSocks.connect(port, token, "example.com", 443, 3000).use { s ->
            val buf = ByteArray(5)
            DataInputStream(s.getInputStream()).readFully(buf)
            // Nothing after the proxy's reply was eaten: TLS would start right here.
            assertEquals("hello", String(buf))
        }
        val seen = take(q)
        assertArrayEquals(byteArrayOf(5, 1, 2), seen.greeting)
        assertArrayEquals(
            byteArrayOf(1, 5) + "krunk".toByteArray() + byteArrayOf(64) + token.toByteArray(),
            seen.auth,
        )
        // CONNECT, ATYP 3 with the name unresolved, port 443 big-endian.
        assertArrayEquals(
            byteArrayOf(5, 1, 0, 3, 11) + "example.com".toByteArray() + byteArrayOf(1, 0xbb.toByte()),
            seen.connect,
        )
    }

    @Test
    fun addressLiteralsAreSentAsAddresses() {
        assertArrayEquals(byteArrayOf(1, 10, 0, 0, 1), VpnSocks.targetAddress("10.0.0.1"))
        val v6 = VpnSocks.targetAddress("[2001:db8::1]")
        assertEquals(17, v6.size)
        assertEquals(4.toByte(), v6[0])
        assertEquals(0x20.toByte(), v6[1])
        assertEquals(1.toByte(), v6[16])
        // Names, even ones that look odd, go as names (the proxy resolves them).
        assertArrayEquals(byteArrayOf(3, 7) + "1.2.3.x".toByteArray(), VpnSocks.targetAddress("1.2.3.x"))
        assertArrayEquals(byteArrayOf(3, 16) + "xn--bcher-kva.de".toByteArray(), VpnSocks.targetAddress("bücher.de"))
    }

    @Test
    fun aCloseAfterTheGreetingOrTheCredentialIsNotConnected() {
        for (stop in listOf(Stop.AFTER_GREETING, Stop.AFTER_AUTH)) {
            val q = LinkedBlockingQueue<Seen>()
            val port = proxy(q, stop = stop)
            try {
                VpnSocks.connect(port, token, "example.com", 443, 3000)
                fail("connected through a proxy that closed ($stop)")
            } catch (e: VpnNotConnected) {
                assertEquals("VPN not connected", e.message)
            }
            take(q)
        }
    }

    @Test
    fun noProxyIsNotConnected() {
        val free = ServerSocket(0, 1, InetAddress.getByName("127.0.0.1")).use { it.localPort }
        try {
            VpnSocks.connect(free, token, "example.com", 443, 3000)
            fail()
        } catch (_: VpnNotConnected) {
        }
    }

    @Test
    fun replyCodes() {
        fun failure(rep: Int): IOException {
            val port = proxy(LinkedBlockingQueue(), rep = rep)
            try {
                VpnSocks.connect(port, token, "example.com", 443, 3000)
            } catch (e: IOException) {
                return e
            }
            error("connected with reply $rep")
        }
        assertTrue(failure(1) is VpnNotConnected)
        assertTrue(failure(2) is VpnNotReachable)
        assertEquals("Not reachable through this VPN", failure(2).message)
        assertTrue(failure(4) is UnknownHostException)
        assertTrue(failure(5) is java.net.ConnectException)
        assertTrue(failure(6) is java.net.SocketTimeoutException)
    }

    /** An HTTP server behind the stand-in: reads one request, records it, writes [answer]. */
    private fun httpBehind(q: LinkedBlockingQueue<Seen>, request: LinkedBlockingQueue<String>, answer: String): Int =
        proxy(q) { c, input ->
            val head = VpnHttp.readHead(input)!!
            val length = head.firstOrNull { it.startsWith("Content-Length: ") }?.substringAfter(": ")?.toInt() ?: 0
            val body = ByteArray(length).also(input::readFully)
            request.put(head.joinToString("\n") + "\n" + String(body))
            c.getOutputStream().write(answer.toByteArray())
            c.getOutputStream().flush()
            Thread.sleep(200)
        }

    @Test
    fun httpWithAChunkedAnswer() {
        val q = LinkedBlockingQueue<Seen>()
        val request = LinkedBlockingQueue<String>()
        val port = httpBehind(
            q,
            request,
            "HTTP/1.1 100 Continue\r\n\r\n" +
                "HTTP/1.1 201 Created\r\nRetry-After: 5\r\nTransfer-Encoding: chunked\r\n\r\n" +
                "3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n",
        )
        // Plain HTTP behind the tunnel stands in for TLS here.
        val conn = VpnHttp(URL("https://api.example.com/api/v1/x?y=1"), port, token) { s, _, _ -> s }
        conn.connectTimeout = 3000
        conn.readTimeout = 3000
        conn.requestMethod = "PUT"
        conn.doOutput = true
        conn.setRequestProperty("Authorization", "Bearer t")
        conn.outputStream.use { it.write("data".toByteArray()) }
        assertEquals(201, conn.responseCode)
        assertEquals("5", conn.getHeaderField("retry-after"))
        assertEquals("abcde", conn.inputStream.readBytes().decodeToString())
        conn.disconnect()
        assertArrayEquals(
            byteArrayOf(5, 1, 0, 3, 15) + "api.example.com".toByteArray() + byteArrayOf(1, 0xbb.toByte()),
            take(q).connect,
        )
        val lines = request.poll(5, TimeUnit.SECONDS)!!.lines()
        assertEquals("PUT /api/v1/x?y=1 HTTP/1.1", lines[0])
        assertTrue(lines.contains("Host: api.example.com"))
        assertTrue(lines.contains("Connection: close"))
        assertTrue(lines.contains("Authorization: Bearer t"))
        assertTrue(lines.contains("Content-Length: 4"))
        assertEquals("data", lines.last())
    }

    @Test
    fun httpWithContentLengthAndAnErrorStatus() {
        val q = LinkedBlockingQueue<Seen>()
        val request = LinkedBlockingQueue<String>()
        val port = httpBehind(q, request, "HTTP/1.1 429 Too Many\r\nContent-Length: 7\r\n\r\nslow upEXTRA")
        val conn = VpnHttp(URL("http://example.com:8080/a"), port, token)
        conn.readTimeout = 3000
        assertEquals(429, conn.responseCode)
        assertEquals("Too Many", conn.responseMessage)
        try {
            conn.inputStream
            fail()
        } catch (_: IOException) {
        }
        assertEquals("slow up", conn.errorStream!!.readBytes().decodeToString())
        conn.disconnect()
        val connect = take(q).connect!!
        // Port 8080 big-endian.
        assertEquals(0x1f.toByte(), connect[connect.size - 2])
        assertEquals(0x90.toByte(), connect[connect.size - 1])
        val lines = request.poll(5, TimeUnit.SECONDS)!!.lines()
        assertEquals("GET /a HTTP/1.1", lines[0])
        assertTrue(lines.contains("Host: example.com:8080"))
    }
}
