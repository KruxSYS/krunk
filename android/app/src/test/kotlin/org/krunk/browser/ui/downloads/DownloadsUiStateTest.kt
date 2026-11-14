package org.krunk.browser.ui.downloads

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.ffi.DownloadEntry
import org.krunk.ffi.DownloadFilter
import org.krunk.ffi.DownloadList
import org.krunk.ffi.DownloadPage
import org.krunk.ffi.DownloadResume
import org.krunk.ffi.DownloadState
import org.krunk.ffi.KrunkStore

class DownloadsUiStateTest {
    private fun entry(
        state: DownloadState,
        url: String = "https://files.a.test/dl/big.iso",
        reason: String? = null,
        resumable: Boolean = false,
        received: Long = 250,
        total: Long? = 1000,
        private: Boolean = false,
        pageUrl: String? = null,
    ) = DownloadEntry(
        id = 7u,
        url = url,
        fileName = "big.iso",
        path = if (state ==
            DownloadState.COMPLETED
        ) {
            "content://media/external/downloads/7/big.iso"
        } else {
            "Download/big.iso"
        },
        state = state,
        received = received,
        total = total,
        started = 1,
        finished = null,
        reason = reason,
        `private` = private,
        pageUrl = pageUrl,
        speed = 100u,
        eta = 3725u,
        resume = if (resumable) DownloadResume("/p/1.part", 1000u, "\"v\"", null, null) else null,
    )

    @Test
    fun aRunningDownloadThatCanGoOnOffersPause() {
        val row = DownloadRow.of(entry(DownloadState.IN_PROGRESS, resumable = true))
        assertEquals(RowStatus.RUNNING, row.status)
        assertEquals(listOf(RowAction.PAUSE, RowAction.CANCEL), row.actions)
        assertNull(row.note)
        assertEquals(0.25f, row.fraction)
        assertEquals(100L, row.speed)
        assertEquals(TimeLeft(1, 2, 5), row.timeLeft)
        assertEquals("files.a.test", row.site)
        assertNull("a tap does nothing while it runs", row.primary)
    }

    @Test
    fun oneThatCannotGoOnOffersOnlyCancelAndSaysSo() {
        val row = DownloadRow.of(entry(DownloadState.IN_PROGRESS, url = "blob:https://a.test/1", total = null))
        assertEquals(listOf(RowAction.CANCEL), row.actions)
        assertEquals(RowNote.CANNOT_PAUSE, row.note)
        assertNull("no size, no bar position", row.fraction)
        assertEquals("a blob has no site of its own", "", row.site)
        assertEquals(
            "a.test",
            DownloadRow.of(
                entry(DownloadState.IN_PROGRESS, url = "blob:https://a.test/1", pageUrl = "https://a.test/app"),
            ).site,
        )
    }

    @Test
    fun pausedByTheUserReadsPausedAndAnErrorReadsFailedWithRetry() {
        var row = DownloadRow.of(entry(DownloadState.PAUSED, resumable = true))
        assertEquals(RowStatus.PAUSED, row.status)
        assertEquals(listOf(RowAction.RESUME, RowAction.CANCEL), row.actions)
        assertEquals(0L, row.speed)
        assertNull(row.timeLeft)

        row = DownloadRow.of(entry(DownloadState.PAUSED, resumable = true, reason = "Paused when Krunk closed"))
        assertEquals(RowStatus.PAUSED, row.status)
        assertEquals(RowAction.RESUME, row.actions.first())

        row = DownloadRow.of(entry(DownloadState.PAUSED, resumable = true, reason = "The connection was lost"))
        assertEquals(RowStatus.FAILED_CAN_RETRY, row.status)
        assertEquals(listOf(RowAction.RETRY, RowAction.CANCEL), row.actions)
        assertEquals("The connection was lost", row.reason)
    }

    @Test
    fun aFinishedDownloadOpensSharesAndCanBeRemoved() {
        val row = DownloadRow.of(entry(DownloadState.COMPLETED, received = 1000))
        assertEquals(RowStatus.DONE, row.status)
        assertEquals(RowAction.OPEN, row.primary)
        assertEquals(listOf(RowAction.OPEN, RowAction.SHARE, RowAction.REMOVE), row.actions)
        assertNull(row.fraction)
        assertEquals("content://media/external/downloads/7/big.iso", row.path)
    }

    @Test
    fun aFailedDownloadRestartsOnlyWhenItsAddressCanBeFetchedAgain() {
        var row = DownloadRow.of(entry(DownloadState.INTERRUPTED, reason = "The server cannot continue this download"))
        assertEquals(RowStatus.FAILED, row.status)
        assertEquals(listOf(RowAction.RESTART, RowAction.REMOVE), row.actions)
        assertEquals("The server cannot continue this download", row.reason)
        assertNull(row.note)

        for (url in listOf("blob:https://a.test/1", "data:text/plain,…")) {
            row = DownloadRow.of(entry(DownloadState.INTERRUPTED, url = url, reason = "Browser closed"))
            assertEquals(listOf(RowAction.REMOVE), row.actions)
            assertEquals(RowNote.AGAIN_FROM_PAGE, row.note)
        }

        row = DownloadRow.of(entry(DownloadState.CANCELLED))
        assertEquals(RowStatus.CANCELLED, row.status)
        assertEquals(listOf(RowAction.RESTART, RowAction.REMOVE), row.actions)
    }

    @Test
    fun privateDownloadsAreMarked() {
        assertTrue(DownloadRow.of(entry(DownloadState.IN_PROGRESS, private = true)).private)
        assertFalse(DownloadRow.of(entry(DownloadState.IN_PROGRESS)).private)
    }

    @Test
    fun timeLeft() {
        assertEquals(TimeLeft(0, 0, 45), DownloadRow.timeLeft(45))
        assertEquals(TimeLeft(0, 3, 0), DownloadRow.timeLeft(180))
        assertEquals(TimeLeft(2, 0, 1), DownloadRow.timeLeft(7201))
        assertEquals(TimeLeft(0, 0, 0), DownloadRow.timeLeft(-5))
    }

    private fun page(items: List<DownloadEntry>, done: UInt = 0u, failed: UInt = 0u) =
        DownloadPage(items, items.size.toUInt(), 0u, done, failed)

    @Test
    fun searchPagingAndEmptyStates() {
        var s = DownloadsUiState()
        assertNull("nothing is said before the first answer", s.empty)
        assertFalse(s.canLoadMore)
        s = s.answer("", page(emptyList()))
        assertEquals(EmptyKind.NO_DOWNLOADS, s.empty)
        assertFalse(s.canClearFinished)

        val full = List(DownloadsUiState.PAGE) { entry(DownloadState.COMPLETED) }
        s = s.answer("", page(full, done = 60u, failed = 2u))
        assertNull(s.empty)
        assertTrue(s.canClearFinished)
        assertTrue("a full page: more may follow", s.canLoadMore)
        s = s.more()
        assertEquals(2 * DownloadsUiState.PAGE, s.limit)
        s = s.answer("", page(full + full.take(10), done = 60u))
        assertFalse(s.canLoadMore)
        assertSame(s, s.more())

        // A new search starts at the first page; an answer to an older search is ignored.
        s = s.search("iso")
        assertEquals(DownloadsUiState.PAGE, s.limit)
        assertSame(s, s.answer("is", page(emptyList())))
        assertSame(s, s.search("iso"))
        s = s.answer("iso", page(emptyList(), done = 60u))
        assertEquals(EmptyKind.NO_MATCH, s.empty)
        assertTrue("clearing is about the whole list", s.canClearFinished)
    }

    @Test
    fun rowsFromTheRealList() {
        val list = DownloadList.load(KrunkStore.inMemory(), 1)
        val done = list.start("https://a.test/a.pdf", "Download/a.pdf", 10, 10, false, "https://a.test/", null)
        list.setPath(done, "content://media/external/downloads/3/a.pdf")
        list.setState(done, DownloadState.COMPLETED, null, 20)
        list.start(
            "https://b.test/b.zip",
            "Download/b.zip",
            1000,
            30,
            true,
            null,
            DownloadResume("/p/2.part", 1000u, "\"v\"", null, null),
        )
        val state = DownloadsUiState().answer("", list.query("", DownloadFilter.ALL, 0u, 50u))
        assertEquals(listOf("b.zip", "a.pdf"), state.rows.map { it.fileName })
        assertEquals(listOf(RowStatus.RUNNING, RowStatus.DONE), state.rows.map { it.status })
        assertTrue("the running private download is in the list, marked", state.rows[0].private)
        assertEquals(RowAction.PAUSE, state.rows[0].actions.first())
        assertTrue(state.canClearFinished)
        assertEquals(
            1,
            DownloadsUiState().search("pdf").answer("pdf", list.query("pdf", DownloadFilter.ALL, 0u, 50u)).rows.size,
        )
    }
}
