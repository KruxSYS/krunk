#!/usr/bin/env bash
# Starts or stops a Mailpit mail sink next to a compose project, for staging and local tests only
# (it is not in compose.yaml, so no setting can ever start it on a live deploy):
#   server/e2e/mailsink.sh up   <compose project>   # e.g. stage-browser
#   server/e2e/mailsink.sh down <compose project>
# It joins the project's default network as "mailpit" (SMTP 1025, API 8025; no host ports).
# Point api at it with staging.env: SMTP_HOST=mailpit SMTP_PORT=1025 SMTP_TLS=none
# ACCOUNTS_TEST_MODE=1.
set -euo pipefail
IMAGE=axllent/mailpit:v1.33.0@sha256:9b3a463be15354c0f0eba91697d80e3e0d8a5623ef0e627cacc836bc17fd5231
cmd=${1:-}; project=${2:-}
[ -n "$cmd" ] && [ -n "$project" ] || { sed -n '2,8p' "$0" >&2; exit 2; }
case "$project" in live-*) echo "refusing: $project is a live project" >&2; exit 1 ;; esac
name="$project-mailsink"
case "$cmd" in
  up)
    docker rm -f "$name" >/dev/null 2>&1 || true
    docker run -d --name "$name" --network "${project}_default" --network-alias mailpit \
      --read-only --tmpfs /tmp --cap-drop ALL --security-opt no-new-privileges:true \
      -e MP_MAX_MESSAGES=500 "$IMAGE" >/dev/null
    echo "mail sink $name is up (mailpit:1025 on ${project}_default)"
    ;;
  down)
    docker rm -f "$name" >/dev/null 2>&1 || true
    echo "mail sink $name removed"
    ;;
  *) sed -n '2,8p' "$0" >&2; exit 2 ;;
esac
