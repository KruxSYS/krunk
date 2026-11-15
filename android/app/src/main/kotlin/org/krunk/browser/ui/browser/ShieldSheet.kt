package org.krunk.browser.ui.browser

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Shield
import androidx.compose.material.icons.outlined.GppMaybe
import androidx.compose.material.icons.outlined.RemoveModerator
import androidx.compose.material.icons.outlined.Shield
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.krunk.browser.R
import org.krunk.browser.ui.theme.KrunkSwitch

/** The icon of a shield mode; each mode has its own, so the states differ without colour. */
private fun shieldIcon(mode: ShieldMode): ImageVector = when (mode) {
    ShieldMode.ACTIVE -> Icons.Filled.Shield
    ShieldMode.ALLOWED, ShieldMode.STARTING, ShieldMode.HIDDEN -> Icons.Outlined.Shield
    ShieldMode.OFF -> Icons.Outlined.RemoveModerator
    ShieldMode.UNFILTERED, ShieldMode.UNAVAILABLE -> Icons.Outlined.GppMaybe
}

@Composable
private fun shieldTint(mode: ShieldMode): Color = when (mode) {
    ShieldMode.ACTIVE -> MaterialTheme.colorScheme.primary
    ShieldMode.UNFILTERED, ShieldMode.UNAVAILABLE -> MaterialTheme.colorScheme.error
    else -> MaterialTheme.colorScheme.onSurfaceVariant
}

/** What a screen reader says for the shield. */
@Composable
fun shieldDescription(ui: ShieldUi): String = when (ui.mode) {
    ShieldMode.ACTIVE -> {
        val n = ui.count.toIntOrNull()
        if (n != null) {
            pluralStringResource(R.plurals.shield_blocked, n, n)
        } else {
            stringResource(R.string.shield_blocked_many, ui.count)
        }
    }

    ShieldMode.ALLOWED -> stringResource(R.string.shield_allowed)

    ShieldMode.OFF -> stringResource(R.string.shield_off)

    ShieldMode.UNFILTERED -> stringResource(R.string.shield_unfiltered)

    ShieldMode.UNAVAILABLE -> stringResource(R.string.shield_unavailable)

    ShieldMode.STARTING -> stringResource(R.string.shield_starting)

    ShieldMode.HIDDEN -> ""
}

/**
 * The shield inside the address pill: a 48 dp target with the mode's icon and, when something
 * was blocked on this page, the count at its corner.
 */
@Composable
fun ShieldButton(ui: ShieldUi, onClick: () -> Unit) {
    val label = shieldDescription(ui)
    Box(
        Modifier
            .size(ToolbarLayout.BUTTON_DP.dp)
            .clip(CircleShape)
            .clickable(onClick = onClick)
            .semantics {
                role = Role.Button
                contentDescription = label
            },
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            shieldIcon(ui.mode),
            contentDescription = null,
            modifier = Modifier.size(20.dp),
            tint = shieldTint(ui.mode),
        )
        if (ui.showCount) {
            Text(
                ui.count,
                modifier = Modifier.align(Alignment.BottomEnd).padding(end = 4.dp, bottom = 8.dp),
                color = MaterialTheme.colorScheme.onSurface,
                fontSize = 10.sp,
                fontWeight = FontWeight.Bold,
                maxLines = 1,
            )
        }
    }
}

/**
 * The sheet behind the shield: what was blocked on this page, the site, and the switch that
 * turns blocking off for the site (the tab reloads when it changes). Private tabs get a hint
 * instead of the switch.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ShieldSheet(
    ui: ShieldUi,
    site: String,
    privateTab: Boolean,
    onAllow: (allowed: Boolean) -> Unit,
    onReload: () -> Unit,
    onSettings: () -> Unit,
    onDismiss: () -> Unit,
) {
    val state = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = state) {
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(shieldIcon(ui.mode), contentDescription = null, tint = shieldTint(ui.mode))
                Spacer(Modifier.width(12.dp))
                Text(stringResource(R.string.shield_sheet_title), style = MaterialTheme.typography.titleLarge)
            }
            if (site.isNotEmpty()) {
                Text(
                    site,
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.StartEllipsis,
                    modifier = Modifier.padding(top = 4.dp),
                )
            }
            Text(
                shieldDescription(ui),
                style = MaterialTheme.typography.headlineSmall,
                modifier = Modifier.padding(top = 16.dp),
            )
            when (ui.mode) {
                ShieldMode.UNFILTERED -> {
                    Text(stringResource(R.string.shield_unfiltered_help), Modifier.padding(top = 8.dp))
                    TextButton(onClick = {
                        onDismiss()
                        onReload()
                    }) { Text(stringResource(R.string.toolbar_reload)) }
                }

                ShieldMode.OFF -> {
                    Text(stringResource(R.string.shield_off_help), Modifier.padding(top = 8.dp))
                    TextButton(onClick = {
                        onDismiss()
                        onSettings()
                    }) { Text(stringResource(R.string.menu_settings)) }
                }

                ShieldMode.UNAVAILABLE -> Text(
                    stringResource(R.string.shield_unavailable_help),
                    Modifier.padding(top = 8.dp),
                )

                else -> Unit
            }
            when (val switch = allowSwitch(ui, privateTab)) {
                AllowSwitch.BLOCKING, AllowSwitch.ALLOWED -> {
                    val blocking = switch == AllowSwitch.BLOCKING
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .padding(top = 16.dp)
                            .toggleable(value = blocking, role = Role.Switch) { on ->
                                onDismiss()
                                onAllow(!on)
                            },
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(Modifier.weight(1f).padding(vertical = 8.dp)) {
                            Text(
                                stringResource(R.string.shield_site_switch),
                                style = MaterialTheme.typography.bodyLarge,
                            )
                            Text(
                                stringResource(R.string.shield_site_switch_help),
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        Spacer(Modifier.width(16.dp))
                        KrunkSwitch(checked = blocking, onCheckedChange = null)
                    }
                }

                AllowSwitch.PRIVATE_TAB -> Text(
                    stringResource(R.string.shield_private_hint),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 16.dp),
                )

                AllowSwitch.NONE -> Unit
            }
            Spacer(Modifier.height(24.dp))
        }
    }
}
