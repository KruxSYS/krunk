package org.krunk.browser.ui.vault

import android.app.Application

/**
 * The vault behind the screens: the one place that decides which. For now an in-memory vault
 * that lives as long as the process ([PreviewVaultPort]: nothing is encrypted, nothing is kept);
 * [app] is what the real one is reached through.
 */
fun vaultPort(app: Application): VaultPort = previewPorts.getOrPut(app) { PreviewVaultPort() }

private val previewPorts = java.util.concurrent.ConcurrentHashMap<Application, PreviewVaultPort>()
