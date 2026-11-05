# Krunk sync: threat model (design)

This extends `docs/SECURITY.md` (the local vault) to the optional account sync. It covers
assets, attackers, mitigations, guarantees and what is out of scope. Mechanisms are specified
in `PROTOCOL.md` (§ references below); architecture is in `PLAN.md`.

## 1. Assets
| Asset | Where it lives | Who may read it |
|---|---|---|
| Synced data: history, bookmarks, settings, tabs, pins, allowlist | Clients (plaintext in `browser.db`); server (XChaCha20-Poly1305 ciphertext) | Linked devices that hold AK |
| Synced passwords and never-save list | Clients (`vault.json`, under VK); server (ciphertext under PAK) | Devices whose vault holds PAK; the holder of the passphrase or recovery key |
| AK, PAK (epoch keys) | `sync.json` (AK, DPAPI or Keystore-wrapped); `vault.json` (PAK, under VK); server (HPKE-sealed only) | Clients only |
| RR (recovery root), passphrase, recovery key | The user's memory or paper; RR only in client memory during setup, restore or change; server (`rr_wrap`, AEAD under KEK) | The user |
| Device credentials | `sync.json` (refresh token); memory (access token); server (HMAC hashes only) | That device |
| Account password, TOTP secret | Server (Argon2id + pepper hash; TOTP secret encrypted at rest with `TOTP_ENC_KEY`) | Server checks only |
| Metadata | Server: email, device count, platforms and labels, record counts, sizes (padded buckets), collections, `seq`/timestamps, IPs (90 days), countries, user agents | Server operator |

**What the server can never see:**
- any key: AK, PAK, RR, collection keys, the passphrase or the recovery key;
- record contents: URLs, titles, bookmark names, passwords, usernames, origins, settings values
  and tab URLs;
- device names;
- per-record version or visit times, because `ver` is inside the ciphertext.

**What it can see** is the metadata row in the table above.

## 2. Attackers
1. **Server operator, or an attacker who controls the server or the database.** They can read,
   modify, withhold or replay anything stored, and serve arbitrary API responses.
2. **DB thief:** a copy of the Postgres volume or of a backup.
3. **Network attacker** between the client and Cloudflare, or between Cloudflare and the origin.
4. **Compromised website, serving malicious JS** on krunk.209.wf: via a server compromise,
   Cloudflare, or a malicious deploy.
5. **Stolen or lost device.** Before revocation it holds AK and maybe PAK in its vault. After
   revocation it is the "revoked device".
6. **Phishing of link codes:** an attacker gets the user to approve the attacker's device on
   the website, or tricks them with a look-alike domain.
7. **Credential stuffing and password guessing** against website accounts.
8. **Same-user malware** on a linked device. It can read files and call DPAPI as the user.
9. **Malicious web page** in Krunk: script on any site the user visits.
10. **Rollback or forking server:** serves older states, or different states to different
    devices.

## 3. Mitigations, attacker by attacker
| # | Attack | Mitigation | Spec |
|---|---|---|---|
| 1 | Read synced data | Everything is encrypted on the client with keys the server never has. Seals use HPKE to device keys or the escrow key, and RR is wrapped under KEKs derived from secrets only the user has. | PROTOCOL §2, §4, §5 |
| 1 | Substitute keys: give a device an attacker-known AK/PAK, or add the attacker's public key | Directories are authenticated by `mac`/`chain` under the epoch keys, or by `escrow_mac` under RR. The server alone holds none of these. `esc_pk` and `pp_kdf` are PAK-authenticated and pinned. A restorer re-derives `esc_pk` from RR and compares it. Device approval uses a committed SAS that the user types into the new device. | §4.4, §5.3, §6.2, §6.3 |
| 1 | Weaken the passphrase KDF | Hard-coded floors (m ≥ 128 MiB, t ≥ 3); re-wraps always use the constants. | §5.1 |
| 1 | Modify, swap or move records | AEAD with AAD binding the version, account, collection, id, key kind and epoch. Per-record `ver` high-water marks. `min_record_epoch`. Fork alarm on an AEAD failure at a known epoch. | §9.1, §10.1, §7.6 |
| 1 | Delete or withhold data | Possible (denial of service). Clients never delete locally because a record is missing: only authenticated tombstones delete. | §9.4, residual R3 |
| 2 | Offline attack on `rr_wrap_pp` | A generated 6-word passphrase (77 bits) or a custom one of ≥ 10^17 estimated guesses, under Argon2id 128 MiB/t=3. The recovery key is 160 bits (HKDF; brute force infeasible). The pepper doesn't apply (it's client-side crypto). An optional OPRF hardening phase is planned. | §5.1, PLAN S11 |
| 2 | Crack account passwords | Argon2id (m=64 MiB, t=3, p=1) over HMAC-SHA256(pepper, password). The pepper lives in `.env`, not in the DB, so a DB-only thief can't test guesses. Even a cracked account password gives no data keys. | PLAN §4.2 |
| 2 | Replay tokens from the DB | Only HMAC hashes of tokens and codes are stored, so a DB copy yields no usable token. | §11.4, PLAN §4.3 |
| 3 | Read or alter traffic | TLS 1.2+ (WebPKI) from the client to Cloudflare, and Cloudflare to the origin per the operator's setup. The payloads are E2E ciphertext anyway. Tampering is caught by AEAD and MACs. | PLAN §4.4 |
| 4 | Steal keys via website JS | **Keys never pass through the website.** The passphrase and recovery key are entered only in `krunk://sync` (Krunk's trusted UI, served from an origin web content can't reach). The website has no API that returns `rr_wrap`, seals or directories (device auth only). | §1, §5.1, §11.2 |
| 4 | Capture the account password, approve a rogue link, revoke devices, delete the account | Possible: the website does see the account password. Damage limits: a rogue link gets **no keys** (it shows as "Linked — no keys yet" and an email is sent). Revocation and deletion are visible and emailed. CSP stays `script-src 'self'` (plus the Turnstile origin on five pages), which narrows injection. | §6.1, §11.2, PLAN §4.6 |
| 4 | Phish the sync passphrase on a fake or compromised page | UI copy everywhere (the canonical string): "The Krunk website never asks for your sync passphrase or recovery key." Entry happens only in `krunk://sync`. | §5.1 |
| 5 | Stolen device, **not yet revoked** | It holds AK (DPAPI) and reads the AK collections. Passwords need its vault's master password. The user revokes it from the website or another device. | §7 |
| 5 | Revoked device | Tokens are deleted at once, and the server rejects every request from it. AK and PAK rotate (automatically, by a remaining device). Eager rekey under new ids; `min_record_epoch` stops old-epoch injection. Writes are frozen until AK rotation, so nothing new lands under the compromised epoch. Devices `added_by` it are flagged for the user. | §7 |
| 6 | Link-code phishing | `user_code` lives 10 minutes and is single-use. The approval page shows the device label, platform, country and request age, with an explicit warning, a confirmation step and re-auth. An email is sent for each link. **A link alone yields no keys**: keys need the passphrase or recovery key, or SAS approval from a linked device. | §6.1 |
| 6 | Approval phishing: the attacker starts a device approval and the user approves it on device A | A shows N's label, platform and country, and the user must read the SAS on A and **type it into N**. An attacker who doesn't physically control N can't complete it. Passwords travel only after a fresh master-password entry and an explicit tick. At most 5 approvals a day, each emailed. | §6.2 |
| 7 | Credential stuffing and guessing | Turnstile on login, register, reset and resend (server-side `siteverify` with hostname and action checks, single use). Per-IP limits (trusted client IP only) and per-account limits with exponential back-off. Generic errors. Optional TOTP. A breached-password check is optional (OPEN_QUESTIONS). | §11.2, PLAN §4.5 |
| 8 | Same-user malware, vault locked | It can read `sync.json` (AK) and so the AK collections, as with any synced browser profile. It **can't** get PAK: PAK lives only in `vault.json` under VK, and escrow needs RR. It can't add a vault key (that needs a PAK MAC) or change `esc_pk` (PAK-authenticated and pinned), and it can't roll back PAK pins (they're stored in the vault). | §4.3, §4.4, §8 |
| 8 | Same-user malware, vault unlocked | Out of scope, as for the local vault (it can read process memory). | SECURITY.md |
| 9 | Malicious web page | Sync runs in the browser process. Web content can't send sync or vault commands: the `Sender::permits` allowlist covers built-in pages only, and `krunk://sync` is a new allowlisted page. | PLAN §5.4 |
| 10 | Rollback or fork | Clients persist high-water marks (directory `seq` and head hash, epochs, per-record `ver`) and refuse older states. `prev_hash` chains expose forks, which trigger an alarm and stop writes. **A first restore can't detect** a rollback to an older consistent state (residual R4). | §4.4, §9.1 |

## 4. Guarantees
- **G1 (confidentiality):** without the cooperation of a key holder (a linked device, or the
  holder of the passphrase or recovery key), none of the server, a DB thief, the network or the
  website learns any key or any record plaintext.
- **G2 (integrity):** the server can't forge or alter records or directories so that a client
  accepts them. It can only withhold, delete or roll back.
- **G3 (revocation):** against a revoked device and an honest server, the device loses access to
  the account at once (tokens), and cryptographically to all data written after the rotation (no
  `AK_{e+1}`/`PAK_{p+1}`).
- **G4 (passwords):** the synced passwords are never readable with only what is on disk under
  DPAPI or Keystore (`sync.json`). Reading them needs the vault master password, the sync
  passphrase or the recovery key, so they are never weaker than the local vault.
- **G5 (opt-in):** with sync off (the default), Krunk makes no sync request and creates no sync
  state.
- **G6 (password reset):** resetting the website password never touches keys or synced data.
  It can't, because the server has no keys.

## 5. Residual risks and out of scope
- **R1, a revoked device colluding with a malicious server.** The server can ignore the
  revocation. Before re-anchoring, it can also forge an epoch chain using the revoked device's
  old `AK_e`, and serve it to devices that were offline during rotation or to restorers.
  - **Mitigations:** the rotation starts on the device that revokes when possible. Restorers
    show the device and revoked lists and require confirmation. Fork alarms. "Finish securing:
    enter your passphrase" writes a new escrow anchor at the rotated heads, which the revoked
    device can't forge (it never had RR).
  - A stateless restorer can't detect a forgery made before that anchor exists.
- **R2, same-user malware with the vault locked** reads the AK collections. It is accepted, as
  in every synced browser. Passwords stay protected (G4).
- **R3, deletion and withholding** by the server: no defence beyond detection by devices that
  have local state. Users keep local copies; there are no server-side backups of plaintext.
- **R4, rollback on a first restore** to an older, consistently signed state.
- **R5, weak user-chosen passphrase:** mitigated by the generated default and the strength gate,
  but a determined user can still choose a memorable one that just meets the gate.
- **R6, metadata:** the server learns the account email, device count, platforms, collection
  sizes (in padded buckets), the timing of syncs, and IPs (kept 90 days). History record ids are
  stable per URL within an epoch, so the server can tell that "the same record changed again"
  without knowing which URL it is.
- **R7, Cloudflare** terminates TLS and sees request metadata and the website's traffic,
  including account passwords typed on the website. It is equivalent to attacker 4 for the
  website, and it never sees keys or plaintext records.
- **R8, total loss:** if the passphrase, the recovery key and every device are all lost, the
  synced data is **unrecoverable**, by design. The setup UI states this before the keyset is
  created, and again when the recovery key is shown.
- **Out of scope:**
  - **Same-user malware while the vault is unlocked**, and **keyloggers**.
  - **A compromised Krunk binary.** Updates are signed separately; see SECURITY.md "Update check".
  - **Traffic analysis** beyond padding buckets.
  - **Quantum attackers.** The classical X25519 in HPKE could later move to the hybrid ML-KEM
    suite in RFC 9180's registry; noted for `v = 2`.

## 6. Scenario checks (also integration tests, PROTOCOL §12 item 13)
| Scenario | Expected result |
|---|---|
| Every device lost, the user has the passphrase | Restore AK and PAK via escrow (§6.3). Passwords return after the vault is created on the new device. |
| Every device lost, the user has only the recovery key | The same as with the passphrase (`KEK_rk`). |
| Everything lost | Unrecoverable. The user may `web/sync/reset` and start over. |
| A revoked device keeps polling with old tokens | 401 everywhere. After rotation the records are under `e+1` and it can't decrypt them. |
| The server sends a directory with an injected public key under the right `seq` | `mac`/`chain` fail: fork or tamper alarm, writes stop. |
| Malware with only `sync.json` writes `pak_dir` with its own vault key | Rejected: no `mac` under PAK. Devices without PAK forward the bytes untouched. |
| Malware swaps `esc_pk` in `ak_dir` | Not possible: `esc_pk` lives only in the PAK-authenticated `pak_dir`, and is pinned. |
| Revocation while no other device is online | Writes are frozen and the revoked device is locked out. Local queues persist. Rotation runs on the next remaining device or restorer. |
| Two devices rotate at the same time | CAS on `prev_hash`: one commit wins, the loser discards its key and adopts the winner. |
| A website password reset | Web sessions revoked, devices stay linked, keys and data untouched, email sent. |
