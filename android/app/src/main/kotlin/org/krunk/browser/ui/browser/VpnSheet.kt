package org.krunk.browser.ui.browser

import android.text.format.Formatter
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.VpnLock
import androidx.compose.material.icons.outlined.VpnKey
import androidx.compose.material.icons.outlined.VpnKeyOff
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import org.krunk.browser.R
import org.krunk.browser.ui.settings.VpnScopeChoice
import org.krunk.browser.ui.settings.vpnKindLabel
import org.krunk.browser.ui.settings.vpnStatusText
import org.krunk.browser.ui.theme.LocalKrunkStyle
import org.krunk.browser.vpn.VpnPhase
import org.krunk.browser.vpn.VpnScope
import org.krunk.browser.vpn.VpnUiState

/** Each phase has its own icon, so the states differ without colour. */
private fun vpnIcon(phase: VpnPhase): ImageVector = when (phase) {
    VpnPhase.CONNECTED -> Icons.Filled.VpnLock
    VpnPhase.CONNECTING -> Icons.Outlined.VpnKey
    VpnPhase.FAILED, VpnPhase.OFF -> Icons.Outlined.VpnKeyOff
}

/**
 * Green / amber / red for [phase], readable on [background]: the darker shades on a light
 * background, the lighter ones on a dark one (light and dark themes, tinted pills).
 */
fun vpnTint(phase: VpnPhase, background: Color, secure: Color? = null): Color {
    val dark = background.luminance() < DARK_LUMINANCE
    return when (phase) {
        VpnPhase.CONNECTED -> secure ?: if (dark) Color(0xFF4ADE80) else Color(0xFF15803D)
        VpnPhase.CONNECTING -> if (dark) Color(0xFFFBBF24) else Color(0xFFB45309)
        VpnPhase.FAILED, VpnPhase.OFF -> if (dark) Color(0xFFF87171) else Color(0xFFDC2626)
    }
}

private const val DARK_LUMINANCE = 0.4f

@Composable
fun vpnMarkDescription(phase: VpnPhase): String = stringResource(
    when (phase) {
        VpnPhase.CONNECTED -> R.string.vpn_mark_connected
        VpnPhase.CONNECTING -> R.string.vpn_mark_connecting
        VpnPhase.FAILED, VpnPhase.OFF -> R.string.vpn_mark_failed
    },
)

/** The VPN mark inside the address pill, next to the shield: 36 dp wide, 48 dp high. */
@Composable
fun VpnMarkButton(phase: VpnPhase, onClick: () -> Unit) {
    val label = vpnMarkDescription(phase)
    val style = LocalKrunkStyle.current
    Box(
        Modifier
            .size(width = 36.dp, height = ToolbarLayout.BUTTON_DP.dp)
            .clip(CircleShape)
            .clickable(onClick = onClick)
            .semantics {
                role = Role.Button
                contentDescription = label
            },
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            vpnIcon(phase),
            contentDescription = null,
            modifier = Modifier.size(18.dp),
            tint = vpnTint(phase, style.pill, secure = style.secure.takeIf { phase == VpnPhase.CONNECTED }),
        )
    }
}

/**
 * The sheet behind the VPN mark (like the Windows popout): profile, state, tunnel address,
 * traffic, Connect / Disconnect, the scope, and the way to the VPN settings.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun VpnSheet(
    state: VpnUiState,
    onConnect: () -> Unit,
    onDisconnect: () -> Unit,
    onScope: (VpnScope) -> Unit,
    onSettings: () -> Unit,
    onDismiss: () -> Unit,
) {
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val context = LocalContext.current
    val profile = state.profiles.firstOrNull { it.id == state.settings.selectedProfileId }
        ?: state.profiles.singleOrNull()
    val status = state.status
    val tint = vpnTint(state.phase, MaterialTheme.colorScheme.surface)
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet) {
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(vpnIcon(state.phase), contentDescription = null, tint = tint)
                Spacer(Modifier.width(12.dp))
                Text(stringResource(R.string.vpn_title), style = MaterialTheme.typography.titleLarge)
            }
            Text(
                profile?.let {
                    stringResource(R.string.vpn_profile_line, it.name, vpnKindLabel(it.kind))
                } ?: stringResource(R.string.vpn_sheet_no_profile),
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                modifier = Modifier.padding(top = 4.dp),
            )
            Text(
                vpnStatusText(state),
                style = MaterialTheme.typography.headlineSmall,
                color = if (state.phase == VpnPhase.OFF) MaterialTheme.colorScheme.onSurface else tint,
                modifier = Modifier.padding(top = 16.dp),
            )
            if (state.phase == VpnPhase.CONNECTED && status != null) {
                status.tunnelIp?.takeIf { it.isNotBlank() }?.let {
                    Text(stringResource(R.string.vpn_sheet_tunnel_ip, it), Modifier.padding(top = 8.dp))
                }
                Text(
                    stringResource(
                        R.string.vpn_sheet_traffic,
                        Formatter.formatShortFileSize(context, status.rxBytes),
                        Formatter.formatShortFileSize(context, status.txBytes),
                    ),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 4.dp),
                )
            }
            Row(Modifier.padding(top = 16.dp)) {
                if (state.settings.enabled) {
                    // Failed: the same button tries again.
                    if (state.phase == VpnPhase.FAILED) {
                        Button(onClick = onConnect) { Text(stringResource(R.string.vpn_connect)) }
                        Spacer(Modifier.width(8.dp))
                    }
                    OutlinedButton(onClick = onDisconnect) { Text(stringResource(R.string.vpn_disconnect)) }
                } else {
                    Button(onClick = onConnect, enabled = state.profiles.isNotEmpty()) {
                        Text(stringResource(R.string.vpn_connect))
                    }
                }
            }
            Text(
                stringResource(R.string.vpn_scope),
                style = MaterialTheme.typography.titleSmall,
                modifier = Modifier.padding(top = 16.dp, bottom = 4.dp),
            )
            VpnScopeChoice(state.settings.scope, onScope)
            TextButton(onClick = {
                onDismiss()
                onSettings()
            }) { Text(stringResource(R.string.vpn_open_settings)) }
            Spacer(Modifier.height(24.dp))
        }
    }
}
