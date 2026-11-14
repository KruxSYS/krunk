package org.krunk.browser.update

import org.krunk.ffi.AndroidUpdate
import org.krunk.ffi.UpdateFailure
import org.krunk.ffi.UpdateFile

/**
 * The update check's states and rules that need no Android classes (tested on the JVM). The
 * network and the installer are in [UpdateController]; the signature and manifest rules in the
 * shared core (`core::update_android`, through `org.krunk.ffi`).
 */
sealed class UpdateState {
    /** Nothing asked yet in this process. */
    data object Idle : UpdateState()

    data object Checking : UpdateState()

    data class UpToDate(val version: String) : UpdateState()

    /** A newer build needs a later Android than this device has. */
    data class NeedsNewerAndroid(val update: AndroidUpdate) : UpdateState()

    /** A newer build for this device: the user may download it. */
    data class Available(val update: AndroidUpdate, val file: UpdateFile) : UpdateState()

    data class Downloading(val update: AndroidUpdate, val file: UpdateFile, val bytes: Long) : UpdateState()

    /** Downloaded and checked; waiting for "install unknown apps" for Krunk. */
    data class NeedsPermission(val update: AndroidUpdate) : UpdateState()

    /** Handed to the system installer; it asks the user. */
    data class Installing(val update: AndroidUpdate) : UpdateState()

    data class Failed(val error: UpdateErrorKind, val detail: String = "") : UpdateState()
}

/** Each failure the user can be told about, with its own message. None of them installs. */
enum class UpdateErrorKind {
    /** The update server could not be reached or answered with an error. */
    FETCH,

    /** The manifest's signature is not Krunk's release key (or not a signature). */
    SIGNATURE,

    /** Signed, but not usable for this app (wrong channel or package, a file elsewhere, ...). */
    INVALID,

    /** No APK in the release fits this device. */
    NO_FILE,

    /** The APK download broke off. */
    DOWNLOAD,

    /** The APK's size or SHA-256 differs from the signed manifest. */
    HASH,

    /** The APK is not Krunk, or not the build the manifest names. */
    PACKAGE,

    /** The APK is signed with another certificate than the installed Krunk. */
    CERTIFICATE,

    /** The system installer refused or failed. */
    INSTALLER,

    /** The user cancelled the installation in the system dialog. */
    ABORTED,
}

object UpdateRules {
    /** The ffi's refusal as one of the user-facing kinds. */
    fun kindOf(f: UpdateFailure): UpdateErrorKind = when (f) {
        is UpdateFailure.BadSignature -> UpdateErrorKind.SIGNATURE

        is UpdateFailure.WrongHash, is UpdateFailure.WrongSize -> UpdateErrorKind.HASH

        is UpdateFailure.BadAddress,
        is UpdateFailure.Malformed,
        is UpdateFailure.NotOnServer,
        is UpdateFailure.TooLarge,
        -> UpdateErrorKind.INVALID
    }

    /** Lower-case hex. */
    fun hex(bytes: ByteArray): String = bytes.joinToString("") { "%02x".format(it) }

    /**
     * Does the downloaded APK, as the package manager reads it, belong here? It must be [pkg],
     * carry exactly the manifest's versionCode, and be signed by the same certificate(s) as the
     * installed app, which must also be the manifest's certificate. Returns the failure, or null.
     * An APK whose certificates cannot be read is refused.
     */
    fun checkApk(
        pkg: String,
        manifestVersionCode: Long,
        manifestCertSha256: String,
        installedCerts: Set<String>,
        apkPackage: String?,
        apkVersionCode: Long?,
        apkCerts: Set<String>,
    ): UpdateErrorKind? = when {
        apkPackage != pkg -> UpdateErrorKind.PACKAGE
        apkVersionCode != manifestVersionCode -> UpdateErrorKind.PACKAGE
        apkCerts.isEmpty() || installedCerts.isEmpty() -> UpdateErrorKind.CERTIFICATE
        apkCerts != installedCerts -> UpdateErrorKind.CERTIFICATE
        apkCerts != setOf(manifestCertSha256.lowercase()) -> UpdateErrorKind.CERTIFICATE
        else -> null
    }

    /**
     * The release notes (Markdown from CHANGELOG-android.md) as plain text for the offer: list
     * items become bullets, a wrapped item's lines are joined, `**bold**`, `` `code` `` and
     * `[text](link)` keep only their text. Paragraph breaks stay.
     */
    fun plainNotes(markdown: String): String {
        val inline = { line: String ->
            line.replace(Regex("""\*\*(.+?)\*\*"""), "$1")
                .replace(Regex("""`([^`]*)`"""), "$1")
                .replace(Regex("""\[([^\]]*)]\([^)]*\)"""), "$1")
        }
        val out = ArrayList<String>()
        for (raw in markdown.replace("\r\n", "\n").split("\n")) {
            val line = raw.trimEnd()
            val item = Regex("""^(\s*)[-*] (.*)$""").find(line)
            when {
                line.isBlank() -> if (out.isNotEmpty() && out.last().isNotEmpty()) out += ""

                item != null -> out += "  ".repeat(item.groupValues[1].length / 2) + "• " + inline(item.groupValues[2])

                out.isNotEmpty() && out.last().isNotEmpty() -> out[out.size - 1] =
                    out.last() + " " + inline(line.trim())

                else -> out += inline(line.trim())
            }
        }
        return out.joinToString("\n").trim()
    }

    /** The User-Agent of Krunk's own update requests: nothing but the app and its version. */
    fun userAgent(versionName: String) = "Krunk-Android/$versionName"
}
