package org.krunk.browser.downloads

import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import org.krunk.browser.ui.browser.downloadFileName
import org.krunk.ffi.downloadCleanName
import org.krunk.ffi.downloadUniqueName

/** File names and addresses of downloads as the list keeps them. */
object DownloadNames {
    /** A web address: the only kind that can be fetched again (a link download, a restart). */
    fun isWeb(url: String?): Boolean {
        val u = url?.trim()?.lowercase() ?: return false
        return u.startsWith("http://") || u.startsWith("https://")
    }

    /**
     * The name a download is saved under: the response's `Content-Disposition` name, else the
     * address's last segment (a `blob:` or `data:` address has none), else "download"; an
     * extension from the media type when there is none; then Krunk's clean-up (no path parts,
     * no control or text-reordering characters) and the core's (`downloadCleanName`).
     */
    fun of(url: String, disposition: String?, contentType: String?, extensionFor: (String) -> String?): String {
        var name = downloadFileName(if (isWeb(url)) url else "", disposition)
        if (!name.contains('.') && !contentType.isNullOrBlank()) {
            extensionFor(contentType)?.takeIf { it.isNotBlank() }?.let { name = "$name.$it" }
        }
        return downloadCleanName(name)
    }

    /**
     * The address as the list stores it. A `data:` address is the file itself and can be huge:
     * only its media type is kept, never the payload.
     */
    fun listUrl(url: String): String {
        if (!url.startsWith(DATA, ignoreCase = true)) return url
        val mime = url.substring(DATA.length).substringBefore(',').substringBefore(';').take(MAX_MIME)
        return "$DATA$mime,…"
    }

    private const val DATA = "data:"
    private const val MAX_MIME = 80
}

/**
 * The `path` of a finished entry is `<content uri>/<file name>`: the list takes the file's name
 * from the last part of a path, and the address is what opens or shares the file.
 */
object DownloadPaths {
    class Published(val uri: String, val fileName: String)

    fun join(uri: String, fileName: String): String = "$uri/$fileName"

    /** Null for a path that is not a published file (a download that never finished). */
    fun split(path: String): Published? {
        val cut = path.lastIndexOf('/')
        if (cut <= 0 || cut == path.length - 1) return null
        val uri = path.substring(0, cut)
        if (!uri.startsWith("content://")) return null
        return Published(uri, path.substring(cut + 1))
    }
}

/** Android 8 and 9: a complete file copied into the app's own Downloads folder. */
object FolderCopy {
    private const val MAX_TRIES = 20

    /**
     * Creates [name] (or the next free variant of it) in [dir] and copies [part] there. Never
     * replaces or deletes a file that was there before: the name is taken atomically
     * (`createNewFile`), and on a failure only the file this call created is deleted.
     */
    @Throws(IOException::class)
    fun copyNew(part: File, dir: File, name: String): File {
        val tried = ArrayList<String>()
        var candidate = name
        repeat(MAX_TRIES) {
            val target = File(dir, candidate)
            if (target.createNewFile()) {
                try {
                    part.inputStream().use { input ->
                        FileOutputStream(target).use { out ->
                            input.copyTo(out)
                            out.fd.sync()
                        }
                    }
                    return target
                } catch (e: IOException) {
                    target.delete()
                    throw e
                } catch (e: RuntimeException) {
                    target.delete()
                    throw IOException(e)
                }
            }
            // Somebody has it (it appeared since the name was chosen, or differs only in case).
            tried += candidate
            candidate = downloadUniqueName(name, dir.list().orEmpty().toList() + tried)
        }
        throw IOException("no free name")
    }
}

/** Lets one event per key through every [intervalMs] (notification and list updates). */
class Throttle(private val intervalMs: Long, private val clock: () -> Long) {
    private val last = HashMap<Any, Long>()

    @Synchronized
    fun pass(key: Any): Boolean {
        val now = clock()
        val before = last[key]
        if (before != null && now - before in 0 until intervalMs) return false
        last[key] = now
        return true
    }

    @Synchronized
    fun forget(key: Any) {
        last.remove(key)
    }
}
