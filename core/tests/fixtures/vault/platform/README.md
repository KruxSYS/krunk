# Vault files per platform

Test: `ffi/src/vault/tests.rs` (`vaults_written_on_windows_and_on_android_hold_the_same_logins`,
`platform_vaults_as_written_do_not_open_off_their_machine`).

## Status: the two files here are STAND-INS

`vault-windows-inner.json` and `vault-android-inner.json` were **not** written by Krunk on Windows
or by Krunk on Android. Both were made on Linux by the shared core
(`cargo test -p krunk_ffi -- --ignored generate_the_platform_stand_ins`), so today the test proves
only that the Android object reads what the core writes. They are to be replaced by files produced
on the platforms, as described below. `vault-windows-dpapi.json` and `vault-android-keystore.json`
do not exist yet; the second test skips a platform whose file is missing and says so.

## What belongs here

A DPAPI or Android Keystore blob opens only on the machine that made it, so each platform has a
pair of files:

| File | Made by | `wrap` |
|---|---|---|
| `vault-windows-dpapi.json` | Krunk 0.19.0 on the Windows VM, through `krunk://passwords` | `dpapi` |
| `vault-windows-inner.json` | the same file with the DPAPI layer of `vaultKey` removed on that VM | `none` |
| `vault-android-keystore.json` | the Android debug app on the emulator, real Keystore | `android-keystore` |
| `vault-android-inner.json` | the same file with the Keystore layer of `vaultKey` removed on that emulator | `none` |

The inner twin differs from the file as written in `wrap` and `vaultKey` only: `vaultKey` becomes
the still password-sealed vault key (what the platform's unprotect call returns), `format`, `kdf`,
`check`, `manifest` and `entries` stay the same text. The test compares exactly these.

Content of every file, so the test needs no knowledge of ids, times or the order of entries:

- vault password `fixture password`
- `alice` at `https://bank.example`, password `s3cret-one`
- `bob` at `https://mail.example:8443`, password `s3cret-two`
- no cards, no recovery phrase, no sync (the inner twin could not carry a recovery slot: its
  `sealed` field is wrapped by the platform as well)

The stand-ins use the cost a new vault gets in both apps (Argon2id, 64 MiB, 3 passes, 1 lane).

## Replacing a stand-in

Put the new files here under the names above and run `cargo test -p krunk_ffi platform`. The
generator refuses to overwrite a file that exists unless `KRUNK_REGEN_FIXTURES=1` is set, so a
real file is not replaced by a stand-in by accident. Then rewrite the status section above: say
which Krunk version, which machine and which strip script produced each file.
