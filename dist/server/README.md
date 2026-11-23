# Krunk download and update server

A single Caddy container (`caddy:2.11.8-alpine`, pinned by digest) that serves:

| Path | What | Caching |
|---|---|---|
| `/` | the website (`site/`, baked into the image) | `no-cache` |
| `/changelog` (0.13.0) | every release's notes, generated from the repository's `CHANGELOG.md` when the image is built | `no-cache` |
| `/pair` (0.13.0) | the phone's page for "Add a device" (`site/account/pair.html`); the token stays in the URL fragment | `no-store` |
| `/download/release.json` | the website's download box | `no-cache` |
| `/download/<version>/…` | `Krunk-Setup-<version>.exe`, `Krunk-win-x64.zip` (as attachments), `NOTES.md`, `SHA256SUMS` | 1 year, `immutable` |
| `/download/android.json` (Android 0.1.0) | the website's Android download box | `no-cache` |
| `/download/android/<version>/…` | `Krunk-android-<version>-<abi>-release.apk` (as attachments, `application/vnd.android.package-archive`), `NOTES.md`, `SHA256SUMS` | 1 year, `immutable` |
| `/updates/win-x64/latest.json`, `latest.json.sig` | Krunk's update check (`core/src/update.rs`) | `no-cache` |
| `/healthz` | `ok` | `no-store` |

Every response has these security headers:
- `Content-Security-Policy` (only this origin)
- `nosniff`
- `Referrer-Policy: no-referrer`
- `X-Frame-Options: DENY`
- `Permissions-Policy`
- `Cross-Origin-Resource-Policy`/`-Opener-Policy`
- HSTS (ignored over plain HTTP)

There is no `Server` header, directory listing or dot-file access. Caddy writes no access log (no
`log` directive), so client IPs aren't stored; its own error and status log goes to the capped
`json-file` log.

The container:
- runs as uid 10001 with every capability dropped and `no-new-privileges`;
- has a read-only root filesystem and the release volume mounted read-only;
- uses `restart: unless-stopped`, a health check, and `json-file` logs capped at 3 × 10 MB.

## Files
- `compose.yaml`: the service. It publishes **no host ports**; ai-pipe or a reverse proxy does that. The repository root's `compose.yaml` includes it, and `service.json` names the entry for ai-pipe.
- `compose.ports.yaml`: publishes ports for standalone use.
- `Dockerfile`, `Caddyfile`, `site/`: the image. Since 0.13.0 its build context is the
  **repository root** (compose: `context: ../..`, `dockerfile: dist/server/Dockerfile`), for
  `CHANGELOG.md`. `Dockerfile.dockerignore` is a whitelist: only `CHANGELOG.md`, `changelog.py`,
  `stamp-assets.py`, the Caddy files and `site/` are sent, never `target/`. By hand, from the root:
  `docker build -f dist/server/Dockerfile .`
- `stamp-assets.py`: the last step of the site build stage. It adds a content hash to every
  link from a page to the site's own scripts, styles and images (`/app.js?v=4cdedc915a`), so
  write those links plainly in `site/` (`/app.js`). Reason: Cloudflare replaces `no-cache` on
  such files with its own four-hour browser cache, so after a deploy visitors got the new page
  with the old script (on 2026-11-11 that kept the new Android download box hidden). The pages
  themselves are not cached. A page that links to a missing script or style sheet fails the build.
- `changelog.py`: the one `CHANGELOG.md` parser (standard library only). A pinned
  `python:3.13-alpine` build stage runs `check` (a malformed file fails the build) and `render`
  (`/changelog`); `publish-release.sh` uses `section` and `date`.
- `.env.example`: settings. Copy it to `.env`; it contains no secrets.
- `gen-signing-key.sh`: makes the release key pair.
- `backup-db.sh`, `restore-db.sh`, `update-cloudflare-ips.sh` (+ the generated
  `cloudflare-*.caddy`): the account and sync API (below).
- `publish-release.sh`: signs and publishes a release.
- `publish-android.sh`: checks and publishes a Krunk for Android release (below).

Volumes:
- `krunk-releases`: published releases (the container's `/srv/releases`).
- `caddy-data` and `caddy-config`: certificates and state for TLS mode.

Compose prefixes the volume names with the project name, e.g. `krunk_krunk-releases`.

## Run it
Local or behind a reverse proxy (plain HTTP on port 8080):
```sh
cd dist/server
cp .env.example .env
docker compose -f compose.yaml -f compose.ports.yaml up -d --build
curl http://localhost:8080/healthz
```
The website shows "No release published yet" until the first release is published.

With a domain (automatic HTTPS from Let's Encrypt):
1. Point the domain's DNS at the host.
2. In `.env`, set `SITE_ADDRESS=downloads.example.org`, `HTTP_PORT=80` and `HTTPS_PORT=443`. Optionally set `CADDY_GLOBAL_OPTIONS=email you@example.org`.
3. Run the same `up` command.

Caddy keeps its certificates in the `caddy-data` volume. Inside the container it listens on 8080 and 8443, so it needs no privileges.

Then build Krunk for that server: `UPDATE_BASE_URL=https://downloads.example.org scripts/build-dist.sh`. The default build points at the live server `https://krunk.kruxsys.com`.

### The live deployment
`https://krunk.kruxsys.com` (a temporary domain) is the ai-pipe service of this repository behind
**Cloudflare**:
- Cloudflare terminates TLS and redirects HTTP to HTTPS, and proxies to Caddy's plain port
  8080, so `SITE_ADDRESS` stays `:8080`.
- Releases are published into the volume `live-browser_krunk-releases`.
- Cloudflare caches the `.exe` and `.zip` downloads. That is safe because their paths are
  per-version and `immutable`.
- It does not cache `.json`, so `latest.json`, sent with `no-cache`, reaches clients at once.
  Check it with `curl -sI https://krunk.kruxsys.com/updates/win-x64/latest.json` (`cf-cache-status`).
- Cloudflare sees the requests, and so the client IPs. It cannot forge an update: Krunk
  checks the signature and the installer's hash.
- Operator provisioning so far (0.6.0): the live `.env` holds the Turnstile keys for
  `krunk.kruxsys.com` (step 1–2 of docs/sync/OPEN_QUESTIONS.md "Operator provisioning"), nothing
  else; `/api/v1/web/config` shows the site key with `accounts_enabled: false` until SMTP
  (step 3) is set. A changed `.env` takes effect with the next deploy (containers are
  re-created; a plain restart keeps the old environment).

### The site knows Krunk (0.6.0)
Krunk leaves a frozen `window.krunk = { version }` in the main frame of pages on its update
server's origin (built in at build time: `UPDATE_BASE_URL`, the live site), and nowhere else.
`site/app.js` reads it next to `/download/release.json` and the download box says "You're
using Krunk X — up to date" or "— version Y is available". Nothing depends on the
User-Agent (which users may edit), other sites never see the marker, and the page's script
stays external (`script-src 'self'`, no inline script). Other browsers get the download box
as before.

## Account and sync API (`api`, `db`, `secrets`, `backup`)
The account and sync API (`server/`, the `krunk-sync` binary) and its PostgreSQL 18.6
database are defined here too. Design: `docs/sync/PLAN.md`; decisions:
`docs/sync/OPEN_QUESTIONS.md`.

- `secrets` creates the server's own secrets on first start (`krunk-sync init-secrets`):
  `postgres_password`, `token_hmac_key`, `password_pepper` and `totp_enc_key`, each 32 random
  bytes in base64url, in the volume `krunk-secrets`. It never overwrites one, then idles.
  `db`, `api` and `backup` mount the volume read-only. No secret goes into `.env`, git or an
  image, and none is logged. **Deleting `krunk-secrets` while keeping `krunk-db` locks `api`
  out of the database** (the password is set only when the database is created).
- `api` runs its migrations at start; the migrator holds a Postgres advisory lock, so two
  instances can't migrate at once.
- **Rollback safety (0.13.0):** an `api` before 0.13.0 refuses to start when `_sqlx_migrations`
  lists a version it doesn't know. So 0.13.0's schema change is an **expand step**
  (`server/expand/0007_account_section.sql`: additive, idempotent), applied after the sqlx
  migrations under an advisory lock and recorded in `krunk_schema_steps`; `_sqlx_migrations` still
  ends at 6. The previous `api` image starts against the migrated database (shown on staging).
  From 0.13.0 on the migrator ignores unknown applied versions, so later sqlx migrations roll back
  too (docs/sync/PLAN.md §4.9).
- `web` doesn't depend on `api`: if the API is down, `/api/*` answers a JSON 502, and the
  site, downloads and updates are unaffected.
- The only operator settings are in `.env` (see `.env.example`): the Turnstile keys and SMTP.
  Without them, accounts stay **closed**: `/api/v1/web/config` reports
  `accounts_enabled: false`, the account endpoints answer `503 accounts_disabled`, and the pages
  at `/account/` say "Accounts are coming soon". Since 0.6.0 the config hands out
  `turnstile_site_key` as soon as the Turnstile keys are set, SMTP or not, so the bot check can
  be seen on the forms while registration still answers 503 (the provisioning steps show one
  by one). Setting both and restarting opens them; no code
  change is needed (docs/sync/OPEN_QUESTIONS.md, "Operator provisioning"). `api` reads them
  through `env_file`: `.env` (this folder's, then the repository root's; on ai-pipe the live
  `.env`), then `staging.env` (below). On ai-pipe the live `.env` is copied from the working
  tree only while live has none, then kept; so the working tree's `.env` holds comments only.
  Test values (Cloudflare's test secrets, `SMTP_TLS=none`, an `http://`/`.test`/`.localhost`/
  `.invalid` PUBLIC_BASE_URL) keep accounts closed unless `ACCOUNTS_TEST_MODE=1`.

**Accounts (milestone S2).** Register, confirm by email, sign in, sign out, reset or change the
password, two-step verification (TOTP plus backup codes). The pages are static
(`site/account/`, `site/privacy.html`, one `account.js`); only the form pages (sign-in,
registration, reset, resend and, since 0.13.0, `/account/delete`) may load Cloudflare Turnstile,
and only when accounts are open. Mail goes through an outbox in Postgres, sent by a worker in
`api`; templates are in `server/templates/`.

**The account section (0.13.0).** "Add a device" by QR (`/pair`, docs/sync/PROTOCOL.md §6.5),
Usage, Activity with full IPs, the data download, sync reset and account deletion
(PROTOCOL §11.2). New mails: `qr_sign_in`, `sync_reset`, `account_deleted`.

**One-step registration (service 2026-11-17).** Register → click the link in the mail → signed
in. When the registration started from Krunk's **Sign in** (the `/link?code=…` tab), that Krunk
is signed in to the new account too, with no further sign-in, code check or password.
- **`next` is kept.** The sign-in and registration pages pass `?next=` to each other, and only
  paths the site's `safeNext()` accepts (`/link?code=…`, `/account/`, `/account/#add-device`,
  `/account/#export`, `/account/delete`). The server keeps the same list (`accounts.rs::safe_next`).
- **`POST web/register`** `{email, password, turnstile, next?, link_code?}` → 202
  `{status:"check_email", reg_poll}` and the cookie **`__Host-krunk_reg`** (random, HttpOnly,
  Secure, `SameSite=Lax` so it comes along on the click from the mail client, 24 hours; only its
  HMAC is stored). Every address gets the same answer, cookie, rows and work, including an
  already verified address, whose registration simply never completes. So nothing reveals whether
  an account exists. A later registration of the same unverified address voids the earlier ones,
  as it already replaced their password.
- **`POST web/register/poll`** `{reg_poll}` → 400 `authorization_pending` until the address is
  confirmed. Then, once, 200 with a website session (cookie + `{csrf, next, link, linked}`);
  afterwards, or when unknown or expired, 400 `expired_token`. It is limited to 1500 an hour per IP
  (`reg-poll/ip`). The "check your email" card polls every 3 s for 10 minutes, then every 15 s for
  the link's 24 hours, and waits longer on `slow_down`/429. A resent mail completes the same
  registration.
- **`POST web/verify-email`** `{token, password?}` → 200 `{csrf, next, link, linked}` with a
  website session, so verifying signs you in. In the browser that registered (its
  `__Host-krunk_reg` matches a live, not voided registration of this very account) no password is
  needed. Anywhere else the first try answers 401 `password_required`, without spending the link,
  and the page asks for the password once. A password that is sent must be right, even in the
  registering browser. The pre-registration hijack defence is unchanged: only someone holding both
  the mailbox and the password (or the registering browser) confirms.
- **Auto-link.** `link_code` is attached to the registration only if:
  - the grant is pending and unexpired;
  - the grant was asked for by Krunk's own client (the user agent stored at `link/authorize`
    parses as `Krunk/x.y`);
  - the register request comes from the **same client IP** as the grant.

  Otherwise the code is ignored and `/link` works as always. This is checked on the grant's user
  agent, not the register request's, because Krunk's tabs send the engine's user agent (Chrome on
  Windows, GeckoView on Android). The same-IP check is the one that stops a `/link` URL sent to
  someone else (device-code phishing).
  - An attached grant waits an hour (`AUTO_LINK_TTL_S`), and `link/token` keeps answering
    `authorization_pending`. The extension is done for every address alike.
  - Verifying approves it for the new account through the shared `approve_grant`: device quota,
    `short`, device row, the `device_linked` mail and the event. This runs in a savepoint inside
    the verification's transaction. If the grant expired or was denied meanwhile, or the quota is
    full, the address is still confirmed and `link` is `"unavailable"`: the page says "Open Krunk →
    Settings → Account → Sign in to finish", and with the fresh session that is one click on `/link`.
  - Krunk apps up to 0.23.0 / Android 0.10.0 stop polling after the code's own 10 minutes. With
    those, a slower confirmation ends the same way.
- **Re-auth right after a sign-in.** Sign-ins by password (`web/login`, verification, the
  registration poll) start with `reauth_at = now()`. For 10 minutes after signing in, the actions
  that need a recent password therefore don't ask again: `/link` approval, "Add a device", removing
  devices, "sign out everywhere else". The manual `/link` path still needs the code ticked.
- **Data:** migration `0010_registrations.sql` adds the `registrations` table only. Housekeeping
  deletes registrations a day after they expire or complete. Activity shows `email_verified`.
- **Password entries:** registering from Krunk = 1 (the register form), or 2 when the mail is
  opened in another browser. An existing user signing in from Krunk = 1.

**Website sign-in from a signed-in Krunk (service 2026-11-17, Krunk 0.24.0 / Android 0.11.0).**
Opening the website in a Krunk that is signed in to the account signs the website in too.
- A visit without a website session ends on `/account/login`. When Krunk (signed in, not a
  private tab, not right after a website sign-out) sees that navigation on its own site, it calls
  **`POST device/web-sign-in`** (bearer) → `{code, expires_in: 60}` and opens
  `/account/login?auto=<code>` (keeping `next`). Only the code's HMAC is stored
  (`web_sign_in_codes`); it is single use and dies with the device. Limits: 30 codes an hour per
  device, 60 redemptions an hour per IP.
- The page takes the code out of the address bar and sends **`POST web/device-sign-in`**
  `{code}` → 200 `{csrf}` + a website session with `via = 'device'` and `via_device`, so the
  signed-in browsers list says "Signed in by <device>", and Activity says "Signed in by Krunk"
  (`device_sign_in`). If the code is bad or expired, the normal form shows.
- Such a session starts **without** re-auth. Changing the password, two-step verification,
  removing devices or deleting the account still ask for the password, so a device token never
  gives full control of the account.
- Signing out on the website goes to `/account/login?signed_out=1`; Krunk then leaves the site
  signed out until it restarts.
- Migration `0011_device_web_sign_in.sql`: the code table, the nullable `web_sessions.via_device`
  column, and `via` allowing `'device'` (the check is only widened).

**Testing accounts** (staging or a local stack). The test values go into `staging.env` at the
repository root: gitignored, so never in a commit and never in a live deploy, which is a copy of a
commit plus its `.env`. The mail sink isn't in compose at all; a script attaches it to the
project's network:
```sh
cat > staging.env <<'END'
ACCOUNTS_TEST_MODE=1
PUBLIC_BASE_URL=https://krunk.test
TURNSTILE_SITE_KEY=1x00000000000000000000AA
TURNSTILE_SECRET_KEY=1x0000000000000000000000000000000AA
SMTP_HOST=mailpit
SMTP_PORT=1025
SMTP_TLS=none
END
# (stage or start the stack, then)
server/e2e/mailsink.sh up stage-browser      # "down" afterwards
# Rust unit + database integration tests (a throwaway Postgres):
docker run -d --name krunk-pgtest -e POSTGRES_PASSWORD=itpass -p 55432:5432 postgres:18.6-alpine3.24
KRUNK_TEST_DATABASE_URL=postgres://postgres:itpass@localhost:55432/postgres cargo test   # in server/
cargo test --release it::timing -- --ignored --nocapture                                  # timing, alone
# Every page in a browser (see the script's header):
NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules node server/e2e/accounts.cjs http://<host:port> <project> <shots dir>
# 0.13.0's flows only: changelog, Add a device, QR sign-in, usage, activity, export, deletion:
NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules node server/e2e/s10.cjs http://<host:port> <project> <krunk-sync-cli> <shots dir>
```
`s10.cjs` plays the Android app with the test client: `krunk-sync-cli pair <link or token>
--server <url> --dir <folder>` claims a QR code through `link/pair/claim`, prints the confirm code
and then waits for the approval like `link`.

**Client IP.**
- Caddy trusts `CF-Connecting-IP` only from Cloudflare's ranges (`cloudflare-trusted.caddy`,
  regenerated by `update-cloudflare-ips.sh`, dated) and from `TRUSTED_PROXIES_EXTRA`. That
  defaults to `10.250.137.4/32`, ai-pipe's proxy in front of krunk.kruxsys.com (OPEN_QUESTIONS Q8).
  Standalone deployments set their own hop, or `TRUSTED_PROXIES_EXTRA=` for none.
- It forwards the result to `api` as `X-Krunk-Client-IP`, and strips `CF-IPCountry` from
  untrusted peers.
- `api` believes those headers only when the TCP peer is the `web` container.
- **Deployment requirement (0.11.7):** the hop in `TRUSTED_PROXIES_EXTRA` must overwrite or
  strip a `CF-Connecting-IP` header sent by the client. It must be reachable only through
  Cloudflare, or set the header itself. Otherwise anyone who reaches the hop directly picks
  their own client IP, and the per-IP rate limits and the IPs in the account's activity can be
  spoofed. The operator checks this for ai-pipe's proxy (audit S10).
- Per-IP rate limits count an IPv6 /64 as one client (0.11.7), and each IPv4 address on its own.

**Backups** (OPEN_QUESTIONS Q4):
- The `backup` service runs `pg_dump -Fc` at start and then every 24 hours into the volume
  `krunk-backups`, mode 600, keeping 14 days. Its health check fails if no dump is younger than
  26 hours. Those dumps stay on the same host as the database and are not encrypted.
- Restore the newest one: `dist/server/restore-db.sh --project <compose project> --latest`.
- Copies that leave the host must be encrypted:
  ```sh
  dist/server/backup-db.sh  --project <compose project> --out <dir> --age-recipient <age public key>
  dist/server/restore-db.sh --project <compose project> --dump <file.dump.age> --age-identity <key file>
  ```
- `restore-db.sh` stops `api`, runs `pg_restore --clean --if-exists --single-transaction` and
  starts `api` again. Since 0.11.7, a failed restore leaves the database as it was and still
  starts `api`. `backup-db.sh` writes to `.part` and renames only a complete dump, and creates
  the output folder private (umask 077).
- **Deleted accounts stay deleted (0.13.0):**
  - The `backup` service also exports the ids of deleted accounts (`account_deletions`) every
    hour into `deletions.log` in `krunk-backups` (merged, `<id> <unix time>` per line, 45 days).
  - `backup-db.sh` writes the same list next to the dump, `krunk-<project>-<time>.deletions`
    (`.age` when the dump is encrypted).
  - `restore-db.sh` first merges the live database's list (and `--deletions FILE[.age]`, the list
    saved with an off-host dump) into `deletions.log`, restores, then deletes those accounts again
    ("accounts deleted again: n"). Restore an off-host copy with its list:
    `restore-db.sh --project <p> --dump <file.dump.age> --deletions <file.deletions.age> --age-identity <key file>`.
  - Drill on staging, 2026-11-10: an account deleted after the dump stayed deleted after
    `--latest`.
- Drills on throwaway stacks, 2026-11-05: with `backup-db.sh` (insert a marker, back up,
  `down -v`, `up`, restore, the marker is back), and with `--latest` from the `backup` volume.
  The first was unencrypted, because `age` isn't installed on the build machine.

## The release key
```sh
dist/server/gen-signing-key.sh   # -> ~/.config/krunk/update-signing.key (mode 600)
                                 #    and assets/keys/update-signing.pub (commit this)
```
- Krunk trusts only the public key it was built with.
- Keep the private key on the publishing machine and back it up offline. Never put it on the server, in the image, a container or git.
- Whoever holds it can make every Krunk install run their program. If it is lost, installed copies can't be updated any more.
- The script refuses to overwrite an existing key.
- To rotate the key, publish one last release signed with the old key whose build trusts the new key.

See `docs/SECURITY.md`, "Update check".

## Publish a release
```sh
scripts/build-dist.sh
dist/server/publish-release.sh dist/Krunk-Setup-0.13.0.exe dist/Krunk-win-x64.zip 0.13.0 \
    --volume krunk_krunk-releases
```
**Release notes (0.13.0):** without `--notes`, the script takes the version's
`## <version> — <YYYY-MM-DD>` section of the repository's `CHANGELOG.md` as `NOTES.md`, and its
date as the release date (unless `--released` is given). It **fails** when the section is
missing or malformed, so add it before publishing. `--changelog FILE` reads another file;
`--notes FILE` uses a Markdown file as before. Needs `python3` (`changelog.py section`/`date`,
the parser the `/changelog` page is built with). The page itself changes only with the next
build of the `web` image.

1. The script computes sizes and SHA-256s and writes `release.json` and `latest.json`.
2. It signs `latest.json` **on this machine** (`openssl pkeyutl -sign -rawin`, Ed25519) and checks the signature again with the public key.
3. It copies everything in, in this order:
   - into a Docker volume through a throwaway, pinned `alpine` container with no network (`--volume NAME`), or into a plain folder (`--dest DIR`);
   - the release files first, then `release.json`;
   - the signature and manifest last, each renamed into place.

A client never sees a manifest for files that are not there yet.

Options:
- `--key PATH`: another private key.
- `--min-supported VERSION`: the oldest version the installer updates in place (default 0.2.0). Older copies are sent to the website instead.
- `--released YYYY-MM-DD`: the release date (default: the changelog section's date, else today, UTC).
- `--changelog FILE` / `--notes FILE`: where `NOTES.md` comes from (above).

Old versions stay under `/download/<version>/` until you remove them from the volume.

The manifest format:
```json
{"version":"0.2.1","released":"2026-11-05","url":"/download/0.2.1/Krunk-Setup-0.2.1.exe",
 "sha256":"…","size":143620406,"notes_url":"/download/0.2.1/NOTES.md","min_supported":"0.2.0"}
```
`latest.json.sig` holds the base64 of the 64-byte Ed25519 signature over the exact bytes of
`latest.json`.

## Publish an Android release
```sh
(cd android && ./gradlew assembleDirectRelease)   # in a fresh clone; signs with the Android release key, docs/ANDROID_DEV.md
# copy the APKs to dist/android/Krunk-android-<version>-<abi>-release.apk, then:
dist/server/publish-android.sh 0.1.1 --volume live-browser_krunk-releases
```
- `--volume` takes the releases volume of the running stack: `<compose project>_krunk-releases`.
  On the ai-pipe deployment that is `live-browser_krunk-releases` (a stack started by hand as
  project `krunk` has `krunk_krunk-releases`). The volume must exist: the script refuses a name
  Docker does not know instead of letting Docker create an empty volume for it.
- The script takes the three APKs (`arm64-v8a`, `universal`, `x86_64`) from `dist/android`
  (`--apks DIR` for another folder).
- It checks every APK before it copies anything. With `aapt2`: the package is Krunk's, the
  `versionName` is the version being published, and the build is not debuggable. With
  `apksigner`: APK Signature Scheme v2 or v3,
  exactly one signer, and the certificate of the release keystore
  (`~/.config/krunk/android-release.properties`, or `--cert-sha256 HEX` on a machine without the
  key). A debug-signed APK is refused.
- A version that is already in the store is refused (exit 3): its files would be replaced in
  place while the edge cache may still serve the old ones. `--force` publishes it again anyway.
- Release notes and date come from the version's section of `CHANGELOG-android.md`
  (`--changelog FILE`, `--notes FILE`, `--released YYYY-MM-DD` as for Windows).
- It writes `download/android/<version>/` first and `download/android.json` last, renamed into
  place. The website shows the Android box only when that file exists.
- `android.json`: `version`, `released`, `notes`, `package`, `min_android`, `min_sdk`,
  `signing_cert_sha256` and `files` (`name`, `abi`, `kind` = `device` / `universal` /
  `emulator`, `label`, `url`, `size`, `sha256`). `site/app.js` makes the `device` file the button.
- Publish only to a server whose `web` image has the `/download/android.json` route: an older
  image would serve that file with the one-year `immutable` header of `/download/*`.
- **The in-app update manifest (0.5.0):** the script also writes
  `updates/android-arm64/latest.json` and `latest.json.sig`. It signs them on this machine with
  the update key (`--key`, default `~/.config/krunk/update-signing.key`: the same Ed25519 key and
  scheme as Windows, base64 of the signature over the exact bytes). It then checks the signature
  against `assets/keys/update-signing.pub`, the key compiled into the app. Once everything is
  copied, it reads each written file back from the volume and compares the SHA-256s. It refuses
  to run if:
  - the update key is missing, or is not that key;
  - the APK keystore is missing (`--cert-sha256` may only repeat its fingerprint);
  - an APK has no `REQUEST_INSTALL_PACKAGES` (the website APKs are the `direct` flavour).

  Write order: APKs, `NOTES.md` and `SHA256SUMS`, then `android.json`, then the signature, then
  the manifest. The Windows manifest, its signature and `download/release.json` are never
  touched.
- Manifest (one line, unknown fields ignored by the app, `core/src/update_android.rs`):
  ```json
  {"channel":"android-arm64","version":"0.5.0","version_code":7,"released":"2026-11-14",
   "package":"org.krunk.browser","min_sdk":26,"signing_cert_sha256":"c94b8915…e624",
   "notes":"<the changelog section>","notes_url":"/download/android/0.5.0/NOTES.md",
   "files":[{"abi":"arm64-v8a","url":"/download/android/0.5.0/Krunk-android-0.5.0-arm64-v8a-release.apk","size":…,"sha256":"…"},
            {"abi":"universal",…},{"abi":"x86_64",…}]}
  ```
  The app fetches it only when the user taps "Check for updates". It offers a version only if
  `version_code` is higher than its own, picks the file for its ABI (universal only as a
  fallback), and installs only an APK that has the listed size and SHA-256, is this package and
  `version_code`, and carries the installed app's certificate. Caddy serves `/updates/*` with
  `Cache-Control: no-cache, no-transform` and the JSON/text content types, so Cloudflare
  revalidates it instead of keeping it for hours.
- **Keys:** the Android release keystore (`~/.config/krunk/android-release.jks`) signs the APKs.
  The update key signs the manifests of both platforms. Both stay on the publishing machine, and
  neither goes into the image, the volume, compose or git. Offline backups of both are still the
  operator's to make.
- **Backup of the release volume:** it holds every published file (Windows and Android) and the
  manifests, and none of it can be rebuilt byte for byte without the keys. An encrypted copy:
  ```sh
  docker run --rm --network none -v live-browser_krunk-releases:/r:ro \
    alpine:3.22@sha256:2e0b81d60597f449baa56dc25d355367869d2dca4cd57e866f1e2b52af3eb55f \
    tar -C /r -cf - . | age -r <age public key> > krunk-releases-$(date -u +%F).tar.age
  ```
  To restore, run `age -d -i <key file> … | docker run -i … tar -C /r -xf -` into the volume. A
  deploy does not touch the volume: ai-pipe changes the image and compose only.

## Checks
```sh
hadolint dist/server/Dockerfile
python3 -I dist/server/changelog.py check CHANGELOG.md
yamllint compose.yaml dist/server/compose.yaml dist/server/compose.ports.yaml
docker compose config -q
```
