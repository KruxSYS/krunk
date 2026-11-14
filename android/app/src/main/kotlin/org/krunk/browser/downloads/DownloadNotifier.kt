package org.krunk.browser.downloads

import android.Manifest
import android.annotation.SuppressLint
import android.app.Notification
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.text.format.Formatter
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import java.util.Collections
import org.krunk.browser.KrunkApp
import org.krunk.browser.MainActivity
import org.krunk.browser.R
import org.krunk.browser.notify.Channels
import org.krunk.browser.ui.downloads.DownloadRow
import org.krunk.ffi.DownloadEntry
import org.krunk.ffi.DownloadState

/**
 * The notifications of downloads (channel [Channels.DOWNLOADS]): one per download while it
 * runs or is paused and for its result, and the foreground service's own.
 */
class DownloadNotifier(context: Context) {
    private val context = context.applicationContext
    private val manager = NotificationManagerCompat.from(this.context)

    /** Ids of downloads whose notification may be showing. */
    val posted: MutableSet<UInt> = Collections.synchronizedSet(HashSet())

    init {
        Channels.ensure(
            this.context,
            this.context.getString(R.string.channel_downloads),
            this.context.getString(R.string.channel_sites),
        )
    }

    /** Whether Android lets Krunk post at all (Android 13 and later ask the user). */
    fun allowed(): Boolean = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
        ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) ==
        PackageManager.PERMISSION_GRANTED

    /** Shows [entry] as it is now. */
    @SuppressLint("MissingPermission")
    fun show(entry: DownloadEntry) {
        if (entry.state == DownloadState.CANCELLED) {
            cancel(entry.id)
            return
        }
        if (!allowed()) return
        posted += entry.id
        manager.notify(Channels.ID_DOWNLOAD_BASE + entry.id.toInt(), build(entry))
    }

    fun cancel(id: UInt) {
        posted -= id
        manager.cancel(Channels.ID_DOWNLOAD_BASE + id.toInt())
    }

    /** The foreground service's notification while [count] downloads run. */
    fun service(count: Int): Notification = NotificationCompat.Builder(context, Channels.DOWNLOADS)
        .setSmallIcon(android.R.drawable.stat_sys_download)
        .setContentTitle(
            context.resources.getQuantityString(R.plurals.downloads_service_running, maxOf(count, 1), count),
        )
        .setContentIntent(screenIntent())
        .setOngoing(true)
        .setOnlyAlertOnce(true)
        .setCategory(NotificationCompat.CATEGORY_PROGRESS)
        .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
        .build()

    private fun build(e: DownloadEntry): Notification {
        val b = NotificationCompat.Builder(context, Channels.DOWNLOADS)
            .setContentTitle(e.fileName)
            .setOnlyAlertOnce(true)
            .setContentIntent(screenIntent())
            // A private download's name is not shown on the lock screen.
            .setVisibility(
                if (e.private) NotificationCompat.VISIBILITY_SECRET else NotificationCompat.VISIBILITY_PRIVATE,
            )
        val total = e.total?.takeIf { it > 0 }
        val amount = if (total != null) {
            context.getString(R.string.downloads_amount_of, size(e.received), size(total))
        } else {
            size(e.received)
        }
        when (e.state) {
            DownloadState.IN_PROGRESS -> {
                b.setSmallIcon(android.R.drawable.stat_sys_download)
                    .setOngoing(true)
                    .setCategory(NotificationCompat.CATEGORY_PROGRESS)
                    .setContentText(amount)
                    .setProgress(
                        PROGRESS_MAX,
                        if (total != null) (e.received * PROGRESS_MAX / total).toInt().coerceIn(0, PROGRESS_MAX) else 0,
                        total == null,
                    )
                if (e.resume != null) {
                    b.addAction(
                        0,
                        context.getString(R.string.downloads_pause),
                        action(DownloadActionReceiver.PAUSE, e.id),
                    )
                }
                b.addAction(
                    0,
                    context.getString(R.string.downloads_cancel),
                    action(DownloadActionReceiver.CANCEL, e.id),
                )
            }

            DownloadState.PAUSED -> {
                val onPurpose = DownloadRow.pausedOnPurpose(e.reason)
                b.setSmallIcon(
                    if (onPurpose) android.R.drawable.stat_sys_download_done else android.R.drawable.stat_notify_error,
                )
                    .setContentText(
                        if (onPurpose) {
                            context.getString(R.string.downloads_note_paused, amount)
                        } else {
                            context.getString(R.string.downloads_status_failed_reason, e.reason)
                        },
                    )
                    .addAction(
                        0,
                        context.getString(if (onPurpose) R.string.downloads_resume else R.string.downloads_retry),
                        action(DownloadActionReceiver.RESUME, e.id),
                    )
                    .addAction(
                        0,
                        context.getString(R.string.downloads_cancel),
                        action(DownloadActionReceiver.CANCEL, e.id),
                    )
            }

            DownloadState.COMPLETED -> {
                b.setSmallIcon(android.R.drawable.stat_sys_download_done)
                    .setContentText(context.getString(R.string.downloads_note_finished))
                    .setAutoCancel(true)
                DownloadOpener.viewIntent(context, e.path)?.let { view ->
                    b.setContentIntent(
                        PendingIntent.getActivity(
                            context,
                            e.id.toInt(),
                            view,
                            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                        ),
                    )
                }
            }

            DownloadState.INTERRUPTED, DownloadState.CANCELLED -> {
                b.setSmallIcon(android.R.drawable.stat_notify_error)
                    .setContentText(
                        e.reason?.takeIf { it.isNotBlank() }
                            ?.let { context.getString(R.string.downloads_status_failed_reason, it) }
                            ?: context.getString(R.string.downloads_status_failed),
                    )
                    .setAutoCancel(true)
            }
        }
        return b.build()
    }

    private fun size(bytes: Long): String = Formatter.formatShortFileSize(context, bytes.coerceAtLeast(0))

    /** A tap that opens Krunk's downloads screen. */
    private fun screenIntent(): PendingIntent = PendingIntent.getActivity(
        context,
        0,
        Intent(context, MainActivity::class.java)
            .setAction(DownloadIntents.ACTION_SHOW)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    private fun action(name: String, id: UInt): PendingIntent = PendingIntent.getBroadcast(
        context,
        0,
        Intent(context, DownloadActionReceiver::class.java)
            .setAction(name)
            // Makes the intents of different downloads different ones.
            .setData(Uri.parse("krunk-download:$id"))
            .putExtra(DownloadActionReceiver.EXTRA_ID, id.toLong()),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    private companion object {
        const val PROGRESS_MAX = 1000
    }
}

/** What reaches [MainActivity] from a download notification. */
object DownloadIntents {
    /** Opens the downloads screen. Harmless from anywhere: it carries nothing. */
    const val ACTION_SHOW = "org.krunk.browser.action.SHOW_DOWNLOADS"
}

/** The buttons of download notifications. Not exported: only Krunk's own pending intents reach it. */
class DownloadActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val id = intent.getLongExtra(EXTRA_ID, -1).takeIf { it in 0..UInt.MAX_VALUE.toLong() }?.toUInt() ?: return
        val downloads = (context.applicationContext as? KrunkApp)?.downloads ?: return
        when (intent.action) {
            PAUSE -> downloads.pause(id)
            RESUME -> downloads.resume(id)
            CANCEL -> downloads.cancel(id)
        }
    }

    companion object {
        const val PAUSE = "org.krunk.browser.downloads.PAUSE"
        const val RESUME = "org.krunk.browser.downloads.RESUME"
        const val CANCEL = "org.krunk.browser.downloads.CANCEL"
        const val EXTRA_ID = "id"
    }
}
