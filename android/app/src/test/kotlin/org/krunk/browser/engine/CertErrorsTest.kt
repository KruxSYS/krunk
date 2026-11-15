package org.krunk.browser.engine

import java.security.cert.X509Certificate
import java.util.Base64
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.mozilla.geckoview.WebRequestError

class CertErrorsTest {
    @Test
    fun hostAndPortOfHttpsAddresses() {
        assertEquals("self-signed.example" to 443, CertErrors.hostAndPort("https://Self-Signed.example/path?q#f"))
        assertEquals("example.com" to 8443, CertErrors.hostAndPort("https://user:pw@example.com:8443/"))
        assertEquals("10.0.2.2" to 4443, CertErrors.hostAndPort("https://10.0.2.2:4443"))
        assertEquals("::1" to 443, CertErrors.hostAndPort("https://[::1]/"))
        assertEquals("::1" to 8443, CertErrors.hostAndPort("https://[::1]:8443/"))
        assertEquals("example.com" to 443, CertErrors.hostAndPort("https://example.com./"))
        assertEquals("xn--bcher-kva.example" to 443, CertErrors.hostAndPort("https://bücher.example/"))
        assertEquals("example.com" to 443, CertErrors.hostAndPort("https://example.com:/"))
    }

    @Test
    fun noHostAndPortForOtherAddresses() {
        assertNull(CertErrors.hostAndPort("http://example.com/"))
        assertNull(CertErrors.hostAndPort("data:text/html,x"))
        assertNull(CertErrors.hostAndPort("https:///path"))
        assertNull(CertErrors.hostAndPort("https://example.com:0/"))
        assertNull(CertErrors.hostAndPort("https://example.com:70000/"))
        assertNull(CertErrors.hostAndPort("https://example.com:44x/"))
        assertNull(CertErrors.hostAndPort("https://exa mple.com/"))
        assertNull(CertErrors.hostAndPort("https://[::1/"))
        assertNull(CertErrors.hostAndPort("https://[::1]x/"))
    }

    @Test
    fun hostPortText() {
        assertEquals("example.com:443", CertErrors.hostPort("example.com", 443))
        assertEquals("[::1]:8443", CertErrors.hostPort("::1", 8443))
    }

    @Test
    fun fingerprintIsTheEnginesSpelling() {
        // SHA-256 of "abc"
        val fp = CertErrors.sha256("abc".toByteArray())
        assertEquals(
            "BA:78:16:BF:8F:01:CF:EA:41:41:40:DE:5D:AE:22:23:B0:03:61:A3:96:17:7A:9C:B4:10:FF:61:F2:00:15:AD",
            fp,
        )
        assertEquals(fp, CertErrors.normalizeFingerprint(fp.lowercase().replace(":", "")))
        assertEquals("BA:78:16:BF:8F:01:CF:EA…", CertErrors.shortFingerprint(fp))
        assertNull(CertErrors.normalizeFingerprint("BA:78"))
        assertNull(CertErrors.normalizeFingerprint("ZZ".repeat(32)))
    }

    @Test
    fun onlyBadCertificatesMayBeOverridden() {
        val url = "https://self-signed.example/"
        assertTrue(CertErrors.overridable(WebRequestError.ERROR_SECURITY_BAD_CERT, true, url))
        assertFalse(CertErrors.overridable(WebRequestError.ERROR_SECURITY_BAD_CERT, false, url))
        assertFalse(CertErrors.overridable(WebRequestError.ERROR_BAD_HSTS_CERT, true, url))
        assertFalse(CertErrors.overridable(WebRequestError.ERROR_SECURITY_SSL, true, url))
        assertFalse(CertErrors.overridable(WebRequestError.ERROR_UNKNOWN_HOST, true, url))
        assertFalse(CertErrors.overridable(WebRequestError.ERROR_SECURITY_BAD_CERT, true, "http://x.example/"))
        assertTrue(CertErrors.isCertificateError(WebRequestError.ERROR_BAD_HSTS_CERT))
        assertFalse(CertErrors.isCertificateError(WebRequestError.ERROR_SECURITY_SSL))
    }

    @Test
    fun problemFromTheCertificate() {
        val names = listOf("*.example.com", "example.com")
        fun problem(host: String, now: Long, names: List<String>, self: Boolean) =
            CertErrors.problem(host, now, notBeforeMs = 100, notAfterMs = 200, names = names, selfIssued = self)
        assertEquals(CertProblem.EXPIRED, problem("www.example.com", 300, names, false))
        assertEquals(CertProblem.NOT_YET_VALID, problem("www.example.com", 50, names, false))
        assertEquals(CertProblem.WRONG_HOST, problem("other.test", 150, names, true))
        assertEquals(CertProblem.SELF_SIGNED, problem("www.example.com", 150, names, true))
        assertEquals(CertProblem.UNTRUSTED, problem("example.com", 150, names, false))
        assertEquals(CertProblem.UNTRUSTED, problem("example.com", 150, emptyList(), false))
    }

    @Test
    fun wildcardsCoverOneLabel() {
        assertTrue(CertErrors.hostMatches("www.example.com", "*.example.com"))
        assertTrue(CertErrors.hostMatches("WWW.Example.com.", "*.example.com"))
        assertFalse(CertErrors.hostMatches("example.com", "*.example.com"))
        assertFalse(CertErrors.hostMatches("a.b.example.com", "*.example.com"))
        assertFalse(CertErrors.hostMatches("example.com", "*.com"))
        assertTrue(CertErrors.hostMatches("10.0.2.2", "10.0.2.2"))
        assertFalse(CertErrors.hostMatches("10.0.2.3", "10.0.2.2"))
    }

    @Test
    fun theListIsSortedWithTemporaryOnesMarked() {
        val a = CertErrors.sha256("a".toByteArray())
        val b = CertErrors.sha256("b".toByteArray())
        val temporary = setOf(CertErrors.key("b.example", 8443, b.lowercase()))
        val rows = CertErrors.exceptions(
            listOf(
                Triple("B.example", 8443, b),
                Triple("a.example", 443, a),
                Triple("a.example", 443, a), // listed twice: shown once
                Triple("bad.example", 443, "not a fingerprint"),
                Triple("", 443, a),
                Triple("c.example", 0, a),
            ),
            temporary,
        )
        assertEquals(
            listOf(
                CertException("a.example", 443, a, temporary = false),
                CertException("b.example", 8443, b, temporary = true),
            ),
            rows,
        )
        assertEquals("b.example:8443", rows[1].hostPort)
    }

    @Test
    fun theErrorPageStartingKeepsTheCertificateError() {
        val page = "data:text/html;base64,AAAA"
        val cert = CertError(
            url = "https://self-signed.example/",
            host = "self-signed.example",
            port = 443,
            derBase64 = "AAAA",
            sha256 = CertErrors.sha256(byteArrayOf(1)),
            problem = CertProblem.SELF_SIGNED,
            overridable = true,
            private = false,
        )
        var s = PageState().requested(cert.url)
        s = s.copy(loadError = LoadError(cert.url, 2, WebRequestError.ERROR_SECURITY_BAD_CERT, page, cert))
        s = s.pageStarted(page).locationChanged(page)
        assertEquals(cert, s.loadError?.cert)
        assertEquals(cert.url, s.url)
        // Any other load ends it.
        assertNull(s.pageStarted("https://other.example/").loadError)
        assertNull(s.requested(cert.url).loadError)
    }

    /**
     * The error screen's data comes from the certificate the site presented. GeckoView 160 hands
     * no certificate to `onLoadError` (the probe in [CertProbe] reads it), so a load error alone
     * makes no screen, and a certificate does.
     */
    @Test
    fun theErrorScreenIsBuiltFromTheCertificate() {
        val cert = javaClass.getResourceAsStream("/cert/krunk-test.der")!!.use {
            java.security.cert.CertificateFactory.getInstance("X.509").generateCertificate(it) as X509Certificate
        }
        val inDates = cert.notBefore.time + 86_400_000L
        fun error(url: String, code: Int = WebRequestError.ERROR_SECURITY_BAD_CERT, now: Long = inDates) =
            CertErrors.fromCertificate(url, code, cert, private = false, nowMs = now)
        val e = error("https://krunk.test/path")!!
        assertEquals("krunk.test", e.host)
        assertEquals(443, e.port)
        assertEquals(
            "37:07:BD:9E:51:F4:3F:30:CD:94:84:05:90:96:9C:6F:6E:C9:91:B8:43:B4:05:0C:2C:9A:DC:45:7D:C0:4D:F5",
            e.sha256,
        )
        assertArrayEquals(cert.encoded, Base64.getDecoder().decode(e.derBase64))
        assertEquals(CertProblem.SELF_SIGNED, e.problem)
        assertTrue(e.overridable)
        assertEquals(8443, error("https://krunk.test:8443/")!!.port)
        // The address's host is not one the certificate is for.
        assertEquals(CertProblem.WRONG_HOST, error("https://other.test/")!!.problem)
        // Past its end date.
        assertEquals(CertProblem.EXPIRED, error("https://krunk.test/", now = cert.notAfter.time + 1_000)!!.problem)
        // A site that demands a valid certificate gets the screen but no "Continue anyway".
        assertFalse(error("https://krunk.test/", WebRequestError.ERROR_BAD_HSTS_CERT)!!.overridable)
        // Other errors and other addresses get no screen.
        assertNull(error("https://krunk.test/", WebRequestError.ERROR_UNKNOWN_HOST))
        assertNull(error("http://krunk.test/"))
        // No certificate from the engine, no screen.
        assertNull(
            CertErrors.fromLoadError(
                "https://krunk.test/",
                WebRequestError.ERROR_SECURITY_BAD_CERT,
                null,
                false,
                inDates,
            ),
        )
    }
}
