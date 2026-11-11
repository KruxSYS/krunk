package org.krunk.browser.tabs

/**
 * Runs [onDone] exactly once: when the capture completes, or when [timeoutMs] passes first
 * (`GeckoView.capturePixels()` resolves with the next composited frame, which an emulator
 * without a working compositor never delivers). [schedule] posts a delayed action on the main
 * thread; it is a parameter so the gate can be tested without a Looper.
 */
class CaptureGate(timeoutMs: Long, schedule: (Long, () -> Unit) -> Unit, private val onDone: () -> Unit) {
    @Volatile
    var finished = false
        private set

    /** True when the timeout fired before [finish] was called. */
    @Volatile
    var timedOut = false
        private set

    init {
        schedule(timeoutMs) {
            if (!finished) timedOut = true
            finish()
        }
    }

    fun finish() {
        if (finished) return
        finished = true
        onDone()
    }
}
