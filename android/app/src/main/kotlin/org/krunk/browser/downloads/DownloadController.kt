package org.krunk.browser.downloads

import android.content.Intent
import android.os.SystemClock
import android.util.Log
import androidx.core.content.ContextCompat
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
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

    /** Too many "Download this file?" questions were waiting; some were dropped (said once per burst). */
    data object TooManyQuestions : DownloadMessage()

    /** Android's notification permission was refused: downloads run, the screen shows them. */
    data object NotificationsOff : DownloadMessage()

    /** Cancel came while the file was being saved into Downloads: it is there, and stays. */
    data class AlreadySaved(val fileName: String) : DownloadMessage()
}

/**
 * Everything about downloads that lives as long as the process: the questions that wait for
 * an answer, the engine that saves the streams, the list, the notifications and the foreground
 * service that runs while a download does. Created lazily by [KrunkApp]; screens only watch
 * its flows.
 *
 * Calls return at once: the work (the database, files) is done on one background thread, the
 * transfers on their own. Nothing on the work thread waits for a transfer.
 *
 * Private downloads: when the last private tab closes, their waiting questions are dropped and
 * those that wait (paused, or failed and able to go on) are cancelled (part deleted, entry and
 * notification gone). One that is still running may finish; its file is saved, but it leaves no
 * notification. At the start of the process, download notifications that belong to no
 * unfinished entry are removed (an earlier process's private file names among them).
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
    private val runningLock = Any()
    private val progressGate = Throttle(UPDATE_INTERVAL_MS, SystemClock::elapsedRealtime)
    private val gate = ServiceGate()

    /** Result notifications of private downloads: gone when the last private tab closes. */
    private val privateNotes: MutableSet<UInt> = Collections.synchronizedSet(HashSet())
    private val watching = AtomicBoolean(false)
    private var recovered = false

    /** Unfinished entries at the start of the process: the only ones a button of an earlier process may act on. */
    private val recoveredIds = HashSet<UInt>()

    /** Whether a private tab is open (the private session lasts while one is). */
    @Volatile private var privateOpen = false

    /** The open tabs; null until the tabs were first seen. */
    @Volatile private var openTabs: Set<UInt>? = null

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

    /**
     * Runs [block] on the work thread, after the start-up clean-up. Nothing in it may end the app;
     * when it fails, [onFail] runs (it releases what the call held).
     */
    private fun act(onFail: (() -> Unit)? = null, block: (DownloadEngine) -> Unit) {
        work.execute {
            try {
                if (!recovered) {
                    recovered = true
                    recoveredIds += list.unfinished().map { it.id }
                    // What an earlier process left in the notifications goes; the recovery posts
                    // the unfinished ones again, as they are now.
                    notifier.sweep(recoveredIds)
                    engine.recover()
                }
                block(engine)
            } catch (e: Exception) {
                Log.e(KrunkApp.TAG, "downloads: ${e.javaClass.simpleName}")
                onFail?.let { runCatching(it) }
            }
            bump()
        }
    }

    /** A button of a notification: one of an earlier process ([fresh] false) acts only on what that process knew. */
    private fun actOn(id: UInt, fresh: Boolean, block: (DownloadEngine) -> Unit) = act { engine ->
        if (fresh || id in recoveredIds) block(engine)
    }

    private fun bump() {
        _revision.value += 1
    }

    /**
     * At the app's start: tidies up after the last run (part files nobody owns, downloads that
     * were running, stale notifications), watches the tabs (questions of closed tabs, the end of
     * the private session) and Krunk coming to the foreground (the service is tried again).
     * Called on the main thread.
     */
    fun start(tabs: TabManager) {
        act { }
        if (!watching.compareAndSet(false, true)) return
        scope.launch {
            tabs.tabs.collect { all ->
                val open = all.map { it.id }.toSet()
                openTabs = open
                questions.keepTabs(open)
                val anyPrivate = all.any { it.private }
                val was = privateOpen
                privateOpen = anyPrivate
                if (was && !anyPrivate) endPrivateSession()
            }
        }
        ProcessLifecycleOwner.get().lifecycle.addObserver(
            object : DefaultLifecycleObserver {
                override fun onStart(owner: LifecycleOwner) {
                    // A start Android refused in the background may work now.
                    gate.unblock()
                    ensureService()
                }
            },
        )
    }

    /** The last private tab closed. */
    private fun endPrivateSession() {
        questions.dropPrivate()
        val ids = synchronized(privateNotes) { privateNotes.toList().also { privateNotes.clear() } }
        ids.forEach(notifier::cancel)
        act { engine -> engine.endPrivateSession().forEach(notifier::cancel) }
    }

    // ----- starting -----

    /**
     * A response the engine cannot show (a page in tab [tabId] started a download): asked first,
     * always, also when the response says the question could be skipped. Called on the main thread.
     */
    fun offer(private: Boolean, pageUrl: String?, response: WebResponse, tabId: UInt?) {
        val r = GeckoFetcher.response(response)
        val tabs = openTabs
        if (tabId != null && tabs != null && tabId !in tabs) {
            // Its tab closed before the answer of the page arrived.
            r.close()
            return
        }
        val name = DownloadNames.of(r.url, r.header("Content-Disposition"), r.contentType, DownloadOpener::extensionFor)
        val from = if (DownloadNames.isWeb(r.url)) r.url else pageUrl.orEmpty()
        val asked = PendingDownload(r, name, if (from.isEmpty()) "" else pillText(from), private, pageUrl, tabId)
        if (questions.offer(asked) == DownloadQuestions.Offer.DROPPED_TELL) {
            _messages.trySend(DownloadMessage.TooManyQuestions)
        }
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
        // The user starts a download: the service may be tried again.
        gate.unblock()
        // A response nobody took (the list could not be opened) is closed.
        act(onFail = { request.response?.close() }) {
            if (!notifier.allowed() && runCatching { app.core.store.kvGet(KV_NOTIFY_ASKED) }.getOrNull() == null) {
                _askNotifications.value = true
            }
            if (it.start(request) != null) _messages.trySend(DownloadMessage.Started(name))
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

    /** [fresh]: false for a notification button of an earlier process. */
    fun pause(id: UInt, fresh: Boolean = true) = actOn(id, fresh) { it.pause(id) }

    fun resume(id: UInt, fresh: Boolean = true) {
        gate.unblock()
        actOn(id, fresh) { it.resume(id) }
    }

    /**
     * Cancels at once, also a download whose thread sits in a stalled read. Too late while the
     * file is being saved into Downloads: then the user is told that it is saved (and it stays).
     */
    fun cancel(id: UInt, fresh: Boolean = true) = actOn(id, fresh) { engine ->
        when {
            engine.cancel(id) -> notifier.cancel(id)

            engine.isFinishing(id) || engine.entry(id)?.state == DownloadState.COMPLETED -> {
                engine.entry(id)?.let { _messages.trySend(DownloadMessage.AlreadySaved(it.fileName)) }
            }
        }
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

    /** Reads and publishes the running count as one step: two ends at once cannot leave a stale count. */
    private fun publishRunning(): Int = synchronized(runningLock) {
        engine.runningCount().also { _running.value = it }
    }

    override fun onState(entry: DownloadEntry) {
        val count = publishRunning()
        val final = entry.state != DownloadState.IN_PROGRESS && entry.state != DownloadState.PAUSED
        if (final) progressGate.forget(entry.id)
        synchronized(notifyLock) {
            // The newest state wins when two changes cross; a private entry that ended is
            // already off the list, and then this is its last word.
            val now = if (final) entry else engine.entry(entry.id) ?: entry
            if (entry.private && !privateOpen && now.state != DownloadState.IN_PROGRESS) {
                // The private session is over: nothing of it stays in the notifications, and
                // one that stopped where it could go on is cancelled like the others were.
                notifier.cancel(entry.id)
                if (now.state == DownloadState.PAUSED) act { it.cancel(entry.id) }
            } else {
                if (entry.private && final && now.state != DownloadState.CANCELLED) privateNotes += entry.id
                notifier.show(now)
            }
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

    // ----- the foreground service (rules: ServiceGate) -----

    private fun ensureService() {
        if (!gate.shouldStart(_running.value)) return
        try {
            ContextCompat.startForegroundService(app, Intent(app, DownloadService::class.java))
        } catch (e: RuntimeException) {
            // Android refuses a start from the background: the download goes on for as long as
            // the process lives; tried again at the next change and when Krunk is in the foreground.
            gate.startRefused()
            Log.w(KrunkApp.TAG, "downloads: no foreground service: ${e.javaClass.simpleName}")
        }
    }

    fun serviceNotification(count: Int) = notifier.service(count)

    /**
     * The service ended. When nothing was running that is all; a download that started in the
     * same moment gets a new one ([again]; not after Android's time limit or a refusal, which
     * stop what runs).
     */
    fun serviceStopped(again: Boolean) {
        gate.stopped()
        if (again) ensureService()
    }

    /**
     * The service could not become a foreground one: it cannot run, so the downloads are
     * treated as at Android's time limit (resumable ones paused, the rest interrupted), and
     * no new start is tried until Krunk is in the foreground or the user starts or resumes one.
     */
    fun serviceRefused() {
        gate.foregroundRefused()
        act { it.timeLimit() }
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
