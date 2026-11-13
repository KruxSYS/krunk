package org.krunk.browser.ui.vault

import android.app.Application
import org.krunk.browser.KrunkApp

/**
 * The vault behind the screens: the app's own ([AppVaultPort]), one per process. Anything that
 * is not Krunk's application (a preview) gets an in-memory stand-in.
 */
fun vaultPort(app: Application): VaultPort = ports.getOrPut(app) {
    if (app is KrunkApp) AppVaultPort(app) else PreviewVaultPort()
}

private val ports = java.util.concurrent.ConcurrentHashMap<Application, VaultPort>()
