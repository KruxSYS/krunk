package org.krunk.browser.downloads

import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.ContentResolver
import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.provider.MediaStore
import android.util.Log
import android.webkit.MimeTypeMap
import androidx.annotation.RequiresApi
import androidx.core.content.FileProvider
import java.io.File
import java.io.FileNotFoundException
import java.io.IOException
import java.net.SocketTimeoutException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.krunk.browser.KrunkApp
import org.krunk.ffi.HttpHeader
import org.mozilla.geckoview.GeckoRuntime
import org.mozilla.geckoview.GeckoWebExecutor
import org.mozilla.geckoview.WebRequest
import org.mozilla.geckoview.WebResponse

/**
 * Fetches through the engine's own network stack ([GeckoWebExecutor]): its cookies, its proxy
 * settings, and with `FETCH_FLAGS_PRIVATE` the cookie jar of the private tabs (they are
 * `usePrivateMode` sessions without a context id). Called from a transfer thread; the engine's
 * result is asked for and observed on the main thread (a `GeckoResult` needs a Looper).
 */
class GeckoFetcher(private val runtime: () -> GeckoRuntime) : DownloadFetcher {
    private val main = Handler(Looper.getMainLooper())

    override fun fetch(url: String, headers: List<HttpHeader>, referrer: String?, private: Boolean): DownloadResponse {
        val done = CountDownLatch(1)
        // One state for "waiting", "answered" and "abandoned": whichever side comes second sees
        // the other's word, so a response that arrives after the waiter gave up is closed.
        val outcome = AtomicReference<Any?>(WAITING)
        fun answer(value: Any) {
            if (!outcome.compareAndSet(WAITING, value)) (value as? WebResponse)?.let { closeBody(it) }
            done.countDown()
        }
        main.post {
            try {
                val request = WebRequest.Builder(url)
                request.method("GET")
                for (h in headers) request.header(h.name, h.value)
                // Lets this request through Krunk's engine-request gate (krunk-guard), which
                // otherwise stops the engine's requests outside a tab; the gate removes it.
                request.header(FETCH_MARK, "1")
                if (DownloadNames.isWeb(referrer)) request.referrer(referrer)
                val flags = if (private) GeckoWebExecutor.FETCH_FLAGS_PRIVATE else GeckoWebExecutor.FETCH_FLAGS_NONE
                GeckoWebExecutor(runtime()).fetch(request.build(), flags).accept(
                    { response -> answer(response ?: IOException("no response")) },
                    { error -> answer(error ?: IOException("request failed")) },
                )
            } catch (e: RuntimeException) {
                answer(e)
            }
        }
        done.await(CONNECT_TIMEOUT_MS, TimeUnit.MILLISECONDS)
        if (outcome.compareAndSet(WAITING, ABANDONED)) throw SocketTimeoutException("no answer")
        return when (val got = outcome.get()) {
            is WebResponse -> response(got)

            else -> {
                // The engine's reason, without the address (it may carry a token).
                Log.w(
                    KrunkApp.TAG,
                    "download request failed: ${(got as? Throwable)?.let {
                        "${it.javaClass.simpleName}: ${it.message}"
                    }}",
                )
                throw IOException((got as? Throwable)?.javaClass?.simpleName ?: "no response")
            }
        }
    }

    private fun closeBody(response: WebResponse) {
        runCatching { response.body?.close() }
    }

    companion object {
        /** See krunk-guard's gate-api.js. */
        const val FETCH_MARK = "X-Krunk-Fetch"
        private val WAITING = Any()
        private val ABANDONED = Any()
        const val CONNECT_TIMEOUT_MS = 90_000L
        const val READ_TIMEOUT_MS = 60_000L

        /** The engine's response as the download code reads it. */
        fun response(response: WebResponse): DownloadResponse {
            response.setReadTimeoutMillis(READ_TIMEOUT_MS)
            return DownloadResponse(response.uri, response.statusCode, response.headers, response.body)
        }
    }
}

/** Android 10 and later: complete files go into the public Downloads collection. */
@RequiresApi(Build.VERSION_CODES.Q)
class MediaStorePublisher(context: Context) : DownloadPublisher {
    private val resolver = context.applicationContext.contentResolver
    private val packageName = context.packageName
    private val collection: Uri = MediaStore.Downloads.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)

    /** Krunk's own files there (others' are not visible; the system renames on a clash with those). */
    override fun taken(): List<String> = try {
        val names = ArrayList<String>()
        resolver.query(collection, arrayOf(MediaStore.MediaColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            while (c.moveToNext()) c.getString(0)?.let(names::add)
        }
        names
    } catch (e: RuntimeException) {
        emptyList()
    }

    override fun publish(part: File, name: String, mime: String?): String {
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, name)
            // Without a type the system derives it from the name; with one that does not fit
            // the name it would add an extension of its own.
            DownloadOpener.mimeForName(name)?.let { put(MediaStore.MediaColumns.MIME_TYPE, it) }
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }
        var uri: Uri? = null
        try {
            uri = resolver.insert(collection, values) ?: throw IOException("no entry")
            val out = resolver.openOutputStream(uri) ?: throw IOException("no stream")
            out.use { part.inputStream().use { input -> input.copyTo(it, COPY_BUFFER) } }
            // Only a complete file loses the pending mark (and becomes visible to others).
            resolver.update(uri, ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }, null, null)
            val stored = resolver.query(
                uri,
                arrayOf(MediaStore.MediaColumns.DISPLAY_NAME),
                null,
                null,
                null,
            )?.use { c ->
                if (c.moveToFirst()) c.getString(0) else null
            }
            return DownloadPaths.join(uri.toString(), stored?.takeIf { it.isNotBlank() && !it.contains('/') } ?: name)
        } catch (e: IOException) {
            uri?.let { runCatching { resolver.delete(it, null, null) } }
            throw e
        } catch (e: RuntimeException) {
            uri?.let { runCatching { resolver.delete(it, null, null) } }
            throw IOException(e)
        }
    }

    /**
     * Krunk's own rows that are still pending (the process died while publishing): deleted, so
     * a retry leaves one file. Other apps' rows are neither visible nor touched.
     */
    override fun sweep() {
        val selection =
            "${MediaStore.MediaColumns.IS_PENDING} = 1 AND ${MediaStore.MediaColumns.OWNER_PACKAGE_NAME} = ?"
        val args = arrayOf(packageName)
        try {
            val ids = ArrayList<Long>()
            val cursor = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                val query = Bundle().apply {
                    putInt(MediaStore.QUERY_ARG_MATCH_PENDING, MediaStore.MATCH_ONLY)
                    putString(ContentResolver.QUERY_ARG_SQL_SELECTION, selection)
                    putStringArray(ContentResolver.QUERY_ARG_SQL_SELECTION_ARGS, args)
                }
                resolver.query(collection, arrayOf(MediaStore.MediaColumns._ID), query, null)
            } else {
                @Suppress("DEPRECATION")
                resolver.query(
                    MediaStore.setIncludePending(collection),
                    arrayOf(MediaStore.MediaColumns._ID),
                    selection,
                    args,
                    null,
                )
            }
            cursor?.use { c -> while (c.moveToNext()) ids += c.getLong(0) }
            for (id in ids) runCatching { resolver.delete(ContentUris.withAppendedId(collection, id), null, null) }
            if (ids.isNotEmpty()) Log.i(KrunkApp.TAG, "downloads: ${ids.size} half-published files removed")
        } catch (e: RuntimeException) {
            Log.w(KrunkApp.TAG, "downloads: pending files not swept: ${e.javaClass.simpleName}")
        }
    }

    private companion object {
        const val COPY_BUFFER = 256 * 1024
    }
}

/**
 * Android 8 and 9 have no Downloads collection an app can write to without the storage
 * permission (which Krunk does not ask for): files stay in the app's external `Download`
 * folder, as before 0.4.0, and are opened and shared through the FileProvider.
 */
class AppFolderPublisher(context: Context) : DownloadPublisher {
    private val context = context.applicationContext

    private fun dir(): File? = context.getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS)

    override fun taken(): List<String> = dir()?.list()?.toList() ?: emptyList()

    override fun publish(part: File, name: String, mime: String?): String {
        val dir = dir() ?: throw IOException("no external storage")
        dir.mkdirs()
        val target = FolderCopy.copyNew(part, dir, name)
        try {
            val uri = FileProvider.getUriForFile(context, "${context.packageName}$AUTHORITY_SUFFIX", target)
            return DownloadPaths.join(uri.toString(), target.name)
        } catch (e: RuntimeException) {
            // Only the file this call created.
            target.delete()
            throw IOException(e)
        }
    }

    private companion object {
        const val AUTHORITY_SUFFIX = ".files"
    }
}

/** Opens and shares finished downloads: always a `content://` address with a read grant. */
object DownloadOpener {
    enum class Result {
        OK,

        /** The file was deleted or moved since. */
        GONE,

        /** No app on the device shows this kind of file. */
        NO_APP,
    }

    fun publisher(context: Context): DownloadPublisher = if (Build.VERSION.SDK_INT >=
        Build.VERSION_CODES.Q
    ) {
        MediaStorePublisher(context)
    } else {
        AppFolderPublisher(context)
    }

    fun extensionFor(mime: String): String? = MimeTypeMap.getSingleton().getExtensionFromMimeType(mime)

    fun mimeForName(name: String): String? {
        val ext = name.substringAfterLast('.', "").lowercase()
        if (ext.isEmpty()) return null
        return MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext)
    }

    private fun exists(context: Context, uri: Uri): Boolean = try {
        context.contentResolver.openFileDescriptor(uri, "r")?.use { true } ?: false
    } catch (e: FileNotFoundException) {
        false
    } catch (e: RuntimeException) {
        false
    }

    private fun mime(context: Context, uri: Uri, name: String): String =
        runCatching { context.contentResolver.getType(uri) }.getOrNull()?.takeIf { it.isNotBlank() }
            ?: mimeForName(name)
            ?: "*/*"

    /** The intent that shows the file of a finished entry, or null when [path] names none. */
    fun viewIntent(context: Context, path: String): Intent? {
        val file = DownloadPaths.split(path) ?: return null
        val uri = Uri.parse(file.uri)
        return Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, mime(context, uri, file.fileName))
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
    }

    /**
     * Opens the file of a finished entry. The checks (the file still there, its type) ask the
     * media provider and run on [Dispatchers.IO]; the activity starts from the caller's thread.
     */
    suspend fun open(context: Context, path: String): Result {
        val intent = withContext(Dispatchers.IO) {
            viewIntent(context, path)?.takeIf { i -> i.data?.let { exists(context, it) } == true }
        } ?: return Result.GONE
        return launch(context, intent)
    }

    /** Shares the file of a finished entry; checks on [Dispatchers.IO] as for [open]. */
    suspend fun share(context: Context, path: String, chooserTitle: CharSequence?): Result {
        val file = DownloadPaths.split(path) ?: return Result.GONE
        val uri = Uri.parse(file.uri)
        val type = withContext(Dispatchers.IO) {
            if (exists(context, uri)) mime(context, uri, file.fileName) else null
        } ?: return Result.GONE
        val send = Intent(Intent.ACTION_SEND).apply {
            this.type = type
            putExtra(Intent.EXTRA_STREAM, uri)
            // The chooser passes the grant on through the clip data.
            clipData = ClipData.newRawUri(file.fileName, uri)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        val chooser = Intent.createChooser(send, chooserTitle)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        return launch(context, chooser)
    }

    private fun launch(context: Context, intent: Intent): Result = try {
        context.startActivity(intent)
        Result.OK
    } catch (e: ActivityNotFoundException) {
        Result.NO_APP
    } catch (e: RuntimeException) {
        Log.w(KrunkApp.TAG, "download not opened: ${e.javaClass.simpleName}")
        Result.NO_APP
    }
}
