package org.krunk.browser.vpn

import java.io.File
import java.io.IOException

/** Which tabs the VPN carries (Windows 0.22.0): every tab and Krunk's own requests, or private tabs only. */
enum class VpnScope { ALL, PRIVATE }

/**
 * The VPN's switches (contract §5). Stored as plain JSON in `files/krunk-vpn/settings.json`: no
 * secrets in here, never synced, excluded from backup and device transfer.
 */
data class VpnSettings(
    /** The VPN is on: Krunk's traffic in [scope] goes through it or nowhere (fail-closed). */
    val enabled: Boolean = false,
    val connectAtStart: Boolean = false,
    val scope: VpnScope = VpnScope.ALL,
    /** "Reach the local network directly" (off by default, as Windows 0.14.2). */
    val lanDirect: Boolean = false,
    val showInAddressBar: Boolean = true,
    val selectedProfileId: String? = null,
) {
    fun toJson(): String = buildString {
        append('{')
        append("\"enabled\":").append(enabled).append(',')
        append("\"connectAtStart\":").append(connectAtStart).append(',')
        append("\"scope\":\"").append(scope.name.lowercase()).append("\",")
        append("\"lanDirect\":").append(lanDirect).append(',')
        append("\"showInAddressBar\":").append(showInAddressBar).append(',')
        append("\"selectedProfileId\":")
        // Ids are made by VpnStore (hex); anything else is not written.
        val id = selectedProfileId?.takeIf(VpnStore::validId)
        if (id == null) append("null") else append('"').append(id).append('"')
        append('}')
    }

    companion object {
        /**
         * Reads what [toJson] wrote. Flat objects only (`"key": true|false|null|"string"`);
         * unknown keys are skipped and a broken file gives the defaults.
         */
        fun fromJson(text: String): VpnSettings {
            val values = parseFlat(text) ?: return VpnSettings()
            val d = VpnSettings()
            fun bool(key: String, default: Boolean) = (values[key] as? Boolean) ?: default
            return VpnSettings(
                enabled = bool("enabled", d.enabled),
                connectAtStart = bool("connectAtStart", d.connectAtStart),
                scope = if ((values["scope"] as? String) == "private") VpnScope.PRIVATE else VpnScope.ALL,
                lanDirect = bool("lanDirect", d.lanDirect),
                showInAddressBar = bool("showInAddressBar", d.showInAddressBar),
                selectedProfileId = (values["selectedProfileId"] as? String)?.takeIf(VpnStore::validId),
            )
        }

        /** A flat JSON object; values are Boolean, String or null. Null when it is not one. */
        internal fun parseFlat(text: String): Map<String, Any?>? {
            val out = HashMap<String, Any?>()
            var i = 0
            fun ws() {
                while (i < text.length && text[i].isWhitespace()) i++
            }
            fun str(): String? {
                if (i >= text.length || text[i] != '"') return null
                i++
                val sb = StringBuilder()
                while (i < text.length) {
                    val c = text[i++]
                    when (c) {
                        '"' -> return sb.toString()
                        '\\' -> {
                            if (i >= text.length) return null
                            val e = text[i++]
                            when (e) {
                                '"', '\\', '/' -> sb.append(e)
                                'n' -> sb.append('\n')
                                't' -> sb.append('\t')
                                else -> return null
                            }
                        }
                        else -> sb.append(c)
                    }
                }
                return null
            }
            ws()
            if (i >= text.length || text[i] != '{') return null
            i++
            ws()
            if (i < text.length && text[i] == '}') return out
            while (true) {
                ws()
                val key = str() ?: return null
                ws()
                if (i >= text.length || text[i] != ':') return null
                i++
                ws()
                val value: Any? = when {
                    text.startsWith("true", i) -> true.also { i += 4 }
                    text.startsWith("false", i) -> false.also { i += 5 }
                    text.startsWith("null", i) -> null.also { i += 4 }
                    i < text.length && text[i] == '"' -> str() ?: return null
                    else -> return null
                }
                out[key] = value
                ws()
                if (i >= text.length) return null
                when (text[i++]) {
                    ',' -> continue
                    '}' -> return out
                    else -> return null
                }
            }
        }
    }
}

/** [VpnSettings] in `<dir>/settings.json`, written atomically (temp file, then rename). */
class VpnSettingsFile(private val dir: File) {
    private val file get() = File(dir, VpnStore.SETTINGS_FILE)

    fun read(): VpnSettings = try {
        if (file.isFile) VpnSettings.fromJson(file.readText()) else VpnSettings()
    } catch (_: IOException) {
        VpnSettings()
    }

    @Throws(IOException::class)
    fun write(settings: VpnSettings) {
        dir.mkdirs()
        val tmp = File(dir, VpnStore.SETTINGS_FILE + ".tmp")
        tmp.writeText(settings.toJson())
        if (!tmp.renameTo(file)) {
            tmp.delete()
            throw IOException("settings not saved")
        }
    }
}
