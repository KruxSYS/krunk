package org.krunk.browser.vault

import java.io.File

/**
 * Where the vault lives: `filesDir/krunk-vault/`, beside (not inside) `filesDir/krunk`, the one
 * directory device-to-device transfer carries. The backup rules exclude [DIR] by name as well
 * (`res/xml/backup_rules.xml`, `data_extraction_rules.xml`; `BackupRulesTest` holds the two
 * together): the Keystore key that wraps the vault key cannot leave the device, so a copy of
 * the file would never open.
 */
object VaultFiles {
    const val DIR = "krunk-vault"
    const val VAULT_FILE = "vault.json"
    const val STATE_FILE = "state.json"

    fun dir(filesDir: File): File = File(filesDir, DIR)

    fun vaultFile(filesDir: File): File = File(dir(filesDir), VAULT_FILE)

    fun stateFile(filesDir: File): File = File(dir(filesDir), STATE_FILE)
}
