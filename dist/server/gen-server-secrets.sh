#!/usr/bin/env bash
# Adds the API's secrets (docs/sync/PLAN.md §4.10) to an .env file, generating each missing
# one with `openssl rand`. Existing values are never overwritten.
#   dist/server/gen-server-secrets.sh [ENV_FILE]     (default: .env in the repository root)
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
env_file=${1:-"$root/.env"}
command -v openssl >/dev/null || { echo "openssl is required" >&2; exit 1; }
touch "$env_file"
chmod 600 "$env_file"
added=0
for key in POSTGRES_PASSWORD TOKEN_HMAC_KEY PASSWORD_PEPPER TOTP_ENC_KEY; do
  if grep -Eq "^${key}=.+" "$env_file"; then
    echo "kept      $key"
    continue
  fi
  # 32 random bytes, base64url without padding (URL-safe, also inside DATABASE_URL).
  value=$(openssl rand -base64 32 | tr '+/' '-_' | tr -d '=\n')
  sed -i "/^${key}=\$/d" "$env_file"
  printf '%s=%s\n' "$key" "$value" >> "$env_file"
  echo "generated $key"
  added=$((added + 1))
done
grep -q '^PASSWORD_PEPPER_ID=' "$env_file" || echo 'PASSWORD_PEPPER_ID=1' >> "$env_file"
echo "$env_file: $added new secret(s); file mode $(stat -c %a "$env_file")"
