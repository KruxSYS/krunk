Test-only Ed25519 key pair and a manifest signed with it by OpenSSL
(`openssl pkeyutl -sign -rawin`, as `dist/server/publish-release.sh` does). Used by
`core/src/update.rs` tests and the dev update-flow test. This key signs nothing real:
release builds trust only `assets/keys/update-signing.pub`.
