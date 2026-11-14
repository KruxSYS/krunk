package org.krunk.browser.ui.browser

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import org.krunk.browser.R
import org.krunk.browser.engine.PermissionPrompt
import org.krunk.browser.engine.permissions.AndroidDenied
import org.krunk.browser.engine.permissions.AndroidPermissions
import org.krunk.browser.engine.permissions.CameraChoice
import org.krunk.browser.engine.permissions.PermissionKind
import org.krunk.browser.engine.permissions.SitePermissions

/** What the site wants, as a line of the prompt. */
@Composable
private fun askLine(kind: PermissionKind, embeddedIn: String): String = when (kind) {
    PermissionKind.CAMERA -> stringResource(R.string.perm_ask_camera)

    PermissionKind.MICROPHONE -> stringResource(R.string.perm_ask_microphone)

    PermissionKind.GEOLOCATION -> stringResource(R.string.perm_ask_geolocation)

    PermissionKind.NOTIFICATIONS -> stringResource(R.string.perm_ask_notifications)

    PermissionKind.PERSISTENT_STORAGE -> stringResource(R.string.perm_ask_persistent_storage)

    PermissionKind.XR -> stringResource(R.string.perm_ask_xr)

    PermissionKind.AUTOPLAY_INAUDIBLE -> stringResource(R.string.perm_ask_autoplay_inaudible)

    PermissionKind.AUTOPLAY_AUDIBLE -> stringResource(R.string.perm_ask_autoplay_audible)

    PermissionKind.DRM -> stringResource(R.string.perm_ask_drm)

    PermissionKind.STORAGE_ACCESS -> if (embeddedIn.isEmpty()) {
        stringResource(R.string.perm_ask_storage_access_page)
    } else {
        stringResource(R.string.perm_ask_storage_access, embeddedIn)
    }

    PermissionKind.LOCAL_DEVICE -> stringResource(R.string.perm_ask_local_device)

    PermissionKind.LOCAL_NETWORK -> stringResource(R.string.perm_ask_local_network)
}

@Composable
private fun cameraLabel(index: Int, camera: CameraChoice): String = when (camera.facing) {
    CameraChoice.Facing.FRONT -> stringResource(R.string.perm_camera_front)
    CameraChoice.Facing.BACK -> stringResource(R.string.perm_camera_back)
    CameraChoice.Facing.OTHER -> camera.name.ifBlank { stringResource(R.string.perm_camera_other, index + 1) }
}

/**
 * A site's permission request. The title names the site that asks; when that is a frame of
 * another site (or a site asking for its cookies inside the page), the page's site is named
 * too. Allow and Deny; "Remember" is ticked from the start; Back or a tap outside is a deny
 * for this time that is not remembered.
 */
@Composable
fun PermissionPromptDialog(prompt: PermissionPrompt) {
    var remember by remember { mutableStateOf(true) }
    var camera by remember { mutableStateOf(prompt.cameras.firstOrNull()?.id) }
    val denyOnly = prompt.kinds.none { it.rememberAllow }
    AlertDialog(
        onDismissRequest = prompt::dismiss,
        title = {
            Text(
                if (prompt.site.isEmpty()) {
                    stringResource(R.string.perm_prompt_title_page)
                } else {
                    stringResource(R.string.perm_prompt_title, prompt.site)
                },
            )
        },
        text = {
            Column(Modifier.heightIn(max = 360.dp).verticalScroll(rememberScrollState())) {
                for (kind in prompt.kinds) {
                    Text(askLine(kind, prompt.embeddedIn), style = MaterialTheme.typography.bodyLarge)
                }
                if (prompt.embeddedIn.isNotEmpty() && prompt.site.isNotEmpty()) {
                    Text(
                        stringResource(R.string.perm_prompt_embedded, prompt.site, prompt.embeddedIn),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(top = 8.dp),
                    )
                }
                if (prompt.cameras.size > 1) {
                    Text(
                        stringResource(R.string.perm_prompt_camera_choice),
                        style = MaterialTheme.typography.labelLarge,
                        modifier = Modifier.padding(top = 12.dp),
                    )
                    Column(Modifier.selectableGroup()) {
                        prompt.cameras.forEachIndexed { index, choice ->
                            Row(
                                Modifier
                                    .fillMaxWidth()
                                    .heightIn(min = 48.dp)
                                    .selectable(selected = camera == choice.id, role = Role.RadioButton) {
                                        camera = choice.id
                                    },
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                RadioButton(selected = camera == choice.id, onClick = null)
                                Text(cameraLabel(index, choice), Modifier.padding(start = 12.dp))
                            }
                        }
                    }
                }
                if (prompt.canRemember) {
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .padding(top = 8.dp)
                            .heightIn(min = 48.dp)
                            .toggleable(value = remember, role = Role.Checkbox) { remember = it },
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Checkbox(checked = remember, onCheckedChange = null)
                        Text(
                            stringResource(
                                when {
                                    prompt.private -> R.string.perm_prompt_remember_private
                                    denyOnly -> R.string.perm_prompt_remember_deny_only
                                    else -> R.string.perm_prompt_remember
                                },
                            ),
                            Modifier.padding(start = 12.dp),
                        )
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = { prompt.answer(allow = true, remember = remember, cameraId = camera) }) {
                Text(stringResource(R.string.perm_allow))
            }
        },
        dismissButton = {
            TextButton(onClick = { prompt.answer(allow = false, remember = remember) }) {
                Text(stringResource(R.string.perm_deny))
            }
        },
    )
}

private fun groupName(group: AndroidPermissions.Group): Int = when (group) {
    AndroidPermissions.Group.CAMERA -> R.string.perm_android_camera
    AndroidPermissions.Group.MICROPHONE -> R.string.perm_android_microphone
    AndroidPermissions.Group.LOCATION -> R.string.perm_android_location
    AndroidPermissions.Group.NOTIFICATIONS -> R.string.perm_android_notifications
    AndroidPermissions.Group.OTHER -> R.string.perm_android_other
}

private fun openAppSettings(context: Context) {
    val intent = Intent(
        Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
        Uri.fromParts("package", context.packageName, null),
    )
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    try {
        context.startActivity(intent)
    } catch (_: ActivityNotFoundException) {
        // A device without that settings page: the message has said what is missing.
    }
}

/**
 * Says so when Android refused a permission a site needed: which one is missing, with a button
 * to Krunk's page in the system settings when Android no longer shows its own dialog.
 */
@Composable
fun PermissionMessages(permissions: SitePermissions?, snackbar: SnackbarHostState) {
    val context = LocalContext.current
    val res = LocalResources.current
    LaunchedEffect(permissions) {
        permissions?.messages?.collect { denied: AndroidDenied ->
            val names = denied.groups.map { res.getString(groupName(it)) }
            val what = names.reduceOrNull { a, b -> res.getString(R.string.perm_android_and, a, b) }
                ?: res.getString(R.string.perm_android_other)
            val result = snackbar.showSnackbar(
                res.getString(R.string.perm_android_denied, what),
                actionLabel = if (denied.openSettings) res.getString(R.string.perm_android_settings) else null,
                duration = SnackbarDuration.Long,
            )
            if (result == SnackbarResult.ActionPerformed) openAppSettings(context)
        }
    }
}
