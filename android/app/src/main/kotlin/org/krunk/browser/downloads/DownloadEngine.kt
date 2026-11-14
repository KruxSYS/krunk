package org.krunk.browser.downloads

import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.security.SecureRandom
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executor
import org.krunk.ffi.DownloadEntry
import org.krunk.ffi.DownloadList
import org.krunk.ffi.DownloadResume
import org.krunk.ffi.DownloadState
import org.krunk.ffi.DownloadVersion
import org.krunk.ffi.FfiException
import org.krunk.ffi.HttpHeader
import org.krunk.ffi.ResumeVerdict
import org.krunk.ffi.downloadComplete
import org.krunk.ffi.downloadRangeHeaders
import org.krunk.ffi.downloadResumable
import org.krunk.ffi.downloadResumeCheck
import org.krunk.ffi.downloadUniqueName

/** A response of the engine's network stack whose body is (or may be) saved. */
class DownloadResponse(val url: String, val status: Int, val headers: Map<String, String>, val body: InputStream?) {
    fun header(name: String): String? = headers.entries.firstOrNull { it.key.equals(name, ignoreCase = true) }?.value

    /** The media type without its parameters, lower case. */
    val contentType: String?
        get() = header("Content-Type")?.substringBefore(';')?.trim()?.lowercase()?.ifEmpty { null }

    fun close() {
        runCatching { body?.close() }
    }
}

/** Asks the engine's network stack for a URL (its cookies, its proxy, the private jar). Blocks. */
fun interface DownloadFetcher {
    @Throws(IOException::class)
    fun fetch(url: String, headers: List<HttpHeader>, referrer: String?, private: Boolean): DownloadResponse
}

/** Where complete files go (the public Downloads collection). */
interface DownloadPublisher {
    /** Names that are in use there (as far as the app can see). */
    fun taken(): List<String>

    /**
     * Copies the complete [part] file there as [name]. Returns what the list stores as the
     * path: `<content uri>/<file name>` (see [DownloadPaths]).
     */
    @Throws(IOException::class)
    fun publish(part: File, name: String, mime: String?): String
}

/** A new download. With a [response] the engine's open stream is saved; without, the URL is fetched. */
class DownloadRequest(
    val url: String,
    /** The page it comes from: the referrer of a fetch, and what the list shows. */
    val pageUrl: String?,
    val private: Boolean,
    /** The name to show until the response names the file. */
    val displayName: String? = null,
    val response: DownloadResponse? = null,
)

/** The reasons the list stores with a failed or paused download (texts from the resources). */
data class DownloadReasons(
    val network: String,
    val timedOut: String,
    val endedEarly: String,
    /** `%d` is the HTTP status. */
    val serverError: String,
    val cannotContinue: String,
    val partGone: String,
    val storage: String,
    val publish: String,
    /** Must start with "Paused" (see `DownloadRow.pausedOnPurpose`). */
    val timeLimit: String,
    val timeLimitStopped: String,
)

/**
 * Krunk's own downloads: saves response streams into part files, publishes complete files and
 * keeps the list ([DownloadList], the shared core) in step. Plain Kotlin; everything outside
 * is handed in.
 *
 * The states (plan, 2.1) and what the list allows: a finished state (Completed, Interrupted,
 * Cancelled) is final. So an error on a download that can go on is *Paused* with a reason
 * ("Retry" continues it), an error on one that cannot is *Interrupted*, and starting that one
 * over is a new entry ([restart]). A download is never started over silently: when the server
 * does not continue the same file the entry ends as Interrupted.
 */
class DownloadEngine(
    private val list: DownloadList,
    private val partDir: File,
    private val fetcher: DownloadFetcher,
    private val publisher: DownloadPublisher,
    private val reasons: DownloadReasons,
    private val clock: () -> Long,
    /** Runs the transfers (each call blocks until its download stops). */
    private val executor: Executor,
    private val listener: Listener,
    /** A file extension for a media type, when the name has none. */
    private val extensionFor: (String) -> String? = { null },
    private val openPart: (File, Boolean) -> OutputStream = { file, append -> FileOutputStream(file, append) },
) {
    interface Listener {
        /** The entry changed state (or its name); for a private one this may be its last sign. */
        fun onState(entry: DownloadEntry)

        /** Bytes arrived (called often: throttle). */
        fun onProgress(id: UInt)
    }

    private class Stop(val state: DownloadState)

    private class Job(val id: UInt, val url: String, val part: File, val private: Boolean, val referrer: String?) {
        @Volatile var stop: Stop? = null

        @Volatile var response: DownloadResponse? = null
        var previous: Job? = null
        var finishing = false
        var version: DownloadVersion? = null
        var name: String = ""
        var mime: String? = null
        val done = CountDownLatch(1)
    }

    private val lock = Any()
    private val publishLock = Any()
    private val jobs = HashMap<UInt, Job>()

    /** Stopped transfers whose thread has not let go of the part file yet. */
    private val closing = HashMap<UInt, Job>()
    private val random = SecureRandom()

    /** Downloads that are being transferred right now. */
    fun runningCount(): Int = synchronized(lock) { jobs.size }

    fun entry(id: UInt): DownloadEntry? = try {
        list.get(id)
    } catch (e: FfiException) {
        null
    }

    /**
     * After a start of the app: [DownloadList.load] has already turned what was running into
     * Paused (it can go on) or Interrupted. Here a paused entry whose part file is gone becomes
     * Interrupted, the others learn how much is there, and part files that belong to no
     * unfinished entry (a private download's, a cancelled one's) are deleted.
     */
    fun recover() {
        val keep = HashSet<String>()
        for (e in list.unfinished()) {
            val running = synchronized(lock) { jobs[e.id] ?: closing[e.id] }
            if (running != null) {
                keep += running.part.name
                continue
            }
            val part = e.resume?.let { partOf(it) }
            if (e.state == DownloadState.PAUSED && part != null && part.isFile) {
                keep += part.name
                list.progress(e.id, part.length(), null, clock())
            } else {
                setState(e.id, DownloadState.INTERRUPTED, reasons.partGone)
            }
        }
        partDir.listFiles()?.forEach { if (it.name !in keep) it.delete() }
    }

    /** Starts a download; null when the list could not take it (the response is closed then). */
    fun start(request: DownloadRequest): UInt? {
        val name = request.displayName
            ?: request.response?.let {
                DownloadNames.of(it.url, it.header("Content-Disposition"), it.contentType, extensionFor)
            }
            ?: DownloadNames.of(request.url, null, null, extensionFor)
        val job: Job
        try {
            partDir.mkdirs()
            synchronized(lock) {
                val id = list.start(
                    DownloadNames.listUrl(request.url),
                    "$PENDING_DIR/$name",
                    null,
                    clock(),
                    request.private,
                    request.pageUrl,
                    null,
                )
                job = Job(id, request.url, newPartFile(), request.private, request.pageUrl)
                job.name = name
                jobs[id] = job
            }
        } catch (e: FfiException) {
            request.response?.close()
            return null
        }
        announce(job.id)
        executor.execute { transfer(job, request.response, resuming = false) }
        return job.id
    }

    /** Pauses a running download that can go on later; false when it cannot be paused. */
    fun pause(id: UInt): Boolean = stop(id, DownloadState.PAUSED, null, onlyResumable = true)

    /** Cancels a running or paused download; its part file is deleted. */
    fun cancel(id: UInt): Boolean {
        if (stop(id, DownloadState.CANCELLED, null, onlyResumable = false)) return true
        val e = entry(id) ?: return false
        if (e.state != DownloadState.PAUSED) return false
        val part = e.resume?.let { partOf(it) }
        val waitFor = synchronized(lock) { closing[id] }
        setState(id, DownloadState.CANCELLED, null)
        waitFor?.done?.await()
        part?.delete()
        return true
    }

    /** Android's time limit for the background service: pause what can go on, end the rest. */
    fun timeLimit() {
        val ids = synchronized(lock) { jobs.keys.toList() }
        for (id in ids) {
            if (!stop(id, DownloadState.PAUSED, reasons.timeLimit, onlyResumable = true)) {
                stop(id, DownloadState.INTERRUPTED, reasons.timeLimitStopped, onlyResumable = false)
            }
        }
    }

    /** Goes on with a paused download ("Resume" and "Retry"). */
    fun resume(id: UInt): Boolean {
        var failed: String? = null
        val job: Job? = synchronized(lock) {
            if (jobs.containsKey(id)) return false
            val e = entry(id) ?: return false
            if (e.state != DownloadState.PAUSED) return false
            val r = e.resume
            val part = r?.let { partOf(it) }?.takeIf { it.isFile }
            if (r == null || part == null) {
                failed = if (r == null) reasons.cannotContinue else reasons.partGone
                null
            } else {
                try {
                    list.setState(id, DownloadState.IN_PROGRESS, null, clock())
                } catch (e: FfiException) {
                    return false
                }
                Job(id, e.url, part, e.private, r.referer).also {
                    it.version = DownloadVersion(r.total, r.etag, r.lastModified)
                    it.name = e.fileName
                    it.previous = closing[id]
                    jobs[id] = it
                }
            }
        }
        if (job == null) {
            setState(id, DownloadState.INTERRUPTED, failed)
            return false
        }
        announce(id)
        executor.execute { transfer(job, null, resuming = true) }
        return true
    }

    /**
     * Starts a failed or cancelled download over as a new entry and takes the old one off the
     * list. Only for web addresses: a `blob:` or `data:` address is gone with its page.
     */
    fun restart(id: UInt): UInt? {
        val e = entry(id) ?: return null
        if (e.state != DownloadState.INTERRUPTED && e.state != DownloadState.CANCELLED) return null
        if (!DownloadNames.isWeb(e.url)) return null
        val fresh = start(DownloadRequest(e.url, e.pageUrl, e.private, displayName = e.fileName)) ?: return null
        remove(id)
        return fresh
    }

    /** Takes a finished download off the list (the file stays). */
    fun remove(id: UInt): Boolean = try {
        list.remove(id)
    } catch (e: FfiException) {
        false
    }

    fun clearFinished() {
        try {
            list.clearFinished()
        } catch (e: FfiException) {
            // The list stays as it is.
        }
    }

    // ----- the transfer -----

    private fun transfer(job: Job, given: DownloadResponse?, resuming: Boolean) {
        var out: OutputStream? = null
        try {
            job.previous?.done?.await()
            job.previous = null
            val version = job.version
            val offset = if (resuming) job.part.length() else 0L
            var total: Long? = version?.total?.toLong()
            var received = offset
            if (resuming && version != null && offset > version.total.toLong()) {
                settle(job, DownloadState.INTERRUPTED, reasons.cannotContinue)
                return
            }
            if (!(resuming && version != null && offset == version.total.toLong())) {
                val response =
                    given ?: fetcher.fetch(job.url, requestHeaders(offset, version), job.referrer, job.private)
                job.response = response
                if (job.stop != null) return
                if (offset > 0 && version != null) {
                    val verdict = downloadResumeCheck(
                        response.status.toUShort(),
                        headersOf(response),
                        offset.toULong(),
                        version.total,
                        version.etag,
                        version.lastModified,
                    )
                    if (verdict != ResumeVerdict.APPEND) {
                        settle(job, DownloadState.INTERRUPTED, reasons.cannotContinue)
                        return
                    }
                } else {
                    total = begin(job, response) ?: return
                    total = total.takeIf { it >= 0 }
                }
                job.mime = response.contentType ?: job.mime
                val body = response.body ?: throw IOException("no body")
                out = try {
                    openPart(job.part, offset > 0)
                } catch (e: IOException) {
                    throw PartWriteException(e)
                }
                list.progress(job.id, received, total, clock())
                val buffer = ByteArray(BUFFER)
                while (true) {
                    val n = body.read(buffer)
                    if (n < 0) break
                    if (job.stop != null) return
                    try {
                        out.write(buffer, 0, n)
                    } catch (e: IOException) {
                        throw PartWriteException(e)
                    }
                    received += n
                    list.progress(job.id, received, total, clock())
                    listener.onProgress(job.id)
                }
                try {
                    out.flush()
                    out.close()
                } catch (e: IOException) {
                    throw PartWriteException(e)
                }
                out = null
            }
            if (job.stop != null) return
            // A stream that ends before the announced length is a failure, never a file.
            if (!downloadComplete(received.toULong(), total?.toULong())) throw ShortStreamException()
            synchronized(lock) {
                if (job.stop != null) return
                job.finishing = true
            }
            complete(job)
        } catch (e: PartWriteException) {
            fail(job, reasons.storage)
        } catch (e: ShortStreamException) {
            fail(job, reasons.endedEarly)
        } catch (e: PublishException) {
            fail(job, reasons.publish)
        } catch (e: java.net.SocketTimeoutException) {
            fail(job, reasons.timedOut)
        } catch (e: IOException) {
            fail(job, reasons.network)
        } catch (e: FfiException) {
            fail(job, reasons.storage)
        } catch (e: RuntimeException) {
            fail(job, reasons.network)
        } finally {
            runCatching { out?.close() }
            job.response?.close()
            given?.close()
            val end = job.stop?.state
            if (end != null && end != DownloadState.PAUSED) job.part.delete()
            synchronized(lock) {
                if (jobs[job.id] === job) jobs.remove(job.id)
                if (closing[job.id] === job) closing.remove(job.id)
            }
            job.done.countDown()
        }
    }

    /**
     * The first response of a download: its status, whether it can go on later, its name.
     * Returns the announced length (-1: none), or null when the download ended here.
     */
    private fun begin(job: Job, response: DownloadResponse): Long? {
        val ok = response.status in 200..299 || (response.status == 0 && !DownloadNames.isWeb(job.url))
        if (!ok) {
            settle(job, DownloadState.INTERRUPTED, reasons.serverError.format(response.status))
            return null
        }
        val headers = headersOf(response)
        // The engine hands out the decoded body: a compressed response's length is not the
        // file's (and such a download cannot go on; the core's rule says the same).
        val encoded = response.header("Content-Encoding")?.trim()?.lowercase().let { it != null && it != "identity" }
        val version = if (response.status ==
            206
        ) {
            null
        } else {
            downloadResumable(job.url, response.status.toUShort(), headers)
        }
        val length = if (encoded) {
            null
        } else {
            response.header("Content-Length")?.trim()?.toLongOrNull()?.takeIf {
                it >= 0
            }
        }
        val name =
            DownloadNames.of(response.url, response.header("Content-Disposition"), response.contentType, extensionFor)
        synchronized(lock) {
            if (job.stop != null) return null
            job.version = version
            if (name != job.name) {
                job.name = name
                list.setPath(job.id, "$PENDING_DIR/$name")
            }
            list.setResume(
                job.id,
                version?.let { DownloadResume(job.part.path, it.total, it.etag, it.lastModified, job.referrer) },
            )
        }
        announce(job.id)
        return version?.total?.toLong() ?: length ?: -1
    }

    private fun complete(job: Job) {
        val path = try {
            synchronized(publishLock) {
                publisher.publish(job.part, downloadUniqueName(job.name, publisher.taken()), job.mime)
            }
        } catch (e: IOException) {
            synchronized(lock) { job.finishing = false }
            throw PublishException()
        }
        val last: DownloadEntry?
        synchronized(lock) {
            job.stop = Stop(DownloadState.COMPLETED)
            jobs.remove(job.id)
            list.setPath(job.id, path)
            list.setState(job.id, DownloadState.COMPLETED, null, clock())
            last = entry(job.id)
            // A private download leaves nothing but its file.
            if (job.private) list.remove(job.id)
        }
        last?.let(listener::onState)
    }

    /** An error while transferring: paused with its reason when it can go on, else failed. */
    private fun fail(job: Job, reason: String) {
        val canGoOn = job.version != null && job.part.isFile
        settle(job, if (canGoOn) DownloadState.PAUSED else DownloadState.INTERRUPTED, reason)
    }

    /** Ends a transfer from its own thread, unless somebody stopped it first. */
    private fun settle(job: Job, state: DownloadState, reason: String?) {
        val last: DownloadEntry?
        synchronized(lock) {
            if (job.stop != null) return
            job.stop = Stop(state)
            jobs.remove(job.id)
            last = write(job, state, reason)
        }
        last?.let(listener::onState)
    }

    /** Stops a running transfer from outside. */
    private fun stop(id: UInt, state: DownloadState, reason: String?, onlyResumable: Boolean): Boolean {
        val job: Job
        val last: DownloadEntry?
        synchronized(lock) {
            job = jobs[id] ?: return false
            if (job.finishing || job.stop != null) return false
            if (onlyResumable && job.version == null) return false
            job.stop = Stop(state)
            jobs.remove(id)
            closing[id] = job
            last = write(job, state, reason)
        }
        // Wakes the transfer's thread out of a read.
        job.response?.close()
        last?.let(listener::onState)
        return true
    }

    /** Writes a state; returns the entry as it is then (a private one is dropped when it ended). */
    private fun write(job: Job, state: DownloadState, reason: String?): DownloadEntry? = try {
        list.setState(job.id, state, reason, clock())
        val e = list.get(job.id)
        if (job.private && state != DownloadState.PAUSED) list.remove(job.id)
        e
    } catch (e: FfiException) {
        null
    }

    private fun setState(id: UInt, state: DownloadState, reason: String?) {
        try {
            list.setState(id, state, reason, clock())
        } catch (e: FfiException) {
            return
        }
        announce(id)
    }

    private fun announce(id: UInt) {
        entry(id)?.let(listener::onState)
    }

    private fun requestHeaders(offset: Long, version: DownloadVersion?): List<HttpHeader> =
        if (offset > 0 && version != null) {
            downloadRangeHeaders(offset.toULong(), version.etag, version.lastModified)
        } else {
            // The file as it is: its length can be checked and it can go on after a pause.
            listOf(HttpHeader("Accept-Encoding", "identity"))
        }

    private fun headersOf(response: DownloadResponse): List<HttpHeader> = response.headers.map {
        HttpHeader(it.key, it.value)
    }

    /** The part file an entry names, only if it is one of ours. */
    private fun partOf(resume: DownloadResume): File? =
        File(resume.temp).takeIf { it.parentFile == partDir && it.name.endsWith(PART_SUFFIX) }

    private fun newPartFile(): File {
        val bytes = ByteArray(16)
        random.nextBytes(bytes)
        return File(partDir, bytes.joinToString("") { "%02x".format(it) } + PART_SUFFIX)
    }

    private class PartWriteException(cause: IOException) : IOException(cause)

    private class ShortStreamException : IOException()

    private class PublishException : IOException()

    companion object {
        /** The folder under `filesDir` for part files (never backed up, see the backup rules). */
        const val PART_DIR = "downloads-part"
        const val PART_SUFFIX = ".part"

        /** What an entry's path starts with until the file is published. */
        const val PENDING_DIR = "Download"
        private const val BUFFER = 64 * 1024
    }
}
