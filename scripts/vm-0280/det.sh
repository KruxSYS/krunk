#!/bin/bash
# Krunk 0.28.0: open each URL in the running browser on the VM (address bar), then read it with det.mjs.
#   det.sh OUTDIR url [url...]   (browser started with start.ps1 -Dev; ssh -L 19870 tunnel up)
here=$(dirname "$0"); out=$1; shift; mkdir -p "$out"
for u in "$@"; do
  printf ". C:\\\\pipeline\\\\krunk\\\\lib.ps1\nClick 640 30 300; Go '%s' 1500\n" "$u" > /tmp/det-go.ps1
  "$here/../vm.sh" ps1 /tmp/det-go.ps1 >/dev/null 2>&1
  n=$(echo "$u" | sed -E 's#https?://##; s#[^a-zA-Z0-9]+#_#g' | cut -c1-40)
  node "$here/det.mjs" "$u" "${DET_WAIT:-25000}" > "$out/$n.json"
  "$here/../vm.sh" shot "$n" "$out" >/dev/null 2>&1
  echo "== $u"; head -c 2500 "$out/$n.json"; echo
done
