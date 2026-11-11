package org.krunk.browser.core

import java.io.ByteArrayOutputStream
import java.io.InputStream

/**
 * Reads a response Krunk fetches itself (a favicon, search suggestions) without trusting its
 * size: at most [maxBytes], and for at most [maxMillis] in total. Returns null when the stream
 * holds more or takes longer; the rest is then left unread, so an endless or highly compressed
 * answer costs a bounded amount of memory and time.
 */
fun InputStream.readCapped(
    maxBytes: Int,
    maxMillis: Long = 15_000,
    nowMillis: () -> Long = { System.nanoTime() / 1_000_000 },
): ByteArray? {
    val deadline = nowMillis() + maxMillis
    val out = ByteArrayOutputStream(minOf(maxBytes, 16 * 1024))
    val buffer = ByteArray(8 * 1024)
    while (true) {
        val n = read(buffer)
        if (n < 0) return out.toByteArray()
        if (out.size() + n > maxBytes || nowMillis() > deadline) return null
        out.write(buffer, 0, n)
    }
}
