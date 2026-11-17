package org.krunk.browser.ui.settings

/**
 * Whether text another app shares (`SEND text/plain`) is a VPN profile rather than something
 * for the address bar (plan §7). A cheap look at the lines only; the core's import has the last
 * word (`VpnController.importFile`). Plain Kotlin, tested on the JVM.
 */
object VpnImportSniff {
    /** Larger texts are never profiles (the same limit as a profile file). */
    const val MAX_BYTES = 64 * 1024

    enum class Kind { WIREGUARD, OPENVPN }

    /** The kind the text looks like, or null when it does not look like a profile. */
    fun kind(text: String): Kind? {
        if (text.length > MAX_BYTES || text.toByteArray(Charsets.UTF_8).size > MAX_BYTES) return null
        var iface = false
        var privateKey = false
        var client = false
        var remote = false
        var dev = false
        for (raw in text.lineSequence()) {
            val line = raw.trim().removePrefix("\uFEFF")
            if (line.isEmpty() || line.startsWith("#") || line.startsWith(";")) continue
            val lower = line.lowercase()
            when {
                lower == "[interface]" -> iface = true
                isPrivateKey(lower) -> privateKey = true
                lower == "client" || lower == "tls-client" -> client = true
                lower.startsWith("remote ") || lower.startsWith("remote\t") -> remote = true
                lower.startsWith("dev tun") || lower.startsWith("dev tap") -> dev = true
            }
        }
        return when {
            iface && privateKey -> Kind.WIREGUARD
            remote && (client || dev) -> Kind.OPENVPN
            else -> null
        }
    }

    /** `PrivateKey = ...` (any case, any spacing). */
    private fun isPrivateKey(lower: String): Boolean =
        lower.startsWith("privatekey") && lower.substring("privatekey".length).trimStart().startsWith("=")

    fun looksLikeProfile(text: String): Boolean = kind(text) != null

    /** The file name a shared text is imported under (the core reads the kind from the content). */
    fun fileName(kind: Kind): String = when (kind) {
        Kind.WIREGUARD -> "Shared profile.conf"
        Kind.OPENVPN -> "Shared profile.ovpn"
    }
}
