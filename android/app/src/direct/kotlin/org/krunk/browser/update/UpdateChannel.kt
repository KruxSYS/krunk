package org.krunk.browser.update

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import android.text.format.Formatter
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.SystemUpdate
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import org.krunk.browser.BuildConfig
import org.krunk.browser.R

/**
 * The website APK's update channel (direct flavour). The play flavour has its own [UpdateChannel]
 * without any of this: no screen, no requests, no installer.
 */
object UpdateChannel {
    const val AVAILABLE = true

    /** Process start: removes a leftover update file. Never contacts the server. */
    fun onAppStart(context: Context) = UpdateController.onAppStart(context)

    /** The "Updates" part of Settings › About. */
    @Composable
    fun AboutSection() = UpdateSection()
}

private fun size(context: Context, bytes: Long) = Formatter.formatShortFileSize(context, bytes)

@Composable
private fun UpdateSection() {
    val context = LocalContext.current
    val state by UpdateController.state.collectAsState()
    var showNotes by remember { mutableStateOf(false) }
    val host = Uri.parse(BuildConfig.UPDATE_BASE_URL).host ?: BuildConfig.UPDATE_BASE_URL

    LifecycleResumeEffect(state is UpdateState.NeedsPermission) {
        // Back from Android's "install unknown apps" screen: go on if it was allowed.
        if (state is UpdateState.NeedsPermission && UpdateController.canInstall()) {
            UpdateController.permissionMaybeGranted()
        }
        onPauseOrDispose { }
    }

    HorizontalDivider(Modifier.padding(vertical = 8.dp))
    Text(
        stringResource(R.string.update_title),
        style = MaterialTheme.typography.titleSmall,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
    )
    val busy = state is UpdateState.Checking || state is UpdateState.Downloading || state is UpdateState.Installing
    ListItem(
        headlineContent = { Text(stringResource(R.string.update_check)) },
        supportingContent = { Text(stringResource(R.string.update_check_help)) },
        leadingContent = { Icon(Icons.Filled.SystemUpdate, contentDescription = null) },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        modifier = Modifier.fillMaxWidth().clickable(enabled = !busy) {
            UpdateController.reset()
            UpdateController.check(context)
        },
    )
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
        when (val s = state) {
            UpdateState.Idle -> Unit

            UpdateState.Checking -> {
                Text(stringResource(R.string.update_checking))
                Spacer(Modifier.height(4.dp))
                LinearProgressIndicator(Modifier.fillMaxWidth())
            }

            is UpdateState.UpToDate -> Text(stringResource(R.string.update_up_to_date, s.version))

            is UpdateState.NeedsNewerAndroid -> Text(
                stringResource(
                    R.string.update_needs_android,
                    s.update.version,
                    s.update.minSdk.toInt(),
                    android.os.Build.VERSION.SDK_INT,
                ),
            )

            is UpdateState.Available -> {
                Text(
                    stringResource(R.string.update_available, s.update.version),
                    style = MaterialTheme.typography.titleMedium,
                )
                Text(
                    stringResource(R.string.update_available_size, size(context, s.file.size.toLong()), s.file.abi),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                if (s.update.notes.isNotBlank()) {
                    TextButton(onClick = { showNotes = !showNotes }) { Text(stringResource(R.string.update_whats_new)) }
                    if (showNotes) {
                        Text(
                            UpdateRules.plainNotes(s.update.notes),
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                }
                Spacer(Modifier.height(8.dp))
                Button(onClick = {
                    UpdateController.download(context)
                }) { Text(stringResource(R.string.update_download)) }
            }

            is UpdateState.Downloading -> {
                val total = s.file.size.toLong()
                Text(
                    stringResource(
                        R.string.update_downloading,
                        s.update.version,
                        size(context, s.bytes),
                        size(context, total),
                    ),
                )
                Spacer(Modifier.height(4.dp))
                LinearProgressIndicator(
                    progress = { if (total > 0) (s.bytes.toFloat() / total).coerceIn(0f, 1f) else 0f },
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(4.dp))
                OutlinedButton(onClick = {
                    UpdateController.cancelDownload()
                }) { Text(stringResource(R.string.update_cancel)) }
            }

            is UpdateState.NeedsPermission -> {
                Text(stringResource(R.string.update_permission_waiting))
                OutlinedButton(onClick = {
                    UpdateController.declinePermission()
                }) { Text(stringResource(R.string.update_cancel)) }
                PermissionDialog(context)
            }

            is UpdateState.Installing -> Text(stringResource(R.string.update_installing, s.update.version))

            is UpdateState.Failed -> {
                Text(stringResource(messageOf(s.error)), color = MaterialTheme.colorScheme.error)
                if (s.detail.isNotBlank()) {
                    Text(
                        stringResource(R.string.update_detail, s.detail),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Row {
                    TextButton(onClick = { UpdateController.reset() }) { Text(stringResource(R.string.update_ok)) }
                    Spacer(Modifier.width(8.dp))
                    TextButton(onClick = {
                        UpdateController.reset()
                        UpdateController.check(context)
                    }) { Text(stringResource(R.string.update_try_again)) }
                }
            }
        }
        Spacer(Modifier.height(8.dp))
        Text(
            stringResource(R.string.update_help, host, BuildConfig.VERSION_NAME),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun PermissionDialog(context: Context) {
    var dismissed by remember { mutableStateOf(false) }
    if (dismissed) return
    AlertDialog(
        onDismissRequest = { dismissed = true },
        title = { Text(stringResource(R.string.update_permission_title)) },
        text = { Text(stringResource(R.string.update_permission_text)) },
        confirmButton = {
            TextButton(onClick = {
                dismissed = true
                val intent =
                    Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${context.packageName}"))
                runCatching { context.startActivity(intent) }
            }) { Text(stringResource(R.string.update_permission_open)) }
        },
        dismissButton = {
            TextButton(onClick = {
                dismissed = true
                UpdateController.declinePermission()
            }) { Text(stringResource(R.string.update_permission_later)) }
        },
    )
}

private fun messageOf(kind: UpdateErrorKind): Int = when (kind) {
    UpdateErrorKind.FETCH -> R.string.update_error_fetch
    UpdateErrorKind.SIGNATURE -> R.string.update_error_signature
    UpdateErrorKind.INVALID -> R.string.update_error_invalid
    UpdateErrorKind.NO_FILE -> R.string.update_error_no_file
    UpdateErrorKind.DOWNLOAD -> R.string.update_error_download
    UpdateErrorKind.HASH -> R.string.update_error_hash
    UpdateErrorKind.PACKAGE -> R.string.update_error_package
    UpdateErrorKind.CERTIFICATE -> R.string.update_error_certificate
    UpdateErrorKind.INSTALLER -> R.string.update_error_installer
    UpdateErrorKind.ABORTED -> R.string.update_error_aborted
}
