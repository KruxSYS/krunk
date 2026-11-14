# Vault files per platform

Test: `ffi/src/vault/tests.rs` (`vaults_written_on_windows_and_on_android_hold_the_same_logins`,
`platform_vaults_as_written_do_not_open_off_their_machine`).

## Status

**Windows: real.** `vault-windows-dpapi.json` is the unchanged `vault.json` that Krunk 0.19.0
(release build, hashes equal to `dist/SHA256SUMS`) wrote on the Windows VM (build 26200.8037,
account `WIN-QN0ABMLAO9I\\pipe`, throwaway profile) on 2026-11-13: vault created and both logins
added through `krunk://passwords`. `vault-windows-inner.json` was made from it on the same VM and
account by `strip-dpapi.ps1` (in this folder): DPAPI `Unprotect` of `vaultKey` with the vault's
entropy; the two files differ in the `wrap` and `vaultKey` lines only. SHA-256:
`d5d35fa7…832d` (as written), `58dee4e1…22c2` (inner). Full notes, the screenshot of the passwords
page and the verification output: `.pipeline/evidence-android-0.3.0/fixtures/windows/README.md`.

**Android: real.** `vault-android-keystore.json` is the unchanged `files/krunk-vault/vault.json`
that the debug build of Krunk for Android (main at `3efdf63`, on the way to 0.3.0) wrote on the
sandbox emulator (Android 16, `sdk_gphone64_x86_64`, no StrongBox: the wrap key is the TEE-backed
Keystore key) on 2026-11-14: vault created and both logins added through the Passwords screen
(`android/scripts/a4/a4.sh fixture`). `vault-android-inner.json` was made from it on the same
install by the debug hook `vault-fixture` (`DebugHooks.kt`): Keystore `unprotect` of `vaultKey`;
the two files differ in the `wrap` and `vaultKey` lines only. SHA-256: `30c85426ba8ee0ecefbd92d51ccfe099ebed5c1c8631bfbf610c739124000901` (as written),
`82c182838dcb9b543af6662fc3321ea8cbc6f4de336bd4ba6010cd750cfb4870` (inner). Pulled files and the run's log: `.pipeline/evidence-android-0.3.0/m5/fixture/`,
`m5/run-fixture.log`.

Neither file as written can be opened anywhere else; the test checks that this fails cleanly and
that everything but `wrap` and `vaultKey` equals the inner twin.

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

## Making them again

Windows: `strip-dpapi.ps1` here and the steps in the evidence README. Android:
`android/scripts/a4/a4.sh fixture` on a debug build. `cargo test -p krunk_ffi platform` then
checks the new files. (The stand-in generator `generate_the_platform_stand_ins` remains as an
ignored test; it refuses to overwrite a file unless `KRUNK_REGEN_FIXTURES=1` is set.)
