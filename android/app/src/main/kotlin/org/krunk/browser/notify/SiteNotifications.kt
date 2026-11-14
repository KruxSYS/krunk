package org.krunk.browser.notify

import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import org.krunk.browser.KrunkApp
import org.krunk.browser.MainActivity
import org.krunk.browser.R
import org.krunk.browser.engine.permissions.SiteLabel
import org.krunk.ffi.siteOfHost
import org.krunk.ffi.webOrigin
import org.mozilla.geckoview.WebNotification
import org.mozilla.geckoview.WebNotificationDelegate

/**
 * Notifications of sites (the web Notification API) as Android notifications in the "Site
 * notifications" channel. The title line is the site's own title; the site that sent it is
 * named in the header line Android draws itself, which a page cannot write to.
 *
 * - The same site and tag replace the earlier notification; the engine closing one cancels it.
 * - A tap tells the page (`click()`), brings Krunk to the front and selects a tab of that
 *   site, opening the site in a new tab when none is left. Swiping away tells the page
 *   (`dismiss()`).
 * - The engine's notification objects are kept in memory ([NotificationBook]), not in the
 *   intents: after the process died a tap on a notification that is still in the shade opens
 *   the site (its origin is in the intent) but the page's click handler is not called, because
 *   the engine that showed it is gone. A private tab's notification carries no origin, is
 *   cancelled when the private tabs close, and is cancelled at the next start if the process
 *   died first.
 * - A site's picture (`imageUrl`) is not fetched: it would be a request of Krunk's own.
 *
 * - At most [NotificationBook.MAX_PER_SITE] live notifications per site and
 *   [NotificationBook.MAX_LIVE] in all: the oldest gives way (cancelled, the page is told).
 *
 * Android's permission to post is asked when the user allows a site to notify (the prompt, or
 * Allow on the settings screen), never when a notification is due: then it is only checked
 * ([postable]); without it the notification is dropped and [dropped] tells the user (once).
 */
class SiteNotifications(
    private val context: Context,
    private val postable: () -> Boolean,
    private val dropped: () -> Unit,
) : WebNotificationDelegate {
    /** What a tap leads to: the site whose tab is to be selected. */
    data class Tap(val origin: String?, val private: Boolean)

    private val book = NotificationBook<WebNotification>(Channels.ID_SITE_BASE)

    /** Known to this process only: marks the tap intents it made itself. */
    private val token = java.util.UUID.randomUUID().toString()
    private val manager get() = context.getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager

    init {
        // What an earlier process left: private ones go (again: KrunkApp sweeps at start too),
        // the others keep their ids.
        book.reserve(sweepPrivate(context))
    }

    private fun originOf(n: WebNotification): String? = n.origin?.let(::webOrigin) ?: n.source?.let(::webOrigin)

    override fun onShowNotification(notification: WebNotification) {
        if (postable()) {
            show(notification)
        } else {
            notification.dismiss()
            dropped()
        }
    }

    private fun show(n: WebNotification) {
        val nm = manager ?: return n.dismiss()
        Channels.ensure(
            context,
            context.getString(R.string.channel_downloads),
            context.getString(R.string.channel_sites),
        )
        val origin = originOf(n)
        // The sender is named in a line the page cannot write; one without a web address (a
        // local file, an extension page) is named as such, so it never reads as Krunk's own.
        val site = origin?.let { SiteLabel.of(it, ::siteOfHost).text } ?: context.getString(R.string.notify_site_local)
        val shown = book.show(origin, n.tag, n.privateBrowsing, n)
        for (old in shown.evicted) {
            nm.cancel(old.id)
            old.notification.dismiss()
        }
        val id = shown.id
        val immutable = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        val tap = Intent(context, MainActivity::class.java)
            .setAction(ACTION_TAP)
            .putExtra(EXTRA_ID, id)
            .putExtra(EXTRA_TOKEN, token)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        // A private page's site is not written into anything that outlives the process.
        if (!n.privateBrowsing && origin != null) tap.putExtra(EXTRA_ORIGIN, origin)
        val swipe = Intent(
            context,
            SiteNotificationReceiver::class.java,
        ).setAction(ACTION_DISMISS).putExtra(EXTRA_ID, id)
        val title = n.title.orEmpty().bounded(MAX_TITLE).ifBlank { site }
        val text = n.text.orEmpty().bounded(MAX_TEXT)
        val builder = Notification.Builder(context, Channels.SITES)
            .setSmallIcon(R.drawable.ic_notify_site)
            .setContentTitle(title)
            .setContentText(text)
            .setStyle(Notification.BigTextStyle().bigText(text))
            .setCategory(Notification.CATEGORY_MESSAGE)
            .setAutoCancel(true)
            .setOnlyAlertOnce(false)
            .setContentIntent(PendingIntent.getActivity(context, id, tap, immutable))
            .setDeleteIntent(PendingIntent.getBroadcast(context, id, swipe, immutable))
            .setVisibility(if (n.privateBrowsing) Notification.VISIBILITY_SECRET else Notification.VISIBILITY_PRIVATE)
        builder.setSubText(site)
        if (n.privateBrowsing) builder.addExtras(android.os.Bundle().apply { putBoolean(EXTRA_PRIVATE, true) })
        // A silent notification makes no sound of its own; the channel's setting still rules.
        if (n.silent) builder.setGroupAlertBehavior(Notification.GROUP_ALERT_SUMMARY).setGroup(SILENT_GROUP)
        try {
            nm.notify(id, builder.build())
        } catch (e: SecurityException) {
            Log.w(KrunkApp.TAG, "site notification not posted: ${e.javaClass.simpleName}")
            book.remove(id)
            n.dismiss()
        }
    }

    override fun onCloseNotification(notification: WebNotification) {
        val entry =
            book.find(notification, originOf(notification), notification.tag, notification.privateBrowsing) ?: return
        book.remove(entry.id)
        manager?.cancel(entry.id)
    }

    /**
     * The notification [id] was tapped. Only an intent with this process's [token] reaches the
     * book (the activity is exported: another app can send the action, and must not be able to
     * click a notification); anything else, also a notification of an earlier process, is no
     * more than a request to show [originFromIntent].
     */
    fun tapped(id: Int, token: String?, originFromIntent: String?): Tap {
        val fallback = Tap(originFromIntent?.let(::webOrigin), private = false)
        if (token != this.token) return fallback
        book.release(id)
        val entry = book.remove(id) ?: return fallback
        entry.notification.click()
        return Tap(entry.origin, entry.private)
    }

    /** The notification [id] was swiped away. */
    fun dismissed(id: Int) {
        book.release(id)
        book.remove(id)?.notification?.dismiss()
    }

    /** The last private tab closed: nothing of a private page stays in the shade. */
    fun privateSessionEnded() {
        for (entry in book.removePrivate()) {
            manager?.cancel(entry.id)
            entry.notification.dismiss()
        }
    }

    companion object {
        const val ACTION_TAP = "org.krunk.browser.action.SITE_NOTIFICATION"
        const val ACTION_DISMISS = "org.krunk.browser.action.SITE_NOTIFICATION_DISMISSED"
        const val EXTRA_ID = "org.krunk.browser.extra.NOTIFICATION_ID"
        const val EXTRA_TOKEN = "org.krunk.browser.extra.NOTIFICATION_TOKEN"
        const val EXTRA_ORIGIN = "org.krunk.browser.extra.NOTIFICATION_ORIGIN"
        private const val EXTRA_PRIVATE = "org.krunk.browser.extra.NOTIFICATION_PRIVATE"
        private const val SILENT_GROUP = "org.krunk.browser.sites.silent"

        // What a site may put into a notification: bounded.
        private const val MAX_TITLE = 120
        private const val MAX_TEXT = 1000

        private fun String.bounded(max: Int): String = if (length > max) take(max) + "…" else this

        /**
         * Cancels the private site notifications an earlier process left in the shade (it died
         * before the private tabs closed) and returns the ids of the other site notifications
         * still there. Run at every start of Krunk's main process, before anything else of the
         * engine, and again when the notifications object is made.
         */
        fun sweepPrivate(context: Context): List<Int> {
            val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager
                ?: return emptyList()
            return try {
                val left = nm.activeNotifications.orEmpty().filter { it.id >= Channels.ID_SITE_BASE }
                val (private, normal) = left.partition { it.notification.extras.getBoolean(EXTRA_PRIVATE) }
                private.forEach { nm.cancel(it.tag, it.id) }
                normal.map { it.id }
            } catch (e: RuntimeException) {
                Log.w(KrunkApp.TAG, "site notifications: the shade could not be read: ${e.javaClass.simpleName}")
                emptyList()
            }
        }
    }
}

/** Swiping a site notification away: the page hears about it. Not exported. */
class SiteNotificationReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != SiteNotifications.ACTION_DISMISS) return
        val id = intent.getIntExtra(SiteNotifications.EXTRA_ID, -1)
        if (id < 0) return
        // Without a running engine there is no page to tell (and none is started for this).
        (context.applicationContext as? KrunkApp)?.siteNotificationsIfRunning?.dismissed(id)
    }
}
