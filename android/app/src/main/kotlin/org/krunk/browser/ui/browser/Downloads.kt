package org.krunk.browser.ui.browser

import android.app.DownloadManager
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.util.Log
import android.webkit.MimeTypeMap
import org.krunk.browser.KrunkApp
import org.mozilla.geckoview.WebResponse

/**
 * Hands user-requested downloads to Android's [DownloadManager] (its notification, its
 * Downloads folder, resumable). Only http(s) URLs qualify; the manager fetches them itself,
 * so a response the engine already opened is closed here and re-requested by the system.
 */
object Downloads {
    /** Starts a download of [url]; returns the file name shown to the user, or null when the URL cannot be fetched. */
    fun enqueueUrl(context: Context, url: String, fileName: String? = null, mimeType: String? = null): String? {
        if (!downloadable(url)) return null
        val name = fileName ?: downloadFileName(url, null)
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
        return try {
            val dm = context.getSystemService(Context.DOWNLOAD_SERVICE) as? DownloadManager ?: return null
            dm.enqueue(request)
            name
        } catch (e: Exception) {
            Log.w(KrunkApp.TAG, "download failed to start: ${e.message}")
            null
        }
    }

    /** The engine asked us to handle a response (a file the page cannot show). */
    fun enqueue(context: Context, response: WebResponse): String? {
        // The system fetches the file again; the stream the engine opened is not needed.
        runCatching { response.body?.close() }
        val disposition = response.headers.entries.firstOrNull { it.key.equals("Content-Disposition", true) }?.value
        val contentType = response.headers.entries.firstOrNull { it.key.equals("Content-Type", true) }?.value
            ?.substringBefore(';')?.trim()
        var name = downloadFileName(response.uri, disposition)
        if (!name.contains('.') && !contentType.isNullOrBlank()) {
            MimeTypeMap.getSingleton().getExtensionFromMimeType(contentType)?.let { name = "$name.$it" }
        }
        return enqueueUrl(context, response.uri, name, contentType)
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

    /** Copies [text]; returns whether the system already shows its own "Copied" confirmation. */
    fun copy(context: Context, label: String, text: String): Boolean {
        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return true
        cm.setPrimaryClip(ClipData.newPlainText(label, text))
        return Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
    }
}
