package org.krunk.browser.downloads

import java.io.ByteArrayInputStream
import java.io.File
import java.io.InputStream
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

/** The small parts around the engine: the waiting questions, names, paths, the throttle, backup rules. */
class DownloadPartsTest {
    private class Body : InputStream() {
        var closed = false

        override fun read(): Int = -1

        override fun close() {
            closed = true
        }
    }

    private fun pending(body: InputStream, name: String = "a.bin") =
        PendingDownload(DownloadResponse("https://a.test/$name", 200, emptyMap(), body), name, "a.test", false, null)

    @Test
    fun questionsAreAskedOneAtATimeInOrder() {
        val q = DownloadQuestions()
        val first = pending(Body(), "1")
        val second = pending(Body(), "2")
        assertTrue(q.offer(first))
        assertTrue(q.offer(second))
        assertSame(first, q.current.value)
        assertEquals(2, q.waiting)
        assertSame("yes hands the open response on", first, q.answer(first, accept = true))
        assertFalse((first.response.body as Body).closed)
        assertSame(second, q.current.value)
        assertNull(q.answer(second, accept = false))
        assertTrue("no closes it", (second.response.body as Body).closed)
        assertNull(q.current.value)
        assertNull("an answer comes once", q.answer(first, accept = true))
    }

    @Test
    fun aPageCannotPileUpOpenResponses() {
        val q = DownloadQuestions(max = 2)
        val bodies = List(5) { Body() }
        val taken = bodies.map { q.offer(pending(it)) }
        assertEquals(listOf(true, true, false, false, false), taken)
        assertEquals(listOf(false, false, true, true, true), bodies.map { it.closed })
        assertEquals(2, q.waiting)
        q.clear()
        assertTrue(bodies.all { it.closed })
        assertNull(q.current.value)
    }

    @Test
    fun names() {
        val ext: (String) -> String? = { if (it == "application/pdf") "pdf" else null }
        assertEquals("report.pdf", DownloadNames.of("https://a.test/x", "attachment; filename=report.pdf", null, ext))
        assertEquals("x.pdf", DownloadNames.of("https://a.test/x", null, "application/pdf", ext))
        assertEquals("x", DownloadNames.of("https://a.test/x", null, "application/unknown", ext))
        assertEquals(
            "passwd",
            DownloadNames.of("https://a.test/x", "attachment; filename=\"/etc/passwd\"", null, ext).takeLast(6),
        )
        // A right-to-left override would make "photo[RLO]gpj.exe" read as "photoexe.jpg".
        assertEquals("photogpj.exe", DownloadNames.of("https://a.test/photo%E2%80%AEgpj.exe", null, null, ext))
        assertEquals(
            "a_b__c_.txt",
            DownloadNames.of("https://a.test/x", "attachment; filename=\"a<b>:c?.txt\"", null, ext),
        )
        assertEquals("download.pdf", DownloadNames.of("blob:https://a.test/8a2f-11", null, "application/pdf", ext))
        assertEquals("download", DownloadNames.of("data:text/plain,hello/there.exe", null, null, ext))
        assertTrue(DownloadNames.of("https://a.test/" + "n".repeat(400) + ".tar.gz", null, null, ext).length <= 200)
    }

    @Test
    fun addressesInTheList() {
        assertEquals("https://a.test/f?q=1", DownloadNames.listUrl("https://a.test/f?q=1"))
        assertEquals("blob:https://a.test/1", DownloadNames.listUrl("blob:https://a.test/1"))
        assertEquals("data:image/png,…", DownloadNames.listUrl("data:image/png;base64," + "A".repeat(100_000)))
        assertEquals("data:,…", DownloadNames.listUrl("DATA:,hello"))
        assertTrue(DownloadNames.listUrl("data:" + "m".repeat(5000)).length < 100)
        assertTrue(DownloadNames.isWeb("HTTPS://a.test/"))
        assertTrue(DownloadNames.isWeb("http://a.test/"))
        assertFalse(DownloadNames.isWeb("blob:https://a.test/1"))
        assertFalse(DownloadNames.isWeb("data:text/plain,x"))
        assertFalse(DownloadNames.isWeb("file:///sdcard/x"))
        assertFalse(DownloadNames.isWeb(null))
    }

    @Test
    fun publishedPaths() {
        val path = DownloadPaths.join("content://media/external/downloads/7", "f (1).bin")
        assertEquals("content://media/external/downloads/7/f (1).bin", path)
        val back = DownloadPaths.split(path)!!
        assertEquals("content://media/external/downloads/7", back.uri)
        assertEquals("f (1).bin", back.fileName)
        assertNull("an unfinished entry has no file", DownloadPaths.split("Download/f.bin"))
        assertNull(DownloadPaths.split("file:///sdcard/Download/f.bin"))
        assertNull(DownloadPaths.split("content://x/"))
        assertNull(DownloadPaths.split(""))
    }

    @Test
    fun throttle() {
        var now = 0L
        val t = Throttle(500) { now }
        assertTrue(t.pass(1u))
        assertFalse(t.pass(1u))
        assertTrue("each key on its own", t.pass(2u))
        now = 499
        assertFalse(t.pass(1u))
        now = 500
        assertTrue(t.pass(1u))
        t.forget(1u)
        assertTrue(t.pass(1u))
        now = 100 // a clock that went back does not block for ever
        assertTrue(t.pass(1u))
    }

    @Test
    fun partFilesAreNeverBackedUpOrTransferred() {
        // Gradle runs the tests in the module directory.
        val rule = """<exclude domain="file" path="${DownloadEngine.PART_DIR}" />"""
        val backup = File("src/main/res/xml/backup_rules.xml").readText()
        val extraction = File("src/main/res/xml/data_extraction_rules.xml").readText()
        assertEquals(1, backup.split(rule).size - 1)
        assertFalse(backup.contains("<include"))
        val cloud = extraction.substringAfter("<cloud-backup>").substringBefore("</cloud-backup>")
        assertEquals(1, cloud.split(rule).size - 1)
        assertFalse(cloud.contains("<include"))
        // A device transfer carries one directory only, and the part files are not in it.
        val transfer = extraction.substringAfter("<device-transfer>").substringBefore("</device-transfer>")
        assertEquals(
            listOf("""<include domain="file" path="krunk" />"""),
            Regex("<include[^>]*>").findAll(transfer).map {
                it.value
            }.toList(),
        )
        assertFalse(DownloadEngine.PART_DIR.startsWith("krunk/"))
        assertEquals("downloads-part", DownloadEngine.PART_DIR)
    }

    @Test
    fun responseHeadersAreFoundWithoutCase() {
        val r = DownloadResponse(
            "https://a.test/f",
            200,
            mapOf("content-type" to "Text/HTML; charset=utf-8", "X" to "1"),
            ByteArrayInputStream(ByteArray(0)),
        )
        assertEquals("text/html", r.contentType)
        assertEquals("1", r.header("x"))
        assertNull(r.header("y"))
    }
}
