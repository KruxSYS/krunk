# Krunk website, downloads and update feed: Caddy with the static site baked in.
# Releases live in a volume (/srv/releases), written by publish-release.sh.
#
# The build context is the repository root (compose.yaml: context ../..), filtered by the
# whitelist Dockerfile.dockerignore next to this file: only CHANGELOG.md and the files below
# are sent, never target/.

# The /changelog page, generated from CHANGELOG.md. check fails the build on a malformed file.
# Python 3.13.18.
FROM python:3.13-alpine@sha256:1e1f6334529d1fb26a2ef619d7cffb30a7c7d7cc1cc7338ebee283a58d1aeaca AS changelog
WORKDIR /build
COPY CHANGELOG.md dist/server/changelog.py ./
RUN mkdir /out \
    && python3 -I changelog.py check CHANGELOG.md \
    && python3 -I changelog.py render CHANGELOG.md /out/changelog.html

FROM caddy:2.11.8-alpine@sha256:3db3a09e418f86a8fce8de4bc0026fccd2b4cd925aef00cc7d843d9fdc75e442

# Unprivileged user; Caddy listens on 8080/8443, so no capability is needed. The folders
# exist here with the right owner, so new named volumes mounted on them inherit it.
RUN addgroup -S -g 10001 krunk \
    && adduser -S -D -H -u 10001 -G krunk -s /sbin/nologin krunk \
    && mkdir -p /srv/site /srv/releases /data /config \
    && chown -R 10001:10001 /data /config /srv/releases \
    && setcap -r /usr/bin/caddy

COPY --chown=root:root dist/server/Caddyfile dist/server/cloudflare-trusted.caddy dist/server/cloudflare-untrusted.caddy /etc/caddy/
COPY --chown=root:root dist/server/site/ /srv/site/
COPY --from=changelog --chown=root:root /out/changelog.html /srv/site/changelog.html

USER 10001:10001
EXPOSE 8080 8443
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["wget", "-q", "-O", "/dev/null", "http://127.0.0.1:8080/healthz"]
CMD ["caddy", "run", "--config", "/etc/caddy/Caddyfile", "--adapter", "caddyfile"]
