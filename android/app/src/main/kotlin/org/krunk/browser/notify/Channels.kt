package org.krunk.browser.notify

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context

/**
 * Krunk's notification channels and the id ranges of what is posted in them, in one place so
 * downloads and site notifications never share an id.
 */
object Channels {
    /** Download progress and results (low importance: no sound). */
    const val DOWNLOADS = "downloads"

    /** Notifications sites send (web notifications). */
    const val SITES = "sites"

    /** The foreground service's own notification while downloads run. */
    const val ID_DOWNLOAD_SERVICE = 1

    /** A download's notification is this plus its id. */
    const val ID_DOWNLOAD_BASE = 1_000

    /** A site notification's id is this plus a number Krunk counts up. */
    const val ID_SITE_BASE = 1_000_000

    /** Creates the channels (again is harmless: the system keeps the user's changes). */
    fun ensure(context: Context, downloadsName: String, sitesName: String) {
        val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager ?: return
        nm.createNotificationChannel(NotificationChannel(DOWNLOADS, downloadsName, NotificationManager.IMPORTANCE_LOW))
        nm.createNotificationChannel(NotificationChannel(SITES, sitesName, NotificationManager.IMPORTANCE_DEFAULT))
    }
}
