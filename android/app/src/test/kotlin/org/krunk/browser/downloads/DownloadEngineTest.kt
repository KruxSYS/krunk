package org.krunk.browser.downloads

import java.io.File
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.security.MessageDigest
import java.util.concurrent.Executor
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.krunk.ffi.DownloadEntry
import org.krunk.ffi.DownloadFilter
import org.krunk.ffi.DownloadList
import org.krunk.ffi.DownloadState
import org.krunk.ffi.HttpHeader
import org.krunk.ffi.KrunkStore

/**
 * The download state machine against a fake server, a fake Downloads folder and the real list
 * (the shared core, on an in-memory store). Transfers run on the calling thread, so a test
 * reaches "in the middle of a download" through the server's [FakeServer.onServed] hook.
 */
class DownloadEngineTest {
    @get:Rule
    val tmp = TemporaryFolder()

    private val reasons = DownloadReasons(
        network = "network",
        timedOut = "timed out",
        endedEarly = "ended early",
        serverError = "server error %d",
        cannotContinue = "The server cannot continue this download",
        partGone = "part gone",
        storage = "storage",
        publish = "publish",
        timeLimit = "Paused by Android",
        timeLimitStopped = "Stopped by Android",
    )

    /** A stream of [data] from [from], in chunks; it can fail, end early, and tell how far it is. */
    private class FakeBody(
        private val data: ByteArray,
        from: Int,
        private val until: Int,
        private val failAt: Int?,
        private val onServed: (Int) -> Unit,
    ) : InputStream() {
        private var at = from
        var closed = false

        override fun read(): Int = throw UnsupportedOperationException()

        override fun read(b: ByteArray, off: Int, len: Int): Int {
            if (closed) throw IOException("closed")
            if (failAt != null && at >= failAt) throw IOException("connection reset")
            if (at >= until) return -1
            var n = minOf(CHUNK, len, until - at)
            if (failAt != null) n = minOf(n, failAt - at)
            System.arraycopy(data, at, b, off, n)
            at += n
            onServed(at)
            return n
        }

        override fun close() {
            closed = true
        }
    }

    /** A file on a server: whole (200) or from an offset (206) when it supports that. */
    private class FakeServer(val data: ByteArray) : DownloadFetcher {
        var etag: String? = "\"v1\""
        var ranges = true
        var sendLength = true
        var status = 200
        var disposition: String? = null
        var contentType: String? = "application/octet-stream"

        /** The stream breaks with an error once this many bytes of the file were served. */
        var failAt: Int? = null

        /** The stream ends cleanly here although the file is longer. */
        var endAt: Int? = null
        var onServed: (Int) -> Unit = {}
        val requests = ArrayList<List<HttpHeader>>()
        val referrers = ArrayList<String?>()
        val privateFlags = ArrayList<Boolean>()
        val bodies = ArrayList<FakeBody>()
        var down = false

        override fun fetch(
            url: String,
            headers: List<HttpHeader>,
            referrer: String?,
            private: Boolean,
        ): DownloadResponse {
            requests += headers
            referrers += referrer
            privateFlags += private
            if (down) throw IOException("no route")
            val range = headers.firstOrNull { it.name == "Range" }?.value
            val ifRange = headers.firstOrNull { it.name == "If-Range" }?.value
            val from = range?.removePrefix("bytes=")?.removeSuffix("-")?.toInt()
            return if (from != null && ranges && ifRange == etag) {
                response(url, 206, from, mapOf("Content-Range" to "bytes $from-${data.size - 1}/${data.size}"))
            } else {
                response(url, status, 0, emptyMap())
            }
        }

        fun response(
            url: String,
            status: Int = this.status,
            from: Int = 0,
            extra: Map<String, String> = emptyMap(),
        ): DownloadResponse {
            val h = HashMap<String, String>(extra)
            if (sendLength) h["Content-Length"] = (data.size - from).toString()
            etag?.let { h["ETag"] = it }
            if (ranges) h["Accept-Ranges"] = "bytes"
            disposition?.let { h["Content-Disposition"] = it }
            contentType?.let { h["Content-Type"] = it }
            val body = FakeBody(data, from, endAt ?: data.size, failAt, onServed)
            bodies += body
            return DownloadResponse(url, status, h, body)
        }
    }

    private class FakeFolder : DownloadPublisher {
        val files = LinkedHashMap<String, ByteArray>()
        var fail = false

        override fun taken(): List<String> = files.keys.toList()

        override fun publish(part: File, name: String, mime: String?): String {
            if (fail) throw IOException("no space")
            files[name] = part.readBytes()
            return DownloadPaths.join("content://fake/${files.size}", name)
        }
    }

    private class Events : DownloadEngine.Listener {
        val states = ArrayList<DownloadEntry>()
        var progress = 0

        override fun onState(entry: DownloadEntry) {
            states += entry
        }

        override fun onProgress(id: UInt) {
            progress++
        }

        fun last(id: UInt): DownloadEntry = states.last { it.id == id }
    }

    private class Died : Error()

    private val store = KrunkStore.inMemory()
    private var now = 1_000L
    private val folder = FakeFolder()
    private val events = Events()
    private val partDir: File by lazy { File(tmp.root, DownloadEngine.PART_DIR) }
    private var list = DownloadList.load(store, now)
    private var openPart: (File, Boolean) -> OutputStream = { f, append -> java.io.FileOutputStream(f, append) }

    private fun engine(server: DownloadFetcher): DownloadEngine = DownloadEngine(
        list = list,
        partDir = partDir,
        fetcher = server,
        publisher = folder,
        reasons = reasons,
        clock = {
            now += 100
            now
        },
        executor = Executor { it.run() },
        listener = events,
        extensionFor = { if (it == "text/plain") "txt" else null },
        openPart = { f, append -> openPart(f, append) },
    )

    private fun bytes(n: Int): ByteArray = ByteArray(n) { (it * 31 + it / 7).toByte() }

    private fun sha(b: ByteArray): String = MessageDigest.getInstance("SHA-256").digest(b).joinToString("") {
        "%02x".format(it)
    }

    private fun parts(): List<File> = partDir.listFiles()?.toList() ?: emptyList()

    private fun state(id: UInt): DownloadState? = list.get(id)?.state

    private fun link(url: String = URL, private: Boolean = false) = DownloadRequest(url, "https://a.test/page", private)

    @Test
    fun startProgressComplete() {
        val server = FakeServer(bytes(1000))
        val e = engine(server)
        val id = e.start(link())!!
        val entry = list.get(id)!!
        assertEquals(DownloadState.COMPLETED, entry.state)
        assertEquals("content://fake/1/file.bin", entry.path)
        assertEquals("file.bin", entry.fileName)
        assertEquals(1000L, entry.received)
        assertEquals("https://a.test/page", entry.pageUrl)
        assertArrayEquals(server.data, folder.files["file.bin"])
        assertTrue("the part file is gone once the file is published", parts().isEmpty())
        assertTrue(events.progress >= 10)
        assertEquals(DownloadState.IN_PROGRESS, events.states.first().state)
        assertNotNull("a resumable download says so while it runs", events.states.first { it.resume != null })
        // Fetched like the tab would: the page as referrer, the file as it is (no compression).
        assertEquals(listOf<String?>("https://a.test/page"), server.referrers)
        assertEquals(listOf(HttpHeader("Accept-Encoding", "identity")), server.requests.single())
        assertEquals(0, e.runningCount())
    }

    @Test
    fun pauseKeepsThePartAndResumeAppends() {
        val server = FakeServer(bytes(1000))
        val e = engine(server)
        var id = 0u
        server.onServed = { if (it == 300) assertTrue(e.pause(id)) }
        id = e.start(link())!!
        server.onServed = {}
        assertEquals(DownloadState.PAUSED, state(id))
        assertNull(list.get(id)!!.reason)
        assertTrue(server.bodies.single().closed)
        val part = parts().single()
        assertEquals("only whole chunks before the pause are kept", 200L, part.length())
        assertTrue(folder.files.isEmpty())
        assertFalse("a paused download is not paused again", e.pause(id))

        assertTrue(e.resume(id))
        assertEquals(DownloadState.COMPLETED, state(id))
        val request = server.requests.last()
        assertTrue(request.contains(HttpHeader("Range", "bytes=200-")))
        assertTrue(request.contains(HttpHeader("If-Range", "\"v1\"")))
        assertEquals(sha(server.data), sha(folder.files.getValue("file.bin")))
        assertTrue(parts().isEmpty())
        assertFalse("a finished download does not go on", e.resume(id))
    }

    @Test
    fun errorOnAResumableDownloadIsAPauseWithItsReasonAndRetryContinues() {
        val server = FakeServer(bytes(1000))
        server.failAt = 450
        val e = engine(server)
        val id = e.start(link())!!
        val entry = list.get(id)!!
        assertEquals(DownloadState.PAUSED, entry.state)
        assertEquals("network", entry.reason)
        assertEquals(450L, parts().single().length())
        assertTrue(folder.files.isEmpty())

        server.failAt = null
        assertTrue(e.resume(id))
        assertEquals(DownloadState.COMPLETED, state(id))
        assertNull(list.get(id)!!.reason)
        assertArrayEquals(server.data, folder.files["file.bin"])
    }

    @Test
    fun errorOnADownloadThatCannotGoOnFailsAndRestartIsANewEntry() {
        val server = FakeServer(bytes(1000))
        server.etag = null // nothing to recognise the file by later
        server.failAt = 450
        val e = engine(server)
        val id = e.start(link())!!
        var entry = list.get(id)!!
        assertEquals(DownloadState.INTERRUPTED, entry.state)
        assertEquals("network", entry.reason)
        assertTrue("its part file is deleted", parts().isEmpty())
        assertFalse(e.resume(id))
        assertFalse(e.pause(id))

        server.failAt = null
        val fresh = e.restart(id)!!
        assertNotEquals(id, fresh)
        assertNull("the failed entry is off the list", list.get(id))
        entry = list.get(fresh)!!
        assertEquals(DownloadState.COMPLETED, entry.state)
        assertArrayEquals(server.data, folder.files["file.bin"])
        assertNull("a finished download is not started over", e.restart(fresh))
    }

    @Test
    fun noAnswerAtAllFailsAndCanBeRestarted() {
        val server = FakeServer(bytes(10))
        server.down = true
        val e = engine(server)
        val id = e.start(link())!!
        assertEquals(DownloadState.INTERRUPTED, state(id))
        assertEquals("network", list.get(id)!!.reason)
        server.down = false
        assertEquals(DownloadState.COMPLETED, state(e.restart(id)!!))
    }

    @Test
    fun serverErrorIsNeverSavedAsAFile() {
        val server = FakeServer(bytes(100))
        server.status = 404
        val id = engine(server).start(link())!!
        assertEquals(DownloadState.INTERRUPTED, state(id))
        assertEquals("server error 404", list.get(id)!!.reason)
        assertTrue(folder.files.isEmpty())
        assertTrue(parts().isEmpty())
        assertTrue(server.bodies.single().closed)
    }

    @Test
    fun cancelDeletesThePart() {
        val server = FakeServer(bytes(1000))
        val e = engine(server)
        var id = 0u
        server.onServed = { if (it == 300) assertTrue(e.cancel(id)) }
        id = e.start(link())!!
        assertEquals(DownloadState.CANCELLED, state(id))
        assertTrue(parts().isEmpty())
        assertTrue(folder.files.isEmpty())
        assertTrue(server.bodies.single().closed)

        // A paused one too.
        var second = 0u
        server.onServed = { if (it == 300) e.pause(second) }
        second = e.start(link())!!
        assertEquals(1, parts().size)
        assertTrue(e.cancel(second))
        assertEquals(DownloadState.CANCELLED, state(second))
        assertTrue(parts().isEmpty())
        assertFalse(e.resume(second))
        assertFalse(e.cancel(second))
    }

    @Test
    fun afterTheProcessDiedDownloadsGoOnOrFailAndStrayPartsAreSwept() {
        val server = FakeServer(bytes(1000))
        val first = engine(server)
        // Dies in the middle of a resumable download ...
        server.onServed = { if (it == 600) throw Died() }
        val resumable = try {
            first.start(link())
            error("not reached")
        } catch (e: Died) {
            list.unfinished().single().id
        }
        // ... of one that cannot go on, of a private one ...
        server.etag = null
        runCatching { first.start(link("https://a.test/stream")) }.exceptionOrNull() as Died
        server.etag = "\"v1\""
        runCatching { first.start(link("https://a.test/secret", private = true)) }.exceptionOrNull() as Died
        // ... and with a paused download whose part file somebody deleted, and one that kept it.
        var lost = 0u
        server.onServed = { if (it == 300) first.pause(lost) }
        lost = first.start(link("https://a.test/lost"))!!
        val lostPart = File(list.get(lost)!!.resume!!.temp)
        var kept = 0u
        server.onServed = { if (it == 300) first.pause(kept) }
        kept = first.start(link("https://a.test/kept"))!!
        server.onServed = {}
        assertEquals(5, parts().size)
        assertTrue(lostPart.delete())
        File(partDir, "stray.part").writeBytes(ByteArray(10))
        val cannot = list.unfinished().first { it.url == "https://a.test/stream" }.id

        // The next start of the app: a new list on the same database, the same folder.
        list = DownloadList.load(store, now)
        val second = engine(server)
        second.recover()

        val entry = list.get(resumable)!!
        assertEquals(DownloadState.PAUSED, entry.state)
        assertEquals("Paused when Krunk closed", entry.reason)
        assertEquals("what the part file holds", 500L, entry.received)
        assertEquals(DownloadState.INTERRUPTED, state(cannot))
        assertEquals("Browser closed", list.get(cannot)!!.reason)
        assertEquals(DownloadState.INTERRUPTED, state(lost))
        assertEquals("part gone", list.get(lost)!!.reason)
        assertEquals(DownloadState.PAUSED, state(kept))
        assertTrue(
            "nothing of the private download is on the list",
            list.unfinished().none {
                it.url.endsWith("secret")
            },
        )
        assertEquals("only the parts of downloads that can go on stay", 2, parts().size)
        assertTrue(folder.files.isEmpty())

        assertTrue(second.resume(resumable))
        assertEquals(DownloadState.COMPLETED, state(resumable))
        assertTrue(server.requests.last().contains(HttpHeader("Range", "bytes=500-")))
        assertEquals(sha(server.data), sha(folder.files.getValue("file.bin")))
        assertTrue(second.resume(kept))
        assertArrayEquals(server.data, folder.files["kept"])
        assertTrue(parts().isEmpty())
    }

    @Test
    fun aServerThatSendsTheWholeFileAgainEndsTheDownloadAndRestartIsNew() {
        val server = FakeServer(bytes(1000))
        val e = engine(server)
        var id = 0u
        server.onServed = { if (it == 300) e.pause(id) }
        id = e.start(link())!!
        server.onServed = {}
        server.ranges = false // answers 200 with the whole file

        assertTrue(e.resume(id))
        val entry = list.get(id)!!
        assertEquals(DownloadState.INTERRUPTED, entry.state)
        assertEquals("The server cannot continue this download", entry.reason)
        assertTrue("nothing was saved in its place", folder.files.isEmpty())
        assertTrue(parts().isEmpty())
        assertTrue(server.bodies.last().closed)
        assertTrue(events.states.none { it.id == id && it.state == DownloadState.COMPLETED })

        val fresh = e.restart(id)!!
        assertNotEquals(id, fresh)
        assertNull(list.get(id))
        assertEquals(DownloadState.COMPLETED, state(fresh))
        assertArrayEquals(server.data, folder.files["file.bin"])
    }

    @Test
    fun aChangedFileEndsTheDownload() {
        val server = FakeServer(bytes(1000))
        val e = engine(server)
        var id = 0u
        server.onServed = { if (it == 300) e.pause(id) }
        id = e.start(link())!!
        server.onServed = {}
        server.etag = "\"v2\""
        assertTrue(e.resume(id))
        assertEquals(DownloadState.INTERRUPTED, state(id))
        assertEquals("The server cannot continue this download", list.get(id)!!.reason)
        assertTrue(folder.files.isEmpty())
    }

    @Test
    fun aStreamThatEndsShortIsNeverComplete() {
        val server = FakeServer(bytes(1000))
        server.endAt = 600
        val e = engine(server)
        val id = e.start(link())!!
        var entry = list.get(id)!!
        assertEquals("it can go on: paused with the reason", DownloadState.PAUSED, entry.state)
        assertEquals("ended early", entry.reason)
        assertTrue(folder.files.isEmpty())
        server.endAt = null
        assertTrue(e.resume(id))
        assertArrayEquals(server.data, folder.files["file.bin"])

        server.endAt = 600
        server.etag = null
        val second = e.start(link("https://a.test/other.bin"))!!
        entry = list.get(second)!!
        assertEquals(DownloadState.INTERRUPTED, entry.state)
        assertEquals("ended early", entry.reason)
        assertNull(folder.files["other.bin"])
        assertTrue(parts().isEmpty())
    }

    @Test
    fun aStreamWithoutALengthIsCompleteWhenItEndsCleanlyAndCannotBePaused() {
        val server = FakeServer(bytes(700))
        server.sendLength = false
        val e = engine(server)
        var id = 0u
        var paused: Boolean? = null
        server.onServed = { if (it == 300) paused = e.pause(id) }
        id = e.start(link())!!
        assertEquals(false, paused)
        val entry = list.get(id)!!
        assertEquals(DownloadState.COMPLETED, entry.state)
        assertEquals(700L, entry.received)
        assertArrayEquals(server.data, folder.files["file.bin"])
    }

    @Test
    fun aResponseOfThePageIsSavedWithoutAnotherRequest() {
        val server = FakeServer("hello blob".toByteArray())
        server.contentType = "text/plain"
        server.disposition = "attachment; filename=\"notes\""
        val e = engine(server)
        val url = "blob:https://a.test/1b9d6bcd"
        var id = 0u
        var paused: Boolean? = null
        server.onServed = { paused = e.pause(id) }
        id = e.start(DownloadRequest(url, "https://a.test/app", false, "notes.txt", server.response(url)))!!
        assertTrue("the engine's stream was used, nothing was fetched", server.requests.isEmpty())
        assertEquals("a blob cannot be paused", false, paused)
        val entry = list.get(id)!!
        assertEquals(DownloadState.COMPLETED, entry.state)
        assertEquals(url, entry.url)
        assertEquals("notes.txt", entry.fileName)
        assertEquals("hello blob", String(folder.files.getValue("notes.txt")))

        // One that breaks cannot be fetched again: no restart, only the page can.
        server.failAt = 4
        val broken = e.start(DownloadRequest(url, null, false, "notes.txt", server.response(url)))!!
        assertEquals(DownloadState.INTERRUPTED, state(broken))
        assertNull(e.restart(broken))
        assertNotNull(list.get(broken))
        assertTrue(server.requests.isEmpty())
    }

    @Test
    fun aDataAddressIsNotKeptInTheList() {
        val payload = "x".repeat(50_000)
        val url = "data:text/plain;base64,$payload"
        val server = FakeServer("decoded".toByteArray())
        server.contentType = "text/plain"
        server.status = 0 // not an HTTP response
        val e = engine(server)
        val id = e.start(DownloadRequest(url, "https://a.test/app", false, response = server.response(url)))!!
        val entry = list.get(id)!!
        assertEquals(DownloadState.COMPLETED, entry.state)
        assertEquals("data:text/plain,…", entry.url)
        assertEquals("download.txt", entry.fileName)
        assertEquals("decoded", String(folder.files.getValue("download.txt")))
        assertTrue(events.states.none { it.url.length > 100 })
    }

    @Test
    fun namesAreCleanedAndNeverCollide() {
        val server = FakeServer(bytes(10))
        server.disposition = "attachment; filename=\"../../evil\u202Egpj.exe\""
        val e = engine(server)
        val id = e.start(link())!!
        val name = list.get(id)!!.fileName
        assertFalse(name.contains('/') || name.contains('\u202E') || name.startsWith("."))
        assertTrue(name.endsWith(".exe"))

        server.disposition = "attachment; filename=\"a<b>.txt\""
        val ids = (1..3).map { e.start(link())!! }
        assertEquals(listOf("a_b_.txt", "a_b_ (1).txt", "a_b_ (2).txt"), ids.map { list.get(it)!!.fileName })
        assertEquals(4, folder.files.size)
        // Without a name of its own the address names the file; the type gives the extension.
        server.disposition = null
        server.contentType = "text/plain"
        assertEquals("readme.txt", list.get(e.start(link("https://a.test/dl/readme?x=1"))!!)!!.fileName)
        assertEquals("download.txt", list.get(e.start(link("https://a.test/"))!!)!!.fileName)
    }

    @Test
    fun aPrivateDownloadIsListedOnlyWhileItRunsAndLeavesNoRow() {
        val server = FakeServer(bytes(1000))
        val e = engine(server)
        var id = 0u
        var whileRunning: DownloadEntry? = null
        var listed = 0
        server.onServed = {
            if (it == 300) {
                whileRunning = list.unfinished().singleOrNull()
                listed = list.query("", DownloadFilter.ALL, 0u, 10u).items.size
            }
        }
        id = e.start(link(private = true))!!
        assertEquals(true, whileRunning?.private)
        assertEquals("shown while it runs (so it can be cancelled)", 1, listed)
        assertNull("the page of a private download is not kept", whileRunning?.pageUrl)
        assertEquals(listOf(true), server.privateFlags)

        assertNull("off the list the moment it is done", list.get(id))
        assertEquals(0, list.query("", DownloadFilter.ALL, 0u, 10u).items.size)
        assertArrayEquals("only the file remains", server.data, folder.files["file.bin"])
        assertTrue(parts().isEmpty())
        val last = events.last(id)
        assertEquals(DownloadState.COMPLETED, last.state)
        assertTrue(last.private)
        // Nothing was ever written: a new list on the same database knows of no download.
        val reloaded = DownloadList.load(store, now)
        assertEquals(0u, reloaded.query("", DownloadFilter.ALL, 0u, 10u).all)

        // A failed or cancelled one is dropped the same way.
        server.etag = null
        server.failAt = 200
        val failed = e.start(link(private = true))!!
        assertNull(list.get(failed))
        assertEquals(DownloadState.INTERRUPTED, events.last(failed).state)
        assertEquals(0u, DownloadList.load(store, now).query("", DownloadFilter.ALL, 0u, 10u).all)
    }

    @Test
    fun aFileThatCannotBeWrittenFails() {
        val server = FakeServer(bytes(1000))
        openPart = { _, _ -> throw IOException("disk full") }
        val id = engine(server).start(link())!!
        val entry = list.get(id)!!
        assertEquals(DownloadState.INTERRUPTED, entry.state)
        assertEquals("storage", entry.reason)
        assertTrue(server.bodies.single().closed)
    }

    @Test
    fun aFileThatCannotBePublishedKeepsItsPartAndRetryNeedsNoRequest() {
        val server = FakeServer(bytes(1000))
        folder.fail = true
        val e = engine(server)
        val id = e.start(link())!!
        val entry = list.get(id)!!
        assertEquals(DownloadState.PAUSED, entry.state)
        assertEquals("publish", entry.reason)
        assertEquals(1000L, parts().single().length())

        folder.fail = false
        assertTrue(e.resume(id))
        assertEquals(DownloadState.COMPLETED, state(id))
        assertEquals("the bytes were all there", 1, server.requests.size)
        assertArrayEquals(server.data, folder.files["file.bin"])
    }

    @Test
    fun androidsTimeLimitPausesWhatCanGoOnAndEndsTheRest() {
        val server = FakeServer(bytes(1000))
        val e = engine(server)
        var id = 0u
        server.onServed = { if (it == 300) e.timeLimit() }
        id = e.start(link())!!
        assertEquals(DownloadState.PAUSED, state(id))
        assertEquals("Paused by Android", list.get(id)!!.reason)

        server.etag = null
        val second = e.start(link())!!
        assertEquals(DownloadState.INTERRUPTED, state(second))
        assertEquals("Stopped by Android", list.get(second)!!.reason)
        assertEquals(1, parts().size)
    }

    @Test
    fun removeAndClearFinishedLeaveRunningDownloadsAlone() {
        val server = FakeServer(bytes(1000))
        val e = engine(server)
        var id = 0u
        var removedWhileRunning: Boolean? = null
        server.onServed = {
            if (it == 300) {
                removedWhileRunning = e.remove(id)
                e.clearFinished()
            }
        }
        id = e.start(link())!!
        assertEquals(false, removedWhileRunning)
        assertEquals(DownloadState.COMPLETED, state(id))
        assertTrue(e.remove(id))
        assertNull(list.get(id))
        assertNotNull("the file stays", folder.files["file.bin"])
    }

    private companion object {
        const val URL = "https://a.test/dl/file.bin"
        const val CHUNK = 100
    }
}
