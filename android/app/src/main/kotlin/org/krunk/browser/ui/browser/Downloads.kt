package org.krunk.browser.ui.browser

import android.app.DownloadManager
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.PersistableBundle
import android.util.Log
import android.webkit.MimeTypeMap
import org.krunk.browser.KrunkApp
import org.mozilla.geckoview.WebResponse

/**
 * A download a page started (a response the engine cannot show), waiting for the user's answer.
 * [fileName] and [host] are what the question shows; [private] says it comes from a private tab.
 */
class PendingDownload(
    val response: WebResponse,
    val fileName: String,
    val contentType: String?,
    val host: String,
    val private: Boolean,
)

/**
 * Hands downloads to Android's [DownloadManager] (its notification, its Downloads folder,
 * resumable). Only http(s) URLs qualify; the manager fetches them itself, so a response the
 * engine already opened is closed here and re-requested by the system (without the page's
 * cookies: a file behind a sign-in is a known limit until Krunk has its own downloads).
 *
 * A download the user asked for (the link menu) starts at once with [enqueueUrl]. One a page
 * started is first described with [describe] and only started with [start] after the user
 * agreed; [discard] drops it.
 */
object Downloads {
    /** Starts a download of [url]; returns the file name shown to the user, or null when it cannot be started. */
    fun enqueueUrl(context: Context, url: String, fileName: String? = null, mimeType: String? = null): String? {
        if (!downloadable(url)) return null
        val name = fileName ?: downloadFileName(url, null)
        // Building the request can fail too (no external storage): nothing here may throw.
        return try {
            val request = DownloadManager.Request(Uri.parse(url))
                .setTitle(name)
                .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED)
                .setAllowedOverMetered(true)
                .setAllowedOverRoaming(true)
            mimeType?.takeIf { it.isNotBlank() }?.let { request.setMimeType(it) }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                request.setDestinationInExternalPublicDir(Environment.DIRECTORY_DOWNLOADS, name)
            } else {
                // Before Android 10 the public Downloads folder needs WRITE_EXTERNAL_STORAGE, which
                // Krunk does not ask for: the app's own external folder is used instead.
                request.setDestinationInExternalFilesDir(context, Environment.DIRECTORY_DOWNLOADS, name)
            }
            val dm = context.getSystemService(Context.DOWNLOAD_SERVICE) as? DownloadManager ?: return null
            dm.enqueue(request)
            name
        } catch (e: Exception) {
            Log.w(KrunkApp.TAG, "download failed to start: ${e.message}")
            null
        }
    }

    /** What the engine asked us to handle, as the question to the user shows it. */
    fun describe(response: WebResponse, private: Boolean): PendingDownload {
        val disposition = response.headers.entries.firstOrNull { it.key.equals("Content-Disposition", true) }?.value
        val contentType = response.headers.entries.firstOrNull { it.key.equals("Content-Type", true) }?.value
            ?.substringBefore(';')?.trim()
        var name = downloadFileName(response.uri, disposition)
        if (!name.contains('.') && !contentType.isNullOrBlank()) {
            MimeTypeMap.getSingleton().getExtensionFromMimeType(contentType)?.let { name = "$name.$it" }
        }
        return PendingDownload(response, name, contentType, pillText(response.uri), private)
    }

    /** The user agreed: the system fetches the file (the stream the engine opened is not needed). */
    fun start(context: Context, download: PendingDownload): String? {
        discard(download)
        return enqueueUrl(context, download.response.uri, download.fileName, download.contentType)
    }

    /** Not downloaded: only the engine's open stream has to be closed. */
    fun discard(download: PendingDownload) {
        runCatching { download.response.body?.close() }
    }
}

/** Small intent helpers used by the menus. */
object Share {
    fun text(context: Context, text: String, subject: String? = null, chooserTitle: CharSequence? = null) {
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_TEXT, text)
            if (!subject.isNullOrBlank()) putExtra(Intent.EXTRA_SUBJECT, subject)
        }
        val chooser = Intent.createChooser(send, chooserTitle)
        runCatching { context.startActivity(chooser) }
            .onFailure { Log.w(KrunkApp.TAG, "share failed: ${it.message}") }
    }

    /**
     * Copies [text]; returns whether the caller should stay quiet (the system shows its own
     * "Copied", or nothing was copied). [sensitive] (a private tab) asks the system not to show
     * the text in its clipboard preview. Text too long for the clipboard is not copied (the
     * system would throw) instead of ending the app.
     */
    fun copy(context: Context, label: String, text: String, sensitive: Boolean = false): Boolean {
        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return true
        if (text.length > MAX_CLIP) {
            Log.w(KrunkApp.TAG, "not copied: ${text.length} characters are too many for the clipboard")
            return true
        }
        val clip = ClipData.newPlainText(label, text)
        if (sensitive) {
            clip.description.extras = PersistableBundle().apply { putBoolean(EXTRA_IS_SENSITIVE, true) }
        }
        return try {
            cm.setPrimaryClip(clip)
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
        } catch (e: RuntimeException) {
            Log.w(KrunkApp.TAG, "copy failed: ${e.message}")
            true
        }
    }

    /** Well below the binder transaction limit (1 MB, two bytes per character). */
    private const val MAX_CLIP = 100_000

    /** `ClipDescription.EXTRA_IS_SENSITIVE` (API 33); the key is honoured by older systems too. */
    private const val EXTRA_IS_SENSITIVE = "android.content.extra.IS_SENSITIVE"
}
