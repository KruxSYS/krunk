package org.krunk.browser.engine

import java.math.BigInteger
import java.net.IDN
import java.security.MessageDigest
import java.security.cert.X509Certificate
import java.security.interfaces.DSAPublicKey
import java.security.interfaces.ECPublicKey
import java.security.interfaces.RSAPublicKey
import javax.security.auth.x500.X500Principal

/** Whether a certificate is inside its validity period at a given time. */
enum class CertValidity { VALID, EXPIRED, NOT_YET_VALID }

/**
 * What the shield sheet shows of a site's certificate: the plain fields, read once from the
 * certificate (JVM-tested). Nothing here is stored or logged.
 */
data class CertDetails(
    /** Subject common name; null when the certificate has none. */
    val subjectCn: String?,
    /** Subject organisation; null when the certificate has none (domain-validated ones usually). */
    val subjectO: String?,
    val issuerCn: String?,
    val issuerO: String?,
    val notBeforeMs: Long,
    val notAfterMs: Long,
    /** SHA-256 of the DER, `AA:BB:...` (the engine's spelling). */
    val sha256: String,
    val sha1: String,
    /** DNS names (international ones in Unicode) and IP addresses the certificate is for. */
    val altNames: List<String>,
    /** Serial number, hex pairs joined by colons. */
    val serial: String,
    /** "SHA256withRSA", "SHA256withECDSA", ... */
    val signatureAlgorithm: String,
    /** "RSA", "EC", "Ed25519", ... */
    val keyAlgorithm: String,
    /** Key size in bits; 0 when unknown. */
    val keyBits: Int,
) {
    /** The name to show for "Issued to": the CN, else the first alternative name. */
    val issuedTo: String? get() = subjectCn ?: altNames.firstOrNull()

    /** The name to show for "Issued by": the authority's organisation, else its CN. */
    val issuedBy: String? get() = issuerO ?: issuerCn

    fun validity(nowMs: Long): CertValidity = when {
        nowMs < notBeforeMs -> CertValidity.NOT_YET_VALID
        nowMs > notAfterMs -> CertValidity.EXPIRED
        else -> CertValidity.VALID
    }

    // Only for debugging output that must not carry fingerprints (rule 9 of the security review).
    override fun toString(): String = "CertDetails(subject=$subjectCn, issuer=$issuedBy)"

    companion object {
        /** The details of [cert], or null when it cannot be read (no encoding). */
        fun of(cert: X509Certificate): CertDetails? {
            val der = runCatching { cert.encoded }.getOrNull() ?: return null
            val subject = rdns(cert.subjectX500Principal)
            val issuer = rdns(cert.issuerX500Principal)
            return CertDetails(
                subjectCn = subject.first("CN"),
                subjectO = subject.first("O"),
                issuerCn = issuer.first("CN"),
                issuerO = issuer.first("O"),
                notBeforeMs = cert.notBefore.time,
                notAfterMs = cert.notAfter.time,
                sha256 = CertErrors.sha256(der),
                sha1 = MessageDigest.getInstance("SHA-1").digest(der).hexPairs(),
                altNames = altNames(cert),
                serial = serialHex(cert.serialNumber),
                signatureAlgorithm = cert.sigAlgName.orEmpty(),
                keyAlgorithm = cert.publicKey?.algorithm.orEmpty(),
                keyBits = keyBits(cert),
            )
        }

        private fun List<Pair<String, String>>.first(type: String): String? =
            firstOrNull { it.first.equals(type, ignoreCase = true) }?.second?.trim()?.takeIf { it.isNotEmpty() }

        private fun rdns(principal: X500Principal): List<Pair<String, String>> =
            runCatching { parseRfc2253(principal.getName(X500Principal.RFC2253)) }.getOrDefault(emptyList())

        /**
         * The attribute type/value pairs of an RFC 2253 name, in the order written (the most
         * specific first). Multi-valued RDNs ("a=1+b=2") give one pair each. Escapes (`\,`,
         * `\XX` hex bytes in UTF-8) are undone; values written as `#hex` (BER) are skipped.
         */
        fun parseRfc2253(name: String): List<Pair<String, String>> {
            val out = ArrayList<Pair<String, String>>()
            var i = 0
            val n = name.length
            while (i < n) {
                // type
                val eq = name.indexOf('=', i)
                if (eq < 0) break
                val type = name.substring(i, eq).trim()
                i = eq + 1
                val value = java.io.ByteArrayOutputStream()
                var hexValue = i < n && name[i] == '#'
                var quoted = false
                while (i < n) {
                    val c = name[i]
                    if (quoted) {
                        if (c == '"') {
                            quoted = false
                            i++
                            continue
                        }
                    } else if (c == '"') {
                        quoted = true
                        i++
                        continue
                    } else if (c == ',' || c == '+' || c == ';') {
                        break
                    }
                    if (c == '\\' && i + 1 < n) {
                        val hex = if (i + 2 < n) name.substring(i + 1, i + 3) else ""
                        if (hex.length == 2 && hex.all { it.isHexDigit() }) {
                            value.write(hex.toInt(HEX))
                            i += 3
                        } else {
                            value.write(name[i + 1].toString().toByteArray(Charsets.UTF_8))
                            i += 2
                        }
                        continue
                    }
                    value.write(c.toString().toByteArray(Charsets.UTF_8))
                    i++
                }
                if (!hexValue) out += type to value.toString(Charsets.UTF_8.name())
                hexValue = false
                i++ // the separator
            }
            return out
        }

        private fun Char.isHexDigit() = this in '0'..'9' || this in 'a'..'f' || this in 'A'..'F'

        private fun altNames(cert: X509Certificate): List<String> = runCatching {
            cert.subjectAlternativeNames.orEmpty().mapNotNull { entry ->
                val value = entry.getOrNull(1) as? String ?: return@mapNotNull null
                when (entry.getOrNull(0) as? Int) {
                    SAN_DNS -> displayDns(value)
                    SAN_IP -> value
                    else -> null
                }
            }
        }.getOrDefault(emptyList())

        /** A DNS name as people read it: punycode labels in Unicode, a wildcard kept. */
        fun displayDns(name: String): String =
            runCatching { IDN.toUnicode(name, IDN.ALLOW_UNASSIGNED) }.getOrDefault(name)

        private fun serialHex(serial: BigInteger): String {
            val bytes = serial.toByteArray()
            // A positive number with its top bit set gets a leading zero byte in two's complement.
            val trimmed = if (bytes.size > 1 && bytes[0] == 0.toByte()) bytes.copyOfRange(1, bytes.size) else bytes
            return trimmed.hexPairs()
        }

        private fun keyBits(cert: X509Certificate): Int = when (val key = cert.publicKey) {
            is RSAPublicKey -> key.modulus.bitLength()

            is ECPublicKey -> key.params.order.bitLength()

            is DSAPublicKey -> key.params.p.bitLength()

            else -> when (key?.algorithm) {
                "Ed25519", "X25519" -> ED25519_BITS
                "Ed448", "X448" -> ED448_BITS
                else -> 0
            }
        }

        private fun ByteArray.hexPairs(): String = joinToString(":") { "%02X".format(it.toInt() and BYTE) }

        private const val SAN_DNS = 2
        private const val SAN_IP = 7
        private const val HEX = 16
        private const val BYTE = 0xff
        private const val ED25519_BITS = 256
        private const val ED448_BITS = 456
    }
}
