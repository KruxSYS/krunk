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

Then build Krunk for that server: `UPDATE_BASE_URL=https://downloads.example.org scripts/build-dist.sh`. The default build points at the placeholder `https://krunk.example`.

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
dist/server/publish-release.sh dist/Krunk-Setup-0.2.0.exe dist/Krunk-win-x64.zip 0.2.0 \
    --volume krunk_krunk-releases --notes NOTES-0.2.0.md
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
