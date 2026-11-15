package org.krunk.browser.engine

import java.net.IDN
import java.security.MessageDigest
import java.security.cert.X509Certificate
import java.util.Base64
import java.util.Locale
import org.mozilla.geckoview.WebRequestError

/** What is wrong with a certificate, as far as the app can tell from the certificate itself. */
enum class CertProblem {
    EXPIRED,
    NOT_YET_VALID,

    /** Issued for other names than the one in the address. */
    WRONG_HOST,

    /** Signed by itself: nobody vouches for it. */
    SELF_SIGNED,

    /** Issued by an authority the engine does not trust (or any other reason it did not say). */
    UNTRUSTED,
}

/**
 * A load that failed on the site's certificate, kept with the tab's [LoadError] while its error
 * screen is shown. This is what "Continue anyway" acts on: the button on screen only names the
 * tab, never the certificate or the address.
 */
data class CertError(
    val url: String,
    /** ASCII host name (IDN as punycode), IPv6 without brackets. */
    val host: String,
    val port: Int,
    /** The certificate (DER), base64 without line breaks. */
    val derBase64: String,
    /** SHA-256 of the DER, `AA:BB:...`. */
    val sha256: String,
    val problem: CertProblem,
    /** "Continue anyway" may be offered (see [CertErrors.overridable]). */
    val overridable: Boolean,
    val private: Boolean,
)

/** One accepted certificate, as Settings › Privacy › Certificate exceptions lists it. */
data class CertException(val host: String, val port: Int, val sha256: String, val temporary: Boolean) {
    val hostPort: String get() = CertErrors.hostPort(host, port)
}

/** The plain parts of certificate errors and exceptions (JVM-tested). */
object CertErrors {
    const val HTTPS_PORT = 443

    private val AUTHORITY = Regex("""^[hH][tT][tT][pP][sS]://([^/?#\\]*)""")

    /**
     * Host and port of an `https:` address, or null (not https, no host, odd port). The host is
     * lower case ASCII (an international name as punycode), an IPv6 address without brackets;
     * the port is 443 when the address names none.
     */
    fun hostAndPort(url: String): Pair<String, Int>? {
        val authority = AUTHORITY.find(url)?.groupValues?.get(1)?.substringAfterLast('@') ?: return null
        val host: String
        val portText: String?
        if (authority.startsWith("[")) {
            val end = authority.indexOf(']')
            if (end < 0) return null
            host = authority.substring(1, end)
            val rest = authority.substring(end + 1)
            portText = when {
                rest.isEmpty() -> null
                rest.startsWith(":") -> rest.substring(1)
                else -> return null
            }
            if (host.isEmpty() ||
                !host.all { it in '0'..'9' || it.lowercaseChar() in 'a'..'f' || it == ':' || it == '.' }
            ) {
                return null
            }
        } else {
            val colon = authority.lastIndexOf(':')
            val name = if (colon >= 0) authority.substring(0, colon) else authority
            portText = if (colon >= 0) authority.substring(colon + 1) else null
            host = asciiHost(name.trimEnd('.')) ?: return null
        }
        val port = when {
            portText == null || portText.isEmpty() -> HTTPS_PORT
            portText.length > 5 || !portText.all { it in '0'..'9' } -> return null
            else -> portText.toInt()
        }
        if (port !in 1..65535) return null
        return host.lowercase(Locale.ROOT) to port
    }

    private fun asciiHost(name: String): String? {
        if (name.isEmpty() || name.length > 253) return null
        val ascii = if (name.all { it.code < 128 }) {
            name
        } else {
            runCatching { IDN.toASCII(name) }.getOrNull()
                ?: return null
        }
        if (!ascii.all { (it.isLetterOrDigit() && it.code < 128) || it == '-' || it == '.' || it == '_' }) return null
        return ascii
    }

    /** `host:port`, with brackets around an IPv6 address. */
    fun hostPort(host: String, port: Int): String = if (':' in host) "[$host]:$port" else "$host:$port"

    /** SHA-256 of [der] as the engine writes fingerprints: upper-case hex pairs joined by colons. */
    fun sha256(der: ByteArray): String =
        MessageDigest.getInstance("SHA-256").digest(der).joinToString(":") { "%02X".format(it.toInt() and 0xff) }

    /** The same fingerprint whatever its spelling (case, colons, spaces), or null when it is not one. */
    fun normalizeFingerprint(text: String): String? {
        val hex = text.filter { it != ':' && it != ' ' }.uppercase(Locale.ROOT)
        if (hex.length != 64 || !hex.all { it in '0'..'9' || it in 'A'..'F' }) return null
        return hex.chunked(2).joinToString(":")
    }

    /** The first eight bytes of a fingerprint and an ellipsis, for lists. */
    fun shortFingerprint(fingerprint: String): String {
        val full = normalizeFingerprint(fingerprint) ?: return fingerprint.take(SHORT_CHARS)
        return full.take(SHORT_CHARS) + "…"
    }

    private const val SHORT_CHARS = 23 // "AA:BB:CC:DD:EE:FF:00:11"

    /**
     * Whether "Continue anyway" may be offered. Only for a certificate the engine refused for
     * itself ([WebRequestError.ERROR_SECURITY_BAD_CERT]: unknown issuer, self-signed, expired,
     * wrong name) on an `https:` address, with the certificate at hand. Never for a site that
     * demands a valid certificate (HSTS, [WebRequestError.ERROR_BAD_HSTS_CERT]), a broken TLS
     * connection ([WebRequestError.ERROR_SECURITY_SSL]: no certificate to accept) or anything else.
     */
    fun overridable(code: Int, hasCertificate: Boolean, url: String): Boolean =
        code == WebRequestError.ERROR_SECURITY_BAD_CERT && hasCertificate && hostAndPort(url) != null

    /** Whether an error with [code] gets Krunk's certificate screen (with or without Continue). */
    fun isCertificateError(code: Int): Boolean = code == WebRequestError.ERROR_SECURITY_BAD_CERT ||
        code == WebRequestError.ERROR_BAD_HSTS_CERT

    /**
     * The most likely problem, from the certificate alone: validity dates first, then the names
     * it is for, then who signed it. The engine does not say which check failed.
     */
    fun problem(
        host: String,
        nowMs: Long,
        notBeforeMs: Long,
        notAfterMs: Long,
        names: List<String>,
        selfIssued: Boolean,
    ): CertProblem = when {
        nowMs > notAfterMs -> CertProblem.EXPIRED
        nowMs < notBeforeMs -> CertProblem.NOT_YET_VALID
        names.isNotEmpty() && names.none { hostMatches(host, it) } -> CertProblem.WRONG_HOST
        selfIssued -> CertProblem.SELF_SIGNED
        else -> CertProblem.UNTRUSTED
    }

    /** Whether a certificate name (`www.example.com`, `*.example.com`, an IP address) covers [host]. */
    fun hostMatches(host: String, name: String): Boolean {
        val h = host.lowercase(Locale.ROOT).trimEnd('.')
        val n = name.lowercase(Locale.ROOT).trimEnd('.').removePrefix("[").removeSuffix("]")
        if (n.isEmpty() || h.isEmpty()) return false
        if (!n.startsWith("*.")) return h == n
        // A wildcard stands for exactly one label, and never for the whole registrable part.
        val suffix = n.substring(1)
        if (suffix.count { it == '.' } < 2) return false
        if (!h.endsWith(suffix)) return false
        val label = h.substring(0, h.length - suffix.length)
        return label.isNotEmpty() && '.' !in label
    }

    /** The exceptions the engine listed, with what the app knows of temporary ones, sorted for the list. */
    fun exceptions(listed: List<Triple<String, Int, String>>, temporary: Set<String>): List<CertException> = listed
        .mapNotNull { (host, port, fp) ->
            val sha = normalizeFingerprint(fp) ?: return@mapNotNull null
            if (host.isEmpty() || port !in 1..65535) return@mapNotNull null
            CertException(host.lowercase(Locale.ROOT), port, sha, key(host, port, sha) in temporary)
        }
        .distinctBy { it.host to it.port }
        .sortedWith(compareBy({ it.host }, { it.port }))

    /** The key under which the app remembers that an exception is temporary. */
    fun key(host: String, port: Int, sha256: String): String =
        "${host.lowercase(Locale.ROOT)}|$port|${normalizeFingerprint(sha256) ?: sha256}"

    /**
     * What the error screen needs from a failed load, or null when it is not a certificate error,
     * the address has no host, or the engine handed over no certificate. GeckoView 160 always
     * passes null to `onLoadError`; [CertProbe] reads the certificate instead and this is then
     * [fromCertificate].
     */
    fun fromLoadError(url: String, code: Int, cert: X509Certificate?, private: Boolean, nowMs: Long): CertError? =
        cert?.let { fromCertificate(url, code, it, private, nowMs) }

    /** What the error screen needs for [cert]: null when it is not a certificate error or the address has no host. */
    fun fromCertificate(url: String, code: Int, cert: X509Certificate, private: Boolean, nowMs: Long): CertError? {
        if (!isCertificateError(code)) return null
        val (host, port) = hostAndPort(url) ?: return null
        val der = runCatching { cert.encoded }.getOrNull() ?: return null
        val names = runCatching {
            cert.subjectAlternativeNames
                ?.filter { (it[0] as? Int) == SAN_DNS || (it[0] as? Int) == SAN_IP }
                ?.mapNotNull { it[1] as? String }
        }.getOrNull().orEmpty()
        val selfIssued = runCatching { cert.subjectX500Principal == cert.issuerX500Principal }.getOrDefault(false)
        return CertError(
            url = url,
            host = host,
            port = port,
            derBase64 = Base64.getEncoder().encodeToString(der),
            sha256 = sha256(der),
            problem = problem(host, nowMs, cert.notBefore.time, cert.notAfter.time, names, selfIssued),
            overridable = overridable(code, true, url),
            private = private,
        )
    }

    private const val SAN_DNS = 2
    private const val SAN_IP = 7
}
