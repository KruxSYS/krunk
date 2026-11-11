package org.krunk.browser.core

import java.io.ByteArrayInputStream
import java.io.InputStream
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class CappedReadTest {
    /** A server that never stops sending. */
    private class Endless : InputStream() {
        var served = 0L

        override fun read(): Int {
            served++
            return 'x'.code
        }

        override fun read(b: ByteArray, off: Int, len: Int): Int {
            served += len
            b.fill('x'.code.toByte(), off, off + len)
            return len
        }
    }

    @Test
    fun shortAnswersAreReadWhole() {
        val data = ByteArray(1000) { it.toByte() }
        assertArrayEquals(data, ByteArrayInputStream(data).readCapped(1000))
        assertArrayEquals(data, ByteArrayInputStream(data).readCapped(64 * 1024))
        assertEquals(0, ByteArrayInputStream(ByteArray(0)).readCapped(10)?.size)
    }

    @Test
    fun oneByteTooManyIsRefused() {
        assertNull(ByteArrayInputStream(ByteArray(1001)).readCapped(1000))
        assertNull(ByteArrayInputStream(ByteArray(100_000)).readCapped(64 * 1024))
    }

    @Test
    fun anEndlessAnswerStopsAtTheCap() {
        val stream = Endless()
        assertNull(stream.readCapped(512 * 1024))
        // It stopped right after the cap instead of reading until memory ran out.
        assertTrue("read ${stream.served} bytes", stream.served <= 512 * 1024 + 16 * 1024)
    }

    @Test
    fun aTrickleStopsAtTheTimeLimit() {
        var clock = 0L
        val slow = object : InputStream() {
            override fun read(): Int {
                clock += 4_000 // every byte takes four seconds
                return 'x'.code
            }
        }
        assertNull(slow.readCapped(maxBytes = 1_000_000, maxMillis = 15_000, nowMillis = { clock }))
        assertTrue(clock <= 15_000 + 8 * 1024 * 4_000L)
    }
}
