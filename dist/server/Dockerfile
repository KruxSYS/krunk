# Krunk website, downloads and update feed: Caddy with the static site baked in.
# Releases live in a volume (/srv/releases), written by publish-release.sh.
FROM caddy:2.11.8-alpine@sha256:3db3a09e418f86a8fce8de4bc0026fccd2b4cd925aef00cc7d843d9fdc75e442

# Unprivileged user; Caddy listens on 8080/8443, so no capability is needed. The folders
# exist here with the right owner, so new named volumes mounted on them inherit it.
RUN addgroup -S -g 10001 krunk \
    && adduser -S -D -H -u 10001 -G krunk -s /sbin/nologin krunk \
    && mkdir -p /srv/site /srv/releases /data /config \
    && chown -R 10001:10001 /data /config /srv/releases \
    && setcap -r /usr/bin/caddy

COPY --chown=root:root Caddyfile /etc/caddy/Caddyfile
COPY --chown=root:root site/ /srv/site/

USER 10001:10001
EXPOSE 8080 8443
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["wget", "-q", "-O", "/dev/null", "http://127.0.0.1:8080/healthz"]
CMD ["caddy", "run", "--config", "/etc/caddy/Caddyfile", "--adapter", "caddyfile"]
