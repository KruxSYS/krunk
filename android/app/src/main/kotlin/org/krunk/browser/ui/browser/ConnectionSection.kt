package org.krunk.browser.ui.browser

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.widget.Toast
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.outlined.ExpandLess
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.Warning
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import java.security.cert.X509Certificate
import java.text.DateFormat
import java.util.Date
import kotlinx.coroutines.launch
import org.krunk.browser.R
import org.krunk.browser.engine.CertDetails
import org.krunk.browser.engine.CertValidity
import org.krunk.browser.engine.ConnectionKind
import org.krunk.browser.engine.MixedContent
import org.krunk.browser.engine.PageSecurity
import org.krunk.browser.ui.theme.LocalKrunkStyle

/** More names than this are folded behind "Show all names". */
private const val NAMES_SHOWN = 5

/** Where a certificate shown in the sheet came from. */
private sealed interface ProbeState {
    data object Idle : ProbeState

    data object Running : ProbeState

    data object Failed : ProbeState

    data class Read(val details: CertDetails) : ProbeState
}

/**
 * The shield sheet's "Connection" part: whether the connection is secure and, for https, the
 * site's certificate as the engine reported it. [titleModifier] goes on the section's title (the
 * lock in the address bar scrolls the sheet to it). [onForgetException] drops a certificate the
 * user accepted ("Continue anyway") and reloads. [probe] reads the certificate over a connection
 * of Krunk's own; it is only offered when the engine gave none for an https page (a page that
 * loaded content without encryption), and only runs when the user asks.
 */
@Composable
fun ConnectionSection(
    kind: ConnectionKind,
    security: PageSecurity?,
    onForgetException: () -> Unit,
    probe: (suspend () -> X509Certificate?)?,
    titleModifier: Modifier = Modifier,
) {
    if (kind == ConnectionKind.NONE) return
    HorizontalDivider(Modifier.padding(top = 16.dp))
    Text(
        stringResource(R.string.conn_title),
        style = MaterialTheme.typography.titleMedium,
        modifier = titleModifier.padding(top = 16.dp).semantics { heading() },
    )
    val details = remember(security?.certificate) { security?.certificate?.let { CertDetails.of(it) } }
    when (kind) {
        ConnectionKind.KRUNK_PAGE -> Status(
            Icons.Outlined.Info,
            MaterialTheme.colorScheme.onSurfaceVariant,
            stringResource(R.string.conn_krunk_page),
            stringResource(R.string.conn_krunk_page_help),
        )

        ConnectionKind.NOT_SECURE -> Status(
            Icons.Outlined.Info,
            MaterialTheme.colorScheme.error,
            stringResource(R.string.conn_not_secure),
            stringResource(R.string.conn_not_secure_help),
        )

        ConnectionKind.SECURE -> {
            Status(
                Icons.Filled.Lock,
                LocalKrunkStyle.current.secure,
                stringResource(R.string.conn_secure),
                stringResource(R.string.conn_secure_help),
            )
            MixedLine(security)
            details?.let { CertificateBlock(it) }
        }

        ConnectionKind.EXCEPTION -> {
            Status(
                Icons.Outlined.Warning,
                MaterialTheme.colorScheme.error,
                stringResource(R.string.conn_exception),
                stringResource(R.string.conn_exception_help),
            )
            OutlinedButton(onClick = onForgetException, modifier = Modifier.padding(top = 8.dp)) {
                Text(stringResource(R.string.conn_forget))
            }
            MixedLine(security)
            details?.let { CertificateBlock(it) }
        }

        ConnectionKind.BROKEN -> {
            Status(
                Icons.Outlined.Info,
                MaterialTheme.colorScheme.error,
                stringResource(R.string.conn_broken),
                null,
            )
            MixedLine(security)
            if (details != null) {
                CertificateBlock(details)
            } else {
                ProbeBlock(probe)
            }
        }

        ConnectionKind.NONE -> Unit
    }
}

@Composable
private fun Status(icon: ImageVector, tint: Color, title: String, help: String?) {
    Row(Modifier.padding(top = 12.dp), verticalAlignment = Alignment.Top) {
        // The words say it; the icon is decoration for a screen reader.
        Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.padding(top = 2.dp).size(20.dp))
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium)
            if (help != null) {
                Text(
                    help,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun MixedLine(security: PageSecurity?) {
    val s = security ?: return
    val text = when {
        s.mixedLoaded -> R.string.conn_mixed
        s.mixedActive == MixedContent.BLOCKED || s.mixedPassive == MixedContent.BLOCKED -> R.string.conn_mixed_blocked
        else -> return
    }
    Text(
        stringResource(text),
        style = MaterialTheme.typography.bodyMedium,
        color = if (s.mixedLoaded) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(top = 8.dp, start = 32.dp),
    )
}

@Composable
private fun ProbeBlock(probe: (suspend () -> X509Certificate?)?) {
    if (probe == null) {
        Note(stringResource(R.string.conn_no_cert))
        return
    }
    val scope = rememberCoroutineScope()
    var state by remember { mutableStateOf<ProbeState>(ProbeState.Idle) }
    when (val s = state) {
        ProbeState.Idle -> {
            Note(stringResource(R.string.conn_no_cert))
            TextButton(onClick = {
                state = ProbeState.Running
                scope.launch {
                    val cert = probe()
                    state = cert?.let { CertDetails.of(it) }?.let { ProbeState.Read(it) } ?: ProbeState.Failed
                }
            }) { Text(stringResource(R.string.conn_probe)) }
        }

        ProbeState.Running -> Note(stringResource(R.string.conn_probe_running))

        ProbeState.Failed -> Note(stringResource(R.string.conn_probe_failed))

        is ProbeState.Read -> {
            Note(stringResource(R.string.conn_probe_note))
            CertificateBlock(s.details)
        }
    }
}

@Composable
private fun Note(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(top = 8.dp),
    )
}

/** Issued to / by, validity, SHA-256, the names, and the folded technical details. */
@Composable
private fun CertificateBlock(d: CertDetails) {
    val unknown = stringResource(R.string.conn_unknown)
    val dates = remember { DateFormat.getDateInstance(DateFormat.MEDIUM) }
    val validity = d.validity(System.currentTimeMillis())
    Column(Modifier.padding(top = 12.dp)) {
        Field(
            stringResource(R.string.conn_issued_to),
            listOfNotNull(d.issuedTo, d.subjectO).distinct().joinToString("\n").ifEmpty { unknown },
        )
        Field(
            stringResource(R.string.conn_issued_by),
            listOfNotNull(d.issuerO, d.issuerCn).distinct().joinToString("\n").ifEmpty { unknown },
        )
        Field(
            stringResource(R.string.conn_valid_from),
            dates.format(Date(d.notBeforeMs)) +
                if (validity == CertValidity.NOT_YET_VALID) " · " + stringResource(R.string.conn_not_yet_valid) else "",
            error = validity == CertValidity.NOT_YET_VALID,
        )
        Field(
            stringResource(R.string.conn_valid_until),
            dates.format(Date(d.notAfterMs)) +
                if (validity == CertValidity.EXPIRED) " · " + stringResource(R.string.conn_expired) else "",
            error = validity == CertValidity.EXPIRED,
        )
        Fingerprint(stringResource(R.string.conn_fingerprint), d.sha256, copyHint = true)
        Names(d.altNames)
        Details(d, unknown)
    }
}

@Composable
private fun Field(label: String, value: String, error: Boolean = false, mono: Boolean = false) {
    Column(Modifier.fillMaxWidth().padding(top = 8.dp).semantics(mergeDescendants = true) {}) {
        Text(
            label,
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            value,
            style = MaterialTheme.typography.bodyMedium,
            color = if (error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface,
            fontFamily = if (mono) FontFamily.Monospace else null,
        )
    }
}

/** A fingerprint in monospace, wrapped; a long press (or the screen reader's action) copies it. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun Fingerprint(label: String, value: String, copyHint: Boolean) {
    val context = LocalContext.current
    val copyLabel = stringResource(R.string.conn_copy_action)
    val copied = stringResource(R.string.conn_fingerprint_copied)
    val copy = { copyFingerprint(context, value, copied) }
    Column(
        Modifier
            .fillMaxWidth()
            .padding(top = 8.dp)
            .combinedClickable(
                onClick = {},
                onLongClick = copy,
                onLongClickLabel = copyLabel,
                role = Role.Button,
            )
            .semantics(mergeDescendants = true) {},
    ) {
        Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value, style = MaterialTheme.typography.bodyMedium, fontFamily = FontFamily.Monospace)
        if (copyHint) {
            Text(
                stringResource(R.string.conn_fingerprint_copy),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

private fun copyFingerprint(context: Context, value: String, copied: String) {
    val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return
    cm.setPrimaryClip(ClipData.newPlainText("SHA-256", value))
    // Android 13 and later confirm a copy themselves.
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) Toast.makeText(context, copied, Toast.LENGTH_SHORT).show()
}

@Composable
private fun Names(names: List<String>) {
    if (names.isEmpty()) return
    var all by rememberSaveable { mutableStateOf(false) }
    val shown = if (all || names.size <= NAMES_SHOWN) names else names.take(NAMES_SHOWN)
    Field(
        pluralStringResource(R.plurals.conn_names, names.size, names.size),
        shown.joinToString("\n") + if (shown.size < names.size) "\n…" else "",
    )
    if (names.size > NAMES_SHOWN) {
        TextButton(onClick = { all = !all }) {
            Text(stringResource(if (all) R.string.conn_names_hide else R.string.conn_names_show))
        }
    }
}

@Composable
private fun Details(d: CertDetails, unknown: String) {
    var open by rememberSaveable { mutableStateOf(false) }
    val action = stringResource(if (open) R.string.conn_details_hide else R.string.conn_details_show)
    Row(
        Modifier
            .fillMaxWidth()
            .padding(top = 8.dp)
            .combinedClickable(onClick = { open = !open }, onClickLabel = action, role = Role.Button)
            .padding(vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            stringResource(R.string.conn_details),
            style = MaterialTheme.typography.bodyLarge,
            modifier = Modifier.weight(1f),
        )
        Icon(if (open) Icons.Outlined.ExpandLess else Icons.Outlined.ExpandMore, contentDescription = null)
    }
    if (open) {
        Field(stringResource(R.string.conn_serial), d.serial.ifEmpty { unknown }, mono = true)
        Field(stringResource(R.string.conn_signature), d.signatureAlgorithm.ifEmpty { unknown })
        Field(
            stringResource(R.string.conn_key),
            when {
                d.keyAlgorithm.isEmpty() -> unknown
                d.keyBits > 0 -> stringResource(R.string.conn_key_bits, d.keyAlgorithm, d.keyBits)
                else -> d.keyAlgorithm
            },
        )
        Fingerprint(stringResource(R.string.conn_sha1), d.sha1, copyHint = false)
    }
}
