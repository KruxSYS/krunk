# `core::sync`: the sync crypto core (milestone S4)

These are pure functions with no I/O, clock or network. They implement the crypto and format parts of
`docs/sync/PROTOCOL.md`. The app doesn't call them yet (S6). Android uses this same code through
UniFFI (PLAN §5.7).

| Module | PROTOCOL | What |
|---|---|---|
| `labels.rs` | §4–§9 | Every HKDF info, AAD and seal label |
| `crypto.rs` | §2, §3.3, §5 | `lp` encoding; HKDF-SHA256 and HMAC-SHA256 with constant-time verify; XChaCha20-Poly1305 with 24-byte nonces; the passphrase KDF (Argon2id m=131072 KiB, t=3, p=1, **floors enforced**); `KEK_rk`; `rr_wrap`/`rr_unwrap` (commit tag checked first, 48-byte ciphertext, `rr_gen` in the AAD); the escrow keypair from RR; HPKE seal/open (RFC 9180 base mode, `hpke` 0.15.1) with the §3.3 `info`; rejection of low-order X25519 keys |
| `codes.rs` | §3.2, §5.2, §6.1 | Crockford base32 (O→0, I/L→1, `-` and spaces ignored, strict padding bits); the recovery key `KRNK-…-C` with its mod-37 check symbol; the `user_code` |
| `dir.rs` | §4 | `ak_dir`/`pak_dir` over exact bytes; `mac`, `chain` and `escrow_mac`; history wraps; the **acceptance rules of §4.4** (`accept_ak`, `accept_pak`, `open_anchor`), returning `Fork`, `BadTag`, `Gap`, `Stale`, `EpochSkip`, `WeakKdf`, `EscrowKeyChanged`, `SelfRevoked` and so on; seal targets (directory ∩ server-active, never revoked) |
| `envelope.rs` | §9.1–§9.2 | Collection keys `CK`/`IK`; record ids `HMAC(IK, key)[..16]`; the exact length-prefixed AAD; ISO 7816-4 padding into 256 B–64 KiB buckets (a tombstone keeps its bucket); envelope seal/open with strict base64url |
| `sas.rs` | §6.2 | Commitment; length-prefixed transcript; `prk` = HKDF-Extract(SHA-256(T), dh); 6-digit SAS; directional `k_AN`/`k_NA`; box with direction byte; confirm tag |
| `hlc.rs` | §9.3 | 48-bit ms + 16-bit counter; the local clock never runs more than 24 h ahead, and poisoned versions are clamped and flagged |
| `normalize.rs` | §9.6 | History record key: WHATWG URL with IDNA, http(s) only, no fragment or userinfo, tracking parameters removed (`urlclean`) |
| `vectors.rs` | §12 | Vector generator and verifier, the RFC 9180 A.2 interop test, and mutation tests |

## Test vectors
- **`core/tests/fixtures/sync/vectors-v1.json`** (frozen):
  - Contents: Argon2id at the shipped parameters, HKDF for every label, recovery keys and the
    user code, RR wraps, the escrow keypair, HPKE seals with deterministic ephemerals, directory
    tags over fixed bytes, a full record (id, AAD, padding table, envelope), the
    device-approval exchange (commitment, transcript, dh, SAS, `k_AN`/`k_NA`, confirm, box),
    30 URL normalisations, and HLC cases.
  - Each test run rebuilds it and must match byte for byte. Regenerate only on purpose, with
    `KRUNK_GEN_VECTORS=1 cargo test -p core sync::vectors`, and bump the version if a format
    changed.
  - The Kotlin port must reproduce the same file.
- **`core/tests/fixtures/sync/rfc9180-a2.json`**: the RFC's own vectors. The test reproduces
  the sender's `enc` and the ciphertexts at sequence numbers 0, 1, 2, 4, 255 and 256 from
  `ikmE`, and opens the first one. Private keys are compared in clamped form, because the RFC
  prints them unclamped.
- **Robustness:** `cargo-fuzz` isn't available here, so deterministic mutation tests replace
  it. They cover every single-byte flip, truncation and extension of envelopes, directories,
  `rr_wrap`s, boxes and seals, plus 10,000 seeded random inputs each. All are rejected, with no
  panics.
- **Tree repair:** the bookmark tree-repair vectors (PROTOCOL §12 item 12) belong with the
  merge code in S7.
