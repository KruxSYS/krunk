package org.krunk.browser.downloads

import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.ServiceCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import org.krunk.browser.KrunkApp
import org.krunk.browser.notify.Channels

/**
 * Keeps the process in the foreground while a download is being transferred (type `dataSync`),
 * and nothing else: the downloads themselves live in [DownloadController]. It is started when
 * the first transfer begins and stops itself when the last one ends.
 *
 * `startForeground` is always the first thing it does, also when nothing runs any more (it then
 * stops at once): a service started with `startForegroundService` never ends without having
 * called it. When Android refuses it (e.g. Android 15's daily `dataSync` time limit is used
 * up) the service cannot run: the downloads are treated as at the time limit ([onTimeout]:
 * resumable ones paused, the rest interrupted) and no new start is tried until Krunk is in the
 * foreground again or the user starts or resumes a download (see [ServiceGate]).
 */
class DownloadService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var watch: Job? = null
    private var timedOut = false
    private val downloads: DownloadController get() = (application as KrunkApp).downloads

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val downloads = downloads
        // Always first: a service started as a foreground service must become one.
        if (!foreground(downloads, downloads.running.value)) {
            timedOut = true
            downloads.serviceRefused()
            stopSelf()
            return START_NOT_STICKY
        }
        if (watch == null) {
            watch = scope.launch {
                downloads.running.collect { count ->
                    if (count == 0) stopSelf() else foreground(downloads, count)
                }
            }
        }
        return START_NOT_STICKY
    }

    private fun foreground(downloads: DownloadController, count: Int): Boolean = try {
        ServiceCompat.startForeground(
            this,
            Channels.ID_DOWNLOAD_SERVICE,
            downloads.serviceNotification(count),
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC else 0,
        )
        true
    } catch (e: RuntimeException) {
        Log.w(KrunkApp.TAG, "downloads: not in the foreground: ${e.javaClass.simpleName}")
        false
    }

    /**
     * Android 15: a `dataSync` service may run six hours in a day. What can go on later is
     * paused, the rest ends with that reason; the service has to stop within seconds.
     */
    override fun onTimeout(startId: Int, fgsType: Int) {
        timedOut = true
        downloads.serviceTimeLimit()
        stopSelf()
    }

    override fun onDestroy() {
        scope.cancel()
        downloads.serviceStopped(again = !timedOut)
        super.onDestroy()
    }
}
