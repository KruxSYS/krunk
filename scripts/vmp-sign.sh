#!/usr/bin/env bash
# Optional Widevine VMP signing of a package directory with castLabs EVS (Krunk 0.6.0, R25).
# Krunk ships without VMP: the signing runs only when EVS credentials exist, and never fails
# the build. Set it up once on the build host:
#   python3 -m pip install --upgrade castlabs-evs
#   python3 -m castlabs_evs.account signup        (or reauth on a new machine)
#   printf 'EVS_ACCOUNT_NAME=...\nEVS_PASSWD=...\n' > ~/.config/krunk/evs; chmod 600 ~/.config/krunk/evs
# The tokens expire about monthly; the refresh below renews them with the stored credentials.
# Usage: scripts/vmp-sign.sh <package dir with Krunk.exe>
set -u
pkg="${1:?package directory}"
creds="${KRUNK_EVS_FILE:-$HOME/.config/krunk/evs}"
if [ ! -f "$creds" ]; then
  echo "VMP signing skipped (no EVS credentials)" >&2
  exit 0
fi
if ! python3 -c 'import castlabs_evs' 2>/dev/null; then
  echo "VMP signing skipped (castlabs-evs is not installed: python3 -m pip install castlabs-evs)" >&2
  exit 0
fi
set -a
# shellcheck source=/dev/null
. "$creds"
set +a
export EVS_NO_ASK=1
if python3 -m castlabs_evs.account refresh >/dev/null 2>&1 \
   && python3 -m castlabs_evs.vmp sign-pkg "$pkg" \
   && python3 -m castlabs_evs.vmp verify-pkg "$pkg"; then
  echo "VMP signed: $(find "$pkg" -maxdepth 1 -name '*.sig' -printf '%f ' 2>/dev/null)" >&2
else
  echo "VMP signing failed; the package ships unsigned (Widevine still plays where VMP is not enforced)" >&2
fi
exit 0
