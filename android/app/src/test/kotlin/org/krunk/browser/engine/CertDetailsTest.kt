package org.krunk.browser.engine

import java.security.MessageDigest
import java.security.cert.CertificateFactory
import java.security.cert.X509Certificate
import java.time.Instant
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The certificates come from src/test/resources/cert/make-test-certs.py. */
class CertDetailsTest {
    private fun bytes(name: String): ByteArray =
        checkNotNull(javaClass.classLoader?.getResourceAsStream("cert/$name")) { name }.use { it.readBytes() }

    private fun cert(name: String): X509Certificate =
        CertificateFactory.getInstance("X.509").generateCertificate(bytes(name).inputStream()) as X509Certificate

    private fun details(name: String): CertDetails = checkNotNull(CertDetails.of(cert(name)))

    private fun ms(iso: String) = Instant.parse(iso).toEpochMilli()

    @Test
    fun namesOfANormalSite() {
        val d = details("site.der")
        assertEquals("www.example.test", d.subjectCn)
        assertEquals("Example Org", d.subjectO)
        assertEquals("Krunk Test CA R1", d.issuerCn)
        assertEquals("Krunk Test Authority", d.issuerO)
        assertEquals("www.example.test", d.issuedTo)
        assertEquals("Krunk Test Authority", d.issuedBy)
    }

    @Test
    fun alternativeNamesShowInternationalNamesInUnicode() {
        val d = details("site.der")
        assertEquals(
            listOf(
                "www.example.test",
                "example.test",
                "bücher.example",
                "*.cdn.example.test",
                "a.example.test",
                "b.example.test",
                "192.0.2.7",
            ),
            d.altNames,
        )
        assertEquals("bücher.example", CertDetails.displayDns("xn--bcher-kva.example"))
        assertEquals("*.bücher.example", CertDetails.displayDns("*.xn--bcher-kva.example"))
    }

    @Test
    fun fingerprintsAreOfTheDer() {
        val der = bytes("site.der")
        val d = details("site.der")
        assertEquals(CertErrors.sha256(der), d.sha256)
        val sha1 = MessageDigest.getInstance("SHA-1").digest(der).joinToString(":") { "%02X".format(it) }
        assertEquals(sha1, d.sha1)
        assertEquals(32 * 3 - 1, d.sha256.length)
    }

    @Test
    fun technicalDetails() {
        val d = details("site.der")
        assertEquals("F0:0D", d.serial) // 0x00F00D: the sign byte is not shown
        assertEquals("RSA", d.keyAlgorithm)
        assertEquals(2048, d.keyBits)
        assertTrue(d.signatureAlgorithm, d.signatureAlgorithm.contains("ECDSA", ignoreCase = true))
        val ec = details("no-cn.der")
        assertEquals("EC", ec.keyAlgorithm)
        assertEquals(256, ec.keyBits)
    }

    @Test
    fun missingCnFallsBackToTheFirstAlternativeName() {
        val d = details("no-cn.der")
        assertNull(d.subjectCn)
        assertNull(d.subjectO)
        assertEquals("only-san.example", d.issuedTo)
    }

    @Test
    fun missingOrganisationFallsBackToTheCn() {
        val d = details("self.der")
        assertEquals("Router, Inc+Co", d.subjectCn)
        assertNull(d.issuerO)
        assertEquals("Router, Inc+Co", d.issuedBy)
        assertEquals(emptyList<String>(), d.altNames)
    }

    @Test
    fun validityPeriods() {
        val now = ms("2026-11-17T00:00:00Z")
        assertEquals(CertValidity.VALID, details("site.der").validity(now))
        val expired = details("expired.der")
        assertEquals(CertValidity.EXPIRED, expired.validity(now))
        assertEquals(ms("2021-01-01T00:00:00Z"), expired.notAfterMs)
        val future = details("future.der")
        assertEquals(CertValidity.NOT_YET_VALID, future.validity(now))
        assertEquals(ms("2099-01-01T00:00:00Z"), future.notBeforeMs)
    }

    @Test
    fun rfc2253Parsing() {
        assertEquals(
            listOf("CN" to "a,b", "O" to "Ex \"Q\"", "C" to "US"),
            CertDetails.parseRfc2253("""CN=a\,b,O=Ex \"Q\",C=US"""),
        )
        // Multi-valued RDN, hex escapes in UTF-8, quoted value, a BER value skipped.
        assertEquals(
            listOf("CN" to "x", "OU" to "y", "O" to "Bücher", "L" to "a,b"),
            CertDetails.parseRfc2253("""CN=x+OU=y,O=B\C3\BCcher,L="a,b",1.2.3.4=#0403616263"""),
        )
        assertEquals(emptyList<Pair<String, String>>(), CertDetails.parseRfc2253(""))
    }

    @Test
    fun toStringCarriesNoFingerprint() {
        val d = details("site.der")
        assertTrue(d.sha256.take(11) !in d.toString())
        val s = PageSecurity(secure = true, certificate = cert("site.der")).toString()
        assertTrue(s, "BEGIN" !in s && d.sha256.take(11) !in s && s.contains("cert=true"))
    }
}
