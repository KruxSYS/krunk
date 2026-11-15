package org.krunk.browser.ui.settings

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.krunk.browser.R
import org.krunk.browser.engine.CertErrors
import org.krunk.browser.engine.CertException
import org.krunk.browser.ui.BrowserViewModel

/** What the screen shows: still reading, the list, or that the engine did not answer. */
private sealed interface CertRows {
    data object Loading : CertRows

    data object Unavailable : CertRows

    data class Listed(val rows: List<CertException>) : CertRows
}

/**
 * Settings › Privacy › Certificate exceptions: the sites whose untrusted certificate the user
 * accepted (host:port, the start of the certificate's SHA-256), each with Remove. Removing takes
 * effect at once: the engine forgets the exception and drops the open connections.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CertExceptionsScreen(vm: BrowserViewModel) {
    var generation by remember { mutableIntStateOf(0) }
    var state by remember { mutableStateOf<CertRows>(CertRows.Loading) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(generation) {
        state = vm.engine.certs.list()?.let { CertRows.Listed(it) } ?: CertRows.Unavailable
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.cert_exceptions_title)) },
                navigationIcon = {
                    IconButton(onClick = { vm.pop() }) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = stringResource(R.string.action_back),
                        )
                    }
                },
            )
        },
    ) { padding ->
        val modifier = Modifier.fillMaxSize().padding(padding)
        when (val s = state) {
            CertRows.Loading -> Box(modifier)

            CertRows.Unavailable -> Message(stringResource(R.string.cert_exceptions_unavailable), modifier)

            is CertRows.Listed -> if (s.rows.isEmpty()) {
                Message(stringResource(R.string.cert_exceptions_empty), modifier)
            } else {
                LazyColumn(modifier) {
                    items(s.rows, key = { it.hostPort }) { row ->
                        ExceptionLine(row) {
                            scope.launch {
                                vm.engine.certs.remove(row.host, row.port)
                                generation++
                            }
                        }
                        HorizontalDivider()
                    }
                }
            }
        }
    }
}

@Composable
private fun Message(text: String, modifier: Modifier) {
    Box(modifier.padding(32.dp), contentAlignment = Alignment.Center) {
        Text(text, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun ExceptionLine(row: CertException, onRemove: () -> Unit) {
    val fingerprint = "SHA-256 " + CertErrors.shortFingerprint(row.sha256)
    ListItem(
        headlineContent = { Text(row.hostPort) },
        supportingContent = {
            Text(
                if (row.temporary) {
                    fingerprint + " · " + stringResource(
                        R.string.cert_exceptions_temporary,
                    )
                } else {
                    fingerprint
                },
                fontFamily = FontFamily.Monospace,
                style = MaterialTheme.typography.bodySmall,
            )
        },
        trailingContent = {
            IconButton(onClick = onRemove) {
                Icon(
                    Icons.Filled.Delete,
                    contentDescription = stringResource(R.string.cert_exceptions_remove_for, row.hostPort),
                )
            }
        },
    )
}
