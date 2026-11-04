#!/usr/bin/env bash
# Creates Krunk's update-signing key pair (Ed25519, OpenSSL 3):
#
#   dist/server/gen-signing-key.sh [PRIVATE_KEY_PATH] [PUBLIC_KEY_PATH]
#
# Defaults: ~/.config/krunk/update-signing.key (mode 600) and
# assets/keys/update-signing.pub (commit this one; build.rs builds it into Krunk).
#
# The private key signs every latest.json (publish-release.sh). Keep it off the server, out
# of git and out of containers, and back it up offline: whoever holds it can make every Krunk
# install run their program, and if it is lost, existing installs can never be updated again
# (they only trust this key). The script refuses to overwrite an existing key.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
key=${1:-"$HOME/.config/krunk/update-signing.key"}
pub=${2:-"$root/assets/keys/update-signing.pub"}

if [ -e "$key" ]; then
  echo "refusing to overwrite $key (delete it yourself if you really mean to replace the release key)" >&2
  exit 1
fi
command -v openssl >/dev/null || { echo "openssl (3.x) is required" >&2; exit 1; }

mkdir -p "$(dirname "$key")" "$(dirname "$pub")"
chmod 700 "$(dirname "$key")" 2>/dev/null || true
(
  umask 077
  openssl genpkey -algorithm ed25519 -out "$key"
)
chmod 600 "$key"
openssl pkey -in "$key" -pubout -out "$pub"
echo "private key: $key (mode $(stat -c %a "$key")) - back it up offline, never commit it"
echo "public key:  $pub"
cat "$pub"
