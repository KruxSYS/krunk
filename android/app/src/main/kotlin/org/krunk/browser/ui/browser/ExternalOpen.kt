package org.krunk.browser.ui.browser

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import org.krunk.browser.R
import org.krunk.browser.engine.ExternalKind
import org.krunk.browser.engine.ExternalLink

/** Hands a mail or phone link to Android. The dialer opens with the number; nothing is called. */
object ExternalOpen {
    /** Returns false when no app takes it. */
    fun launch(context: Context, link: ExternalLink): Boolean {
        val action = when (link.kind) {
            ExternalKind.MAIL -> Intent.ACTION_SENDTO
            ExternalKind.PHONE -> Intent.ACTION_DIAL
        }
        val intent = Intent(action, Uri.parse(link.uri)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        return try {
            context.startActivity(intent)
            true
        } catch (e: ActivityNotFoundException) {
            false
        } catch (e: SecurityException) {
            false
        }
    }
}

/** A page asked for a mail or phone link without a tap: the user decides. */
@Composable
fun ExternalLinkDialog(link: ExternalLink, onOpen: () -> Unit, onCancel: () -> Unit) {
    val title = when (link.kind) {
        ExternalKind.MAIL -> R.string.external_mail_title
        ExternalKind.PHONE -> R.string.external_phone_title
    }
    AlertDialog(
        onDismissRequest = onCancel,
        title = { Text(stringResource(title)) },
        text = { Text(stringResource(R.string.external_text, link.shown)) },
        confirmButton = { TextButton(onClick = onOpen) { Text(stringResource(R.string.external_open)) } },
        dismissButton = { TextButton(onClick = onCancel) { Text(stringResource(android.R.string.cancel)) } },
    )
}
