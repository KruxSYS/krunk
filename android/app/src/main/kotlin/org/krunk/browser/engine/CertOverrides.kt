package org.krunk.browser.engine

import android.util.Log
import kotlinx.coroutines.withTimeoutOrNull
import org.json.JSONArray
import org.json.JSONObject
import org.krunk.browser.KrunkApp

/**
 * Certificate exceptions ("Continue anyway" on Krunk's certificate error screen), kept by the
 * engine's own certificate override service through the `krunkCerts` experiment API of the
 * bundled site-data extension (assets/extensions/krunk-sitedata/certs-api.js), over the same
 * native-messaging port as [SiteDataExtension].
 *
 * An exception is for host, port and one exact certificate (its SHA-256): the engine checks the
 * fingerprint, so a changed certificate is refused again. Permanent ones are stored by the engine
 * in the profile; temporary ones live in the engine until the app's process ends, and so does
 * [temporary], the app's note of which they are (the engine's list does not say). Private tabs
 * only ever get temporary ones, and the list shows normal tabs' exceptions only. Main thread only.
 */
class CertOverrides(private val ext: SiteDataExtension) {
    private val temporary = HashSet<String>()

    /** Accepts [error]'s certificate for its host and port; false when the engine did not take it. */
    suspend fun add(error: CertError, remember: Boolean): Boolean {
        if (!error.overridable) return false
        val keep = remember && !error.private
        val answer = call(
            JSONObject()
                .put("t", "cert-add")
                .put("host", error.host)
                .put("port", error.port)
                .put("der", error.derBase64)
                .put("temporary", !keep)
                .put("private", error.private),
        ) ?: return false
        // The engine's fingerprint of what it accepted must be the one the user was shown.
        val accepted = CertErrors.normalizeFingerprint(answer)
        if (accepted != CertErrors.normalizeFingerprint(error.sha256)) {
            Log.w(KrunkApp.TAG, "certificate exception: the engine accepted another certificate")
            remove(error.host, error.port)
            return false
        }
        val key = CertErrors.key(error.host, error.port, error.sha256)
        if (keep || error.private) temporary.remove(key) else temporary.add(key)
        return true
    }

    /** Removes the exceptions of host:port (normal and private tabs); false when the engine did not answer. */
    suspend fun remove(host: String, port: Int): Boolean {
        val done = call(JSONObject().put("t", "cert-remove").put("host", host).put("port", port)) != null
        if (done) temporary.removeAll { it.startsWith("${host.lowercase()}|$port|") }
        return done
    }

    /** The exceptions of normal tabs, or null when the engine did not answer. */
    suspend fun list(): List<CertException>? {
        val text = call(JSONObject().put("t", "cert-list")) ?: return null
        val rows = runCatching { JSONArray(text) }.getOrNull() ?: return null
        val listed = ArrayList<Triple<String, Int, String>>()
        for (i in 0 until minOf(rows.length(), MAX_ROWS)) {
            val row = rows.optJSONObject(i) ?: continue
            val host = row.optString("host")
            if (host.length > MAX_HOST) continue
            listed += Triple(host, row.optInt("port"), row.optString("sha256"))
        }
        return CertErrors.exceptions(listed, temporary)
    }

    private suspend fun call(message: JSONObject): String? {
        val answer = withTimeoutOrNull(TIMEOUT_MS) { ext.request(message) } ?: return null
        if (answer.optString("t") != "certs" || !answer.optBoolean("ok")) {
            Log.w(KrunkApp.TAG, "certificate exceptions: ${answer.optString("error").take(300)}")
            return null
        }
        return answer.optString("result")
    }

    private companion object {
        const val TIMEOUT_MS = 15_000L
        const val MAX_ROWS = 5000
        const val MAX_HOST = 300
    }
}
