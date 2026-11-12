package org.krunk.browser.engine

import android.content.Context
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.provider.OpenableColumns
import android.util.Log
import androidx.core.content.FileProvider
import androidx.core.net.toUri
import java.io.File
import java.io.IOException
import java.util.UUID
import kotlin.concurrent.thread

/**
 * The files a page is given through a file field. The engine reads plain files, and Krunk has
 * no storage permission (and wants none), so each picked document is copied into a folder of
 * the tab inside Krunk's cache and the engine gets the copy. A tab's copies go when its session
 * closes, and whatever is left goes at the next start.
 */
class Uploads(context: Context) {
    private val app = context.applicationContext
    private val root = File(app.cacheDir, UPLOADS)
    private val main = Handler(Looper.getMainLooper())

    /** At start: nothing of an earlier run is needed (no page of that run is still uploading). */
    fun clearAll() {
        thread(name = "krunk-uploads-clear") {
            root.deleteRecursively()
            File(app.cacheDir, CAPTURES).deleteRecursively()
        }
    }

    /** The tab's session closed: its uploads are over. */
    fun clearTab(tabId: UInt) {
        val dir = File(root, tabId.toString())
        if (dir.exists()) thread(name = "krunk-uploads-clear") { dir.deleteRecursively() }
    }

    /**
     * Copies the picked documents for [tabId] off the main thread and calls [done] on it with
     * the copies' `file:` addresses, in order. A document that cannot be read is left out;
     * none readable gives an empty list.
     */
    fun copy(tabId: UInt, uris: List<String>, done: (List<Uri>) -> Unit) {
        thread(name = "krunk-upload") {
            val dir = File(File(root, tabId.toString()), UUID.randomUUID().toString())
            val copies = uris.mapNotNull { one(it.toUri(), dir) }
            main.post { done(copies) }
        }
    }

    private fun one(uri: Uri, dir: File): Uri? = try {
        dir.mkdirs()
        when (uri.scheme) {
            "content" -> {
                val target = free(dir, UploadNames.safe(displayName(uri)))
                app.contentResolver.openInputStream(uri)?.use { input ->
                    target.outputStream().use { input.copyTo(it) }
                    Uri.fromFile(target)
                }
            }

            // Only a photo Krunk had taken itself: no other file address is ever accepted.
            "file" -> {
                val source = File(uri.path.orEmpty()).canonicalFile
                val captures = File(app.cacheDir, CAPTURES).canonicalFile
                if (source.parentFile == captures && source.isFile) {
                    val target = free(dir, source.name)
                    if (source.renameTo(target)) Uri.fromFile(target) else null
                } else {
                    null
                }
            }

            else -> null
        }
    } catch (e: IOException) {
        Log.w(TAG, "could not copy a picked file: ${e.javaClass.simpleName}")
        null
    } catch (e: SecurityException) {
        Log.w(TAG, "no access to a picked file: ${e.javaClass.simpleName}")
        null
    }

    private fun displayName(uri: Uri): String? =
        app.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst() && !c.isNull(0)) c.getString(0) else null
        }

    /** Two picked documents may have the same name. */
    private fun free(dir: File, name: String): File {
        var file = File(dir, name)
        var n = 1
        while (file.exists()) {
            file =
                File(
                    dir,
                    "${name.substringBeforeLast('.', name)}-$n" +
                        if ('.' in name) "." + name.substringAfterLast('.') else "",
                )
            n++
        }
        return file
    }

    companion object {
        private const val TAG = "Krunk"
        private const val UPLOADS = "uploads"
        private const val CAPTURES = "capture"

        /**
         * An empty file for a photo and the address the camera app writes it to (through
         * Krunk's `FileProvider`, which exposes only this folder). The file's own address is
         * what the prompt is answered with.
         */
        fun newCapture(context: Context): Pair<File, Uri> {
            val dir = File(context.cacheDir, CAPTURES).apply { mkdirs() }
            val file = File(dir, "photo-${UUID.randomUUID()}.jpg")
            return file to FileProvider.getUriForFile(context, context.packageName + ".files", file)
        }
    }
}
