package org.krunk.browser.ui.browser

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import org.krunk.browser.R
import org.krunk.browser.engine.CertError
import org.krunk.browser.engine.CertErrors
import org.krunk.browser.engine.CertProblem

/**
 * Shown instead of the page when a load failed on the site's certificate. "Back to safety" is
 * the main button; "Continue anyway" only when the error may be overridden, and it acts on the
 * tab's own record of the failed load ([CertError]), not on anything the page says.
 */
@Composable
internal fun PageCertError(
    error: CertError,
    onBack: () -> Unit,
    onContinue: (remember: Boolean, onDone: (Boolean) -> Unit) -> Unit,
    modifier: Modifier = Modifier,
) {
    // A new error (another site, another certificate) starts unchecked again.
    var keep by remember(error) { mutableStateOf(false) }
    var busy by remember(error) { mutableStateOf(false) }
    var failed by remember(error) { mutableStateOf(false) }
    val site = CertErrors.hostPort(error.host, error.port)
    Column(
        modifier.verticalScroll(rememberScrollState()).padding(32.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(stringResource(R.string.cert_title), style = MaterialTheme.typography.titleLarge)
        Text(
            site,
            style = MaterialTheme.typography.bodyLarge,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 12.dp),
        )
        Text(
            stringResource(problemText(error.problem), site),
            style = MaterialTheme.typography.bodyMedium,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 12.dp),
        )
        Text(
            stringResource(if (error.overridable) R.string.cert_risk else R.string.cert_hsts),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 12.dp),
        )
        Text(
            stringResource(R.string.cert_fingerprint),
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(top = 16.dp),
        )
        Text(
            error.sha256,
            style = MaterialTheme.typography.bodySmall,
            fontFamily = FontFamily.Monospace,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 4.dp, bottom = 24.dp),
        )
        Button(onClick = onBack) { Text(stringResource(R.string.cert_back)) }
        if (error.overridable) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(top = 16.dp)
                    .toggleable(value = keep, role = Role.Checkbox, enabled = !busy) { keep = it },
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Checkbox(checked = keep, onCheckedChange = null, enabled = !busy)
                Column(Modifier.padding(start = 8.dp)) {
                    Text(stringResource(R.string.cert_remember), style = MaterialTheme.typography.bodyMedium)
                    Text(
                        stringResource(
                            if (error.private) R.string.cert_private_temporary else R.string.cert_remember_help,
                        ),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            OutlinedButton(
                onClick = {
                    busy = true
                    failed = false
                    onContinue(keep) { ok ->
                        busy = false
                        failed = !ok
                    }
                },
                enabled = !busy,
                modifier = Modifier.padding(top = 8.dp),
            ) { Text(stringResource(R.string.cert_continue)) }
            if (failed) {
                Text(
                    stringResource(R.string.cert_failed),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.padding(top = 8.dp),
                )
            }
        }
    }
}

private fun problemText(problem: CertProblem): Int = when (problem) {
    CertProblem.EXPIRED -> R.string.cert_problem_expired
    CertProblem.NOT_YET_VALID -> R.string.cert_problem_not_yet_valid
    CertProblem.WRONG_HOST -> R.string.cert_problem_wrong_host
    CertProblem.SELF_SIGNED -> R.string.cert_problem_self_signed
    CertProblem.UNTRUSTED -> R.string.cert_problem_untrusted
}
