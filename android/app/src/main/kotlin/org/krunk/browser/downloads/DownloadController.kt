package org.krunk.browser.downloads

import android.content.Intent
import android.os.SystemClock
import android.util.Log
import androidx.core.content.ContextCompat
import java.io.File
import java.util.Collections
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.krunk.browser.KrunkApp
import org.krunk.browser.R
import org.krunk.browser.tabs.TabManager
import org.krunk.browser.ui.browser.pillText
import org.krunk.ffi.DownloadEntry
import org.krunk.ffi.DownloadFilter
import org.krunk.ffi.DownloadList
import org.krunk.ffi.DownloadPage
import org.krunk.ffi.DownloadState
import org.mozilla.geckoview.WebResponse

/** Something to tell the user once (shown as a short message by whatever screen is up). */
sealed class DownloadMessage {
    data class Started(val fileName: String) : DownloadMessage()

    /** Too many "Download this file?" questions were waiting; this one was dropped. */
    data object TooManyQuestions : DownloadMessage()

    /** Android's notification permission was refused: downloads run, the screen shows them. */
    data object NotificationsOff : DownloadMessage()
}

/**
 * Everything about downloads that lives as long as the process: the questions that wait for
 * an answer, the engine that saves the streams, the list, the notifications and the foreground
 * service that runs while a download does. Created lazily by [KrunkApp]; screens only watch
 * its flows.
 *
 * Calls return at once: the work (the database, files) is done on one background thread, the
 * transfers on their own.
 */
class DownloadController(private val app: KrunkApp) : DownloadEngine.Listener {
    /** The "Download this file?" questions; survives any screen. */
    val questions = DownloadQuestions()

    private val _revision = MutableStateFlow(0L)

    /** Counts up whenever the list may read differently (state changes, progress about twice a second). */
    val revision: StateFlow<Long> = _revision.asStateFlow()

    private val _running = MutableStateFlow(0)

    /** Downloads being transferred right now (the foreground service runs while this is above 0). */
    val running: StateFlow<Int> = _running.asStateFlow()

    private val _askNotifications = MutableStateFlow(false)

    /** True when the screen should ask for Android's notification permission (once, at the first download). */
    val askNotifications: StateFlow<Boolean> = _askNotifications.asStateFlow()

    private val _messages = Channel<DownloadMessage>(capacity = 8, onBufferOverflow = BufferOverflow.DROP_OLDEST)
    val messages: Flow<DownloadMessage> = _messages.receiveAsFlow()

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val transfers = Executors.newCachedThreadPool { r -> Thread(r, "krunk-download").apply { isDaemon = true } }
    private val work = Executors.newSingleThreadExecutor { r -> Thread(r, "krunk-downloads").apply { isDaemon = true } }
    private val notifier by lazy { DownloadNotifier(app) }
    private val notifyLock = Any()
    private val progressGate = Throttle(UPDATE_INTERVAL_MS, SystemClock::elapsedRealtime)

    /** Result notifications of private downloads: gone when the last private tab closes. */
    private val privateNotes: MutableSet<UInt> = Collections.synchronizedSet(HashSet())
    private val watching = AtomicBoolean(false)
    private var recovered = false

    @Volatile private var serviceUp = false

    // Opening the list reads the database: both are only touched from background threads.
    private val list: DownloadList by lazy { DownloadList.load(app.core.store, System.currentTimeMillis()) }
    private val engine: DownloadEngine by lazy {
        DownloadEngine(
            list = list,
            partDir = File(app.filesDir, DownloadEngine.PART_DIR),
            fetcher = GeckoFetcher { app.engine.runtime },
            publisher = DownloadOpener.publisher(app),
            reasons = DownloadReasons(
                network = app.getString(R.string.downloads_reason_network),
                timedOut = app.getString(R.string.downloads_reason_timed_out),
                endedEarly = app.getString(R.string.downloads_reason_ended_early),
                serverError = app.getString(R.string.downloads_reason_server_error),
                cannotContinue = app.getString(R.string.downloads_reason_cannot_continue),
                partGone = app.getString(R.string.downloads_reason_part_gone),
                storage = app.getString(R.string.downloads_reason_storage),
                publish = app.getString(R.string.downloads_reason_publish),
                timeLimit = app.getString(R.string.downloads_reason_time_limit),
                timeLimitStopped = app.getString(R.string.downloads_reason_time_limit_stopped),
            ),
            clock = System::currentTimeMillis,
            executor = transfers,
            listener = this,
            extensionFor = DownloadOpener::extensionFor,
        )
    }

    /** Runs [block] on the work thread, after the start-up clean-up. Nothing in it may end the app. */
    private fun act(block: (DownloadEngine) -> Unit) {
        work.execute {
            try {
                if (!recovered) {
                    recovered = true
                    engine.recover()
                }
                block(engine)
            } catch (e: Exception) {
                Log.e(KrunkApp.TAG, "downloads: ${e.javaClass.simpleName}")
            }
            bump()
        }
    }

    private fun bump() {
        _revision.value += 1
    }

    /**
     * At the app's start: tidies up after the last run (part files nobody owns, downloads that
     * were running) and watches the tabs for the last private one closing.
     */
    fun start(tabs: TabManager) {
        act { }
        if (!watching.compareAndSet(false, true)) return
        scope.launch {
            tabs.tabs.map { list -> list.any { it.private } }.distinctUntilChanged().collect { anyPrivate ->
                if (!anyPrivate) forgetPrivateNotes()
            }
        }
    }

    private fun forgetPrivateNotes() {
        val ids = synchronized(privateNotes) { privateNotes.toList().also { privateNotes.clear() } }
        ids.forEach(notifier::cancel)
    }

    // ----- starting -----

    /**
     * A response the engine cannot show (a page started a download): asked first, always, also
     * when the response says the question could be skipped. Called on the main thread.
     */
    fun offer(private: Boolean, pageUrl: String?, response: WebResponse) {
        val r = GeckoFetcher.response(response)
        val name = DownloadNames.of(r.url, r.header("Content-Disposition"), r.contentType, DownloadOpener::extensionFor)
        val from = if (DownloadNames.isWeb(r.url)) r.url else pageUrl.orEmpty()
        val asked = PendingDownload(r, name, if (from.isEmpty()) "" else pillText(from), private, pageUrl)
        if (!questions.offer(asked)) _messages.trySend(DownloadMessage.TooManyQuestions)
    }

    /** The user's answer to a question. */
    fun answer(download: PendingDownload, accept: Boolean) {
        val go = questions.answer(download, accept) ?: return
        begin(DownloadRequest(go.response.url, go.pageUrl, go.private, go.fileName, go.response), go.fileName)
    }

    /** "Download link", "Save image", "Save media": fetched through the engine, with the tab's cookies. */
    fun download(url: String, pageUrl: String?, private: Boolean): String? {
        if (!DownloadNames.isWeb(url)) return null
        val name = DownloadNames.of(url, null, null, DownloadOpener::extensionFor)
        begin(DownloadRequest(url, pageUrl, private), name)
        return name
    }

    private fun begin(request: DownloadRequest, name: String) {
        _messages.trySend(DownloadMessage.Started(name))
        act {
            if (!notifier.allowed() && runCatching { app.core.store.kvGet(KV_NOTIFY_ASKED) }.getOrNull() == null) {
                _askNotifications.value = true
            }
            it.start(request)
        }
    }

    /** The answer to Android's notification question (asked once). */
    fun notificationsAnswered(granted: Boolean) {
        _askNotifications.value = false
        act {
            runCatching { app.core.store.kvSet(KV_NOTIFY_ASKED, "1") }
            if (granted) {
                // What is running gets its notification now.
                list.unfinished().forEach { e -> if (e.state == DownloadState.IN_PROGRESS) onState(e) }
            } else {
                _messages.trySend(DownloadMessage.NotificationsOff)
            }
        }
    }

    // ----- the list -----

    fun pause(id: UInt) = act { it.pause(id) }

    fun resume(id: UInt) = act { it.resume(id) }

    fun cancel(id: UInt) = act {
        it.cancel(id)
        notifier.cancel(id)
    }

    fun restart(id: UInt) = act {
        if (it.restart(id) != null) notifier.cancel(id)
    }

    fun remove(id: UInt) = act {
        if (it.remove(id)) notifier.cancel(id)
    }

    fun clearFinished() = act { engine ->
        engine.clearFinished()
        val gone = synchronized(notifier.posted) { notifier.posted.toList() }
            .filter { it !in privateNotes && engine.entry(it) == null }
        gone.forEach(notifier::cancel)
    }

    /** A page of the list, newest first; running downloads (private ones too) are part of it. */
    suspend fun query(text: String, limit: Int): DownloadPage? = withContext(Dispatchers.IO) {
        try {
            list.query(text.trim(), DownloadFilter.ALL, 0u, limit.toUInt())
        } catch (e: Exception) {
            Log.e(KrunkApp.TAG, "downloads: list not read: ${e.javaClass.simpleName}")
            null
        }
    }

    /** For the debug hooks: the newest entries, what runs and the part files, as text. */
    suspend fun describe(): String = withContext(Dispatchers.IO) {
        val page = runCatching { list.query("", DownloadFilter.ALL, 0u, 20u) }.getOrNull()
        val parts = File(app.filesDir, DownloadEngine.PART_DIR).listFiles().orEmpty()
        val lines = page?.items.orEmpty().map { e ->
            "#${e.id} ${e.state} ${e.received}/${e.total} '${e.fileName}' reason=${e.reason} private=${e.private} " +
                "resumable=${e.resume != null} path=${e.path}"
        }
        "downloads all=${page?.all} running=${_running.value} questions=${questions.waiting} " +
            "parts=${parts.map { "${it.name}:${it.length()}" }}\n" + lines.joinToString("\n")
    }

    // ----- the engine's events (any thread) -----

    override fun onState(entry: DownloadEntry) {
        val count = engine.runningCount()
        _running.value = count
        val final = entry.state != DownloadState.IN_PROGRESS && entry.state != DownloadState.PAUSED
        if (final) progressGate.forget(entry.id)
        synchronized(notifyLock) {
            // The newest state wins when two changes cross; a private entry that ended is
            // already off the list, and then this is its last word.
            val now = if (final) entry else engine.entry(entry.id) ?: entry
            if (entry.private && final && now.state != DownloadState.CANCELLED) privateNotes += entry.id
            notifier.show(now)
        }
        if (count > 0) ensureService()
        bump()
    }

    override fun onProgress(id: UInt) {
        if (progressGate.pass(id)) {
            synchronized(notifyLock) {
                val e = engine.entry(id)
                if (e != null && e.state == DownloadState.IN_PROGRESS) notifier.show(e)
            }
        }
        // However many run, the list is read again about twice a second.
        if (progressGate.pass(LIST)) bump()
    }

    // ----- the foreground service -----

    private fun ensureService() {
        if (serviceUp) return
        serviceUp = true
        try {
            ContextCompat.startForegroundService(app, Intent(app, DownloadService::class.java))
        } catch (e: RuntimeException) {
            // Android refuses a start from the background (or after its daily time limit): the
            // download goes on for as long as the process lives.
            serviceUp = false
            Log.w(KrunkApp.TAG, "downloads: no foreground service: ${e.javaClass.simpleName}")
        }
    }

    fun serviceNotification(count: Int) = notifier.service(count)

    /**
     * The service ended. When nothing was running that is all; a download that started in the
     * same moment gets a new one ([again]; not after Android's time limit, which stops what runs).
     */
    fun serviceStopped(again: Boolean) {
        serviceUp = false
        if (again && _running.value > 0) ensureService()
    }

    /** Android's time limit for the service: what can go on is paused, the rest ends. */
    fun serviceTimeLimit() = act { it.timeLimit() }

    companion object {
        /** Set once Android's notification question was answered. */
        const val KV_NOTIFY_ASKED = "downloads.notify_asked"
        private const val UPDATE_INTERVAL_MS = 500L
        private const val LIST = "list"
    }
}
