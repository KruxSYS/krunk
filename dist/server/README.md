# Krunk download and update server

A single Caddy container (`caddy:2.11.8-alpine`, pinned by digest) that serves:

| Path | What | Caching |
|---|---|---|
| `/` | the website (`site/`, baked into the image) | `no-cache` |
| `/download/release.json` | the website's download box | `no-cache` |
| `/download/<version>/…` | `Krunk-Setup-<version>.exe`, `Krunk-win-x64.zip` (as attachments), `NOTES.md`, `SHA256SUMS` | 1 year, `immutable` |
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
- `Dockerfile`, `Caddyfile`, `site/`: the image.
- `.env.example`: settings. Copy it to `.env`; it contains no secrets.
- `gen-signing-key.sh`: makes the release key pair.
- `backup-db.sh`, `restore-db.sh`, `update-cloudflare-ips.sh` (+ the generated
  `cloudflare-*.caddy`): the account and sync API (below).
- `publish-release.sh`: signs and publishes a release.

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

Then build Krunk for that server: `UPDATE_BASE_URL=https://downloads.example.org scripts/build-dist.sh`. The default build points at the live server `https://krunk.209.wf`.

### The live deployment
`https://krunk.209.wf` (a temporary domain) is the ai-pipe service of this repository behind
**Cloudflare**:
- Cloudflare terminates TLS and redirects HTTP to HTTPS, and proxies to Caddy's plain port
  8080, so `SITE_ADDRESS` stays `:8080`.
- Releases are published into the volume `live-browser_krunk-releases`.
- Cloudflare caches the `.exe` and `.zip` downloads. That is safe because their paths are
  per-version and `immutable`.
- It does not cache `.json`, so `latest.json`, sent with `no-cache`, reaches clients at once.
  Check it with `curl -sI https://krunk.209.wf/updates/win-x64/latest.json` (`cf-cache-status`).
- Cloudflare sees the requests, and so the client IPs. It cannot forge an update: Krunk
  checks the signature and the installer's hash.

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
- `web` doesn't depend on `api`: if the API is down, `/api/*` answers a JSON 502, and the
  site, downloads and updates are unaffected.
- The only operator settings are in `.env` (see `.env.example`): the Turnstile keys and SMTP.
  Without them, accounts stay **closed**: `/api/v1/web/config` reports
  `accounts_enabled: false`, the account endpoints answer `503 accounts_disabled`, and the pages
  at `/account/` say "Accounts are coming soon". Setting both and restarting opens them; no code
  change is needed (docs/sync/OPEN_QUESTIONS.md, "Operator provisioning"). On ai-pipe the
  deployed `.env` is the working tree's, copied with each commit: never leave staging values
  (test keys, `dev-mail`) in it when proposing a deploy.

**Accounts (milestone S2).** Register, confirm by email, sign in, sign out, reset or change the
password, two-step verification (TOTP plus backup codes). The pages are static
(`site/account/`, `site/privacy.html`, one `account.js`); only the four form pages may load
Cloudflare Turnstile, and only when accounts are open. Mail goes through an outbox in Postgres,
sent by a worker in `api`; templates are in `server/templates/`.

**Testing accounts** (staging or a local stack): put Cloudflare's test keys and the Mailpit sink
in `.env` (the commented block at the end of `.env.example`; `COMPOSE_PROFILES=dev-mail`). Then:
```sh
# Rust unit + database integration tests (a throwaway Postgres):
docker run -d --name krunk-pgtest -e POSTGRES_PASSWORD=itpass -p 55432:5432 postgres:18.6-alpine3.24
KRUNK_TEST_DATABASE_URL=postgres://postgres:itpass@localhost:55432/postgres cargo test   # in server/
cargo test --release it::timing -- --ignored --nocapture                                  # timing, alone
# Every page in a browser (PUBLIC_BASE_URL=https://krunk.test in .env; see the script's header):
NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules node server/e2e/accounts.cjs http://<host:port> <project> <shots dir>
```

**Client IP.**
- Caddy trusts `CF-Connecting-IP` only from Cloudflare's ranges (`cloudflare-trusted.caddy`,
  regenerated by `update-cloudflare-ips.sh`, dated) and from `TRUSTED_PROXIES_EXTRA`. That
  defaults to `10.250.137.4/32`, ai-pipe's proxy in front of krunk.209.wf (OPEN_QUESTIONS Q8).
  Standalone deployments set their own hop, or `TRUSTED_PROXIES_EXTRA=` for none.
- It forwards the result to `api` as `X-Krunk-Client-IP`, and strips `CF-IPCountry` from
  untrusted peers.
- `api` believes those headers only when the TCP peer is the `web` container.

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
- `restore-db.sh` stops `api`, runs `pg_restore --clean --if-exists` and starts `api` again.
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
dist/server/publish-release.sh dist/Krunk-Setup-0.2.1.exe dist/Krunk-win-x64.zip 0.2.1 \
    --volume krunk_krunk-releases --notes NOTES-0.2.1.md
```
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
- `--released YYYY-MM-DD`: the release date.

Old versions stay under `/download/<version>/` until you remove them from the volume.

The manifest format:
```json
{"version":"0.2.1","released":"2026-11-05","url":"/download/0.2.1/Krunk-Setup-0.2.1.exe",
 "sha256":"…","size":143620406,"notes_url":"/download/0.2.1/NOTES.md","min_supported":"0.2.0"}
```
`latest.json.sig` holds the base64 of the 64-byte Ed25519 signature over the exact bytes of
`latest.json`.

## Checks
```sh
hadolint dist/server/Dockerfile
yamllint compose.yaml dist/server/compose.yaml dist/server/compose.ports.yaml
docker compose config -q
```
