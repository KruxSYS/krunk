package org.krunk.browser.vpn

import java.io.IOException
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.net.URL
import kotlin.concurrent.thread
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/** Krunk's own CONNECT against a stand-in proxy on loopback. */
class VpnSocketsTest {
    private val token = "0f".repeat(32)

    /** A one-connection proxy: reads the head, hands it to [seen], then runs [answer]. */
    private fun proxy(seen: MutableList<String>, answer: (Socket) -> Unit): Int {
        val server = ServerSocket(0, 1, InetAddress.getByName("127.0.0.1"))
        thread(isDaemon = true) {
            server.use {
                val s = it.accept()
                s.use { c ->
                    seen += VpnSockets.readHead(c.getInputStream()).orEmpty()
                    answer(c)
                }
            }
        }
        return server.localPort
    }

    @Test
    fun connectSendsTheCredentialAndReturnsTheTunnel() {
        val seen = mutableListOf<String>()
        val port = proxy(seen) { c ->
            c.getOutputStream().write("HTTP/1.1 200 Connection established\r\n\r\nhello".toByteArray())
            c.getOutputStream().flush()
            Thread.sleep(200)
        }
        VpnSockets.connectThrough("example.com", 443, 3000, port, token).use { s ->
            val buf = ByteArray(5)
            var n = 0
            while (n < 5) n += s.getInputStream().read(buf, n, 5 - n)
            // Nothing after the proxy's head was eaten: TLS would start right here.
            assertEquals("hello", String(buf))
        }
        assertEquals("CONNECT example.com:443 HTTP/1.1", seen[0])
        assertTrue(seen.contains("Proxy-Authorization: ${VpnRoute.proxyAuthorization(token)}"))
    }

    @Test
    fun aSilentCloseIsNotConnectedNeverDirect() {
        val port = proxy(mutableListOf()) { }
        try {
            VpnSockets.connectThrough("example.com", 443, 3000, port, token)
            fail("connected through a proxy that closed")
        } catch (e: VpnNotConnected) {
            assertEquals("VPN not connected", e.message)
        }
    }

    @Test
    fun refusalsAreErrors() {
        val p503 = proxy(mutableListOf()) { it.getOutputStream().write("HTTP/1.1 503 x\r\n\r\n".toByteArray()) }
        try {
            VpnSockets.connectThrough("example.com", 443, 3000, p503, token)
            fail()
        } catch (_: VpnNotConnected) {
        }
        val p502 = proxy(mutableListOf()) { it.getOutputStream().write("HTTP/1.1 502 x\r\n\r\n".toByteArray()) }
        try {
            VpnSockets.connectThrough("example.com", 443, 3000, p502, token)
            fail()
        } catch (e: IOException) {
            assertEquals("proxy answered 502", e.message)
        }
    }

    @Test
    fun authorityBracketsIpv6() {
        assertEquals("[2001:db8::1]:443", VpnSockets.authority("2001:db8::1", 443))
        assertEquals("[::1]:8080", VpnSockets.authority("[::1]", 8080))
        assertEquals("a.example:80", VpnSockets.authority("a.example", 80))
    }

    @Test
    fun theTunnelConnectionSpeaksHttp() {
        val seen = mutableListOf<String>()
        val request = StringBuilder()
        val port = proxy(seen) { c ->
            val out = c.getOutputStream()
            out.write("HTTP/1.1 200 OK\r\n\r\n".toByteArray())
            out.flush()
            val input = c.getInputStream()
            request.append(VpnSockets.readHead(input)!!.joinToString("\n"))
            val body = ByteArray(4)
            var n = 0
            while (n < 4) n += input.read(body, n, 4 - n)
            request.append("\n").append(String(body))
            out.write(
                (
                    "HTTP/1.1 100 Continue\r\n\r\n" +
                        "HTTP/1.1 201 Created\r\nRetry-After: 5\r\nTransfer-Encoding: chunked\r\n\r\n" +
                        "3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n"
                    ).toByteArray(),
            )
            out.flush()
            Thread.sleep(200)
        }
        // Plain HTTP behind the CONNECT stands in for TLS here.
        val conn = VpnTunnelConnection(URL("https://api.example.com/api/v1/x?y=1"), port, token) { s, _, _ -> s }
        conn.connectTimeout = 3000
        conn.readTimeout = 3000
        conn.requestMethod = "PUT"
        conn.doOutput = true
        conn.setRequestProperty("Authorization", "Bearer t")
        conn.setRequestProperty("Proxy-Authorization", "must not be sent to the site")
        conn.outputStream.use { it.write("data".toByteArray()) }
        assertEquals(201, conn.responseCode)
        assertEquals("5", conn.getHeaderField("retry-after"))
        assertEquals("abcde", conn.inputStream.readBytes().decodeToString())
        conn.disconnect()
        assertEquals("CONNECT api.example.com:443 HTTP/1.1", seen[0])
        val lines = request.toString().lines()
        assertEquals("PUT /api/v1/x?y=1 HTTP/1.1", lines[0])
        assertTrue(lines.contains("Host: api.example.com"))
        assertTrue(lines.contains("Authorization: Bearer t"))
        assertTrue(lines.contains("Content-Length: 4"))
        assertTrue(lines.none { it.startsWith("Proxy-Authorization") })
        assertEquals("data", lines.last())
    }
}
