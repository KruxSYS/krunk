package org.krunk.browser.ui.downloads

import android.Manifest
import android.os.Build
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.krunk.browser.R
import org.krunk.browser.downloads.DownloadController
import org.krunk.browser.downloads.DownloadMessage
import org.krunk.browser.downloads.PendingDownload

/**
 * What downloads show over any screen: the question before a download a page started, Android's
 * notification question at the first download, and short messages. The state behind it lives in
 * the [DownloadController], so nothing is lost when a screen changes.
 */
@Composable
fun DownloadsHost(downloads: DownloadController) {
    val context = LocalContext.current

    val asked by downloads.questions.current.collectAsStateWithLifecycle()
    asked?.let { question ->
        DownloadQuestion(
            download = question,
            waiting = downloads.questions.waiting - 1,
            onDownload = { downloads.answer(question, accept = true) },
            onCancel = { downloads.answer(question, accept = false) },
        )
    }

    val askNotifications by downloads.askNotifications.collectAsStateWithLifecycle()
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        downloads.notificationsAnswered(granted)
    }
    LaunchedEffect(askNotifications) {
        if (askNotifications && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            permission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }

    LaunchedEffect(downloads) {
        downloads.messages.collect { message ->
            val text = when (message) {
                is DownloadMessage.Started -> context.getString(R.string.download_started, message.fileName)
                DownloadMessage.TooManyQuestions -> context.getString(R.string.downloads_too_many_questions)
                DownloadMessage.NotificationsOff -> context.getString(R.string.downloads_notifications_off)
            }
            val long = message !is DownloadMessage.Started
            Toast.makeText(context, text, if (long) Toast.LENGTH_LONG else Toast.LENGTH_SHORT).show()
        }
    }
}

/** Asks before a download a page started: the file name and the site it comes from. */
@Composable
private fun DownloadQuestion(download: PendingDownload, waiting: Int, onDownload: () -> Unit, onCancel: () -> Unit) {
    AlertDialog(
        onDismissRequest = onCancel,
        title = { Text(stringResource(R.string.download_confirm_title)) },
        text = {
            Column {
                Text(download.fileName, style = MaterialTheme.typography.bodyLarge)
                if (download.host.isNotEmpty()) {
                    Text(
                        stringResource(R.string.download_confirm_from, download.host),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(top = 4.dp),
                    )
                }
                if (download.private) {
                    Text(
                        stringResource(R.string.download_confirm_private),
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.padding(top = 12.dp),
                    )
                }
                if (waiting > 0) {
                    Text(
                        stringResource(R.string.download_confirm_more, waiting),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(top = 12.dp),
                    )
                }
            }
        },
        confirmButton = { TextButton(onClick = onDownload) { Text(stringResource(R.string.download_confirm_yes)) } },
        dismissButton = { TextButton(onClick = onCancel) { Text(stringResource(android.R.string.cancel)) } },
    )
}
