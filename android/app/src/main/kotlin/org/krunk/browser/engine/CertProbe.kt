package org.krunk.browser.engine

import android.util.Log
import java.net.InetSocketAddress
import java.security.cert.X509Certificate
import javax.net.ssl.SNIHostName
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLSocket
import javax.net.ssl.X509ExtendedTrustManager
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.krunk.browser.KrunkApp
import org.krunk.browser.vpn.Route
import org.krunk.browser.vpn.VpnNotConnected
import org.krunk.browser.vpn.VpnRoute
import org.krunk.browser.vpn.VpnSocks

/**
 * The certificate a site presents, read from the site itself.
 *
 * GeckoView 160 does not give the certificate of a failed load to `onLoadError` (its
 * `WebRequestError.certificate` is always null), and the security information it reports for the
 * error page carries none either. So Krunk reads it with a TLS connection of its own: same host,
 * same port, no HTTP request, closed as soon as the handshake is done. Whatever the certificate
 * is, it is taken — the engine already refused it, which is why the error screen is shown — and a
 * site that cannot be reached this way simply gets no "Continue anyway" (the engine's own error
 * page stays).
 *
 * The connection is unauthenticated by design (it only exists to see the certificate) and sends
 * nothing but the handshake.
 *
 * With the VPN on (0.10.0) the probe takes the same way as the page ([VpnRoute], the private flag
 * of the tab): through the VPN's SOCKS5 proxy ([VpnSocks], the name resolved there), or not at all while it is down.
 */
object CertProbe {
    private const val CONNECT_TIMEOUT_MS = 5_000
    private const val READ_TIMEOUT_MS = 5_000

    /** The site's certificate, or null when it cannot be read (unreachable, not TLS, too slow). */
    suspend fun fetch(host: String, port: Int, forPrivateTab: Boolean): X509Certificate? = withContext(Dispatchers.IO) {
        runCatching { handshake(host, port, forPrivateTab) }
            .onFailure { Log.d(KrunkApp.TAG, "certificate probe of ${CertErrors.hostPort(host, port)} failed: $it") }
            .getOrNull()
    }

    private fun handshake(host: String, port: Int, forPrivateTab: Boolean): X509Certificate? {
        val context = SSLContext.getInstance("TLS")
        context.init(null, arrayOf(TrustAny), null)
        val socket = when (VpnRoute.decide(host, forPrivateTab)) {
            Route.BLOCKED -> throw VpnNotConnected()

            Route.PROXY -> {
                val tunnel = VpnSocks.connect(host, port, CONNECT_TIMEOUT_MS)
                try {
                    context.socketFactory.createSocket(tunnel, host, port, true) as SSLSocket
                } catch (e: Exception) {
                    runCatching { tunnel.close() }
                    throw e
                }
            }

            Route.DIRECT -> (context.socketFactory.createSocket() as SSLSocket).also {
                try {
                    it.soTimeout = READ_TIMEOUT_MS
                    it.connect(InetSocketAddress(host, port), CONNECT_TIMEOUT_MS)
                } catch (e: Exception) {
                    runCatching { it.close() }
                    throw e
                }
            }
        }
        try {
            socket.soTimeout = READ_TIMEOUT_MS
            // The name the site is reached by, as a browser sends it (never for an IP address).
            if (!isIpAddress(host)) {
                socket.sslParameters = socket.sslParameters.apply { serverNames = listOf(SNIHostName(host)) }
            }
            socket.startHandshake()
            return socket.session.peerCertificates.firstOrNull() as? X509Certificate
        } finally {
            runCatching { socket.close() }
        }
    }

    /** An IPv4 or IPv6 literal, which has no server name to send. */
    fun isIpAddress(host: String): Boolean = ':' in host ||
        (host.isNotEmpty() && host.all { it in '0'..'9' || it == '.' } && '.' in host)

    /**
     * Accepts every certificate: the engine has already judged this one and refused it, and the
     * point of the probe is to look at it, not to trust it. Nothing is sent over the connection.
     */
    private object TrustAny : X509ExtendedTrustManager() {
        override fun checkClientTrusted(chain: Array<out X509Certificate>?, authType: String?) = Unit

        override fun checkServerTrusted(chain: Array<out X509Certificate>?, authType: String?) = Unit

        override fun checkClientTrusted(
            chain: Array<out X509Certificate>?,
            authType: String?,
            socket: java.net.Socket?,
        ) = Unit

        override fun checkServerTrusted(
            chain: Array<out X509Certificate>?,
            authType: String?,
            socket: java.net.Socket?,
        ) = Unit

        override fun checkClientTrusted(
            chain: Array<out X509Certificate>?,
            authType: String?,
            engine: javax.net.ssl.SSLEngine?,
        ) = Unit

        override fun checkServerTrusted(
            chain: Array<out X509Certificate>?,
            authType: String?,
            engine: javax.net.ssl.SSLEngine?,
        ) = Unit

        override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
    }
}
