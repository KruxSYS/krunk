package org.krunk.browser.ui.browser

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.PersistableBundle
import android.util.Log
import org.krunk.browser.KrunkApp

/** Small intent helpers used by the menus. */
object Share {
    fun text(context: Context, text: String, subject: String? = null, chooserTitle: CharSequence? = null) {
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_TEXT, text)
            if (!subject.isNullOrBlank()) putExtra(Intent.EXTRA_SUBJECT, subject)
        }
        val chooser = Intent.createChooser(send, chooserTitle)
        runCatching { context.startActivity(chooser) }
            .onFailure { Log.w(KrunkApp.TAG, "share failed: ${it.message}") }
    }

    /**
     * Copies [text]; returns whether the caller should stay quiet (the system shows its own
     * "Copied", or nothing was copied). [sensitive] (a private tab) asks the system not to show
     * the text in its clipboard preview. Text too long for the clipboard is not copied (the
     * system would throw) instead of ending the app.
     */
    fun copy(context: Context, label: String, text: String, sensitive: Boolean = false): Boolean {
        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return true
        if (text.length > MAX_CLIP) {
            Log.w(KrunkApp.TAG, "not copied: ${text.length} characters are too many for the clipboard")
            return true
        }
        val clip = ClipData.newPlainText(label, text)
        if (sensitive) {
            clip.description.extras = PersistableBundle().apply { putBoolean(EXTRA_IS_SENSITIVE, true) }
        }
        return try {
            cm.setPrimaryClip(clip)
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
        } catch (e: RuntimeException) {
            Log.w(KrunkApp.TAG, "copy failed: ${e.message}")
            true
        }
    }

    /** Well below the binder transaction limit (1 MB, two bytes per character). */
    private const val MAX_CLIP = 100_000

    /** `ClipDescription.EXTRA_IS_SENSITIVE` (API 33); the key is honoured by older systems too. */
    private const val EXTRA_IS_SENSITIVE = "android.content.extra.IS_SENSITIVE"
}
