#!/usr/bin/env bash
# Krunk 0.26.0 publish steps (run from /work/projects/extra/browser/src on the machine that holds
# ~/.config/krunk/update-signing.key; the key is only read).
#   publish-0255.sh backup      copy the live latest.json(.sig) and release.json to live-before/
#   publish-0255.sh stage       publish to stage-browser_krunk-releases and verify the volume
#   publish-0255.sh live        publish to live-browser_krunk-releases
#   publish-0255.sh verify-live check https://krunk.209.wf (manifest signature, hashes, landing page)
set -euo pipefail
E=/work/projects/extra/browser/.pipeline/evidence-0.26.0/publish
ALPINE="alpine:3.22@sha256:2e0b81d60597f449baa56dc25d355367869d2dca4cd57e866f1e2b52af3eb55f"
NOTES=/tmp/krunk-notes-0.26.0.md  # what NOTES.md must be: "# Krunk <v>" + the CHANGELOG.md section
PUB=assets/keys/update-signing.pub
V=0.26.0
cd /work/projects/extra/browser/src
{ printf '# Krunk %s\n\n' "$V"; python3 -I dist/server/changelog.py section CHANGELOG.md "$V"; } > "$NOTES"

list_volume() { docker run --rm -v "$1:/srv:ro" "$ALPINE" sh -c 'cd /srv && find . -type f | sort'; }
read_volume() { docker run --rm -v "$1:/srv:ro" "$ALPINE" cat "/srv/$2"; }

verify_manifest() { # dir with latest.json and latest.json.sig
  local d=$1
  base64 -d "$d/latest.json.sig" > "$d/latest.sig.bin"
  openssl pkeyutl -verify -pubin -inkey "$PUB" -rawin -in "$d/latest.json" -sigfile "$d/latest.sig.bin"
}

case "${1:-}" in
  backup)
    mkdir -p "$E/live-before"
    list_volume live-browser_krunk-releases | tee "$E/live-before-listing.txt"
    for f in updates/win-x64/latest.json updates/win-x64/latest.json.sig download/release.json; do
      read_volume live-browser_krunk-releases "$f" > "$E/live-before/$(basename "$f")"
    done
    cat "$E/live-before/latest.json"; echo
    echo "backed up to $E/live-before/"
    ;;
  stage)
    list_volume stage-browser_krunk-releases > "$E/stage-before-listing.txt"
    dist/server/publish-release.sh "dist/Krunk-Setup-$V.exe" dist/Krunk-win-x64.zip "$V" \
      --volume stage-browser_krunk-releases --min-supported 0.2.0 2>&1 | tee "$E/stage-publish.log"
    {
      echo "== stage-browser_krunk-releases read back"
      list_volume stage-browser_krunk-releases
      echo
      mkdir -p "$E/stage-readback"
      for f in updates/win-x64/latest.json updates/win-x64/latest.json.sig download/release.json "download/$V/SHA256SUMS" "download/$V/NOTES.md"; do
        read_volume stage-browser_krunk-releases "$f" > "$E/stage-readback/$(basename "$f")"
      done
      echo '$ openssl verify latest.json with assets/keys/update-signing.pub (committed, built into Krunk)'
      verify_manifest "$E/stage-readback"
      echo '$ the files in the volume vs dist/'
      read_volume stage-browser_krunk-releases "download/$V/Krunk-Setup-$V.exe" | sha256sum | awk '{print $1}' > "$E/stage-readback/installer.sha"
      read_volume stage-browser_krunk-releases "download/$V/Krunk-win-x64.zip" | sha256sum | awk '{print $1}' > "$E/stage-readback/zip.sha"
      [ "$(cat "$E/stage-readback/installer.sha")" = "$(sha256sum "dist/Krunk-Setup-$V.exe" | awk '{print $1}')" ] && echo "installer = dist" || echo "installer DIFFERS"
      [ "$(cat "$E/stage-readback/zip.sha")" = "$(sha256sum dist/Krunk-win-x64.zip | awk '{print $1}')" ] && echo "zip = dist" || echo "zip DIFFERS"
      cmp -s "$E/stage-readback/NOTES.md" "$NOTES" && echo "NOTES.md = the CHANGELOG.md section" || echo "NOTES.md DIFFERS"
      read_volume stage-browser_krunk-releases "download/$V/files.json" > "$E/stage-readback/files.json"
      want=$(python3 -c "import json,sys; print(json.load(open(sys.argv[1]))['files_sha256'])" "$E/stage-readback/latest.json")
      [ "$(sha256sum "$E/stage-readback/files.json" | awk '{print $1}')" = "$want" ] && echo "files.json = its hash in the signed manifest ($(python3 -c "import json,sys; print(len(json.load(open(sys.argv[1]))['files']))" "$E/stage-readback/files.json" 2>/dev/null) entries)" || echo "files.json DIFFERS from the manifest"
      echo; cat "$E/stage-readback/latest.json"; echo; echo; cat "$E/stage-readback/release.json"
    } 2>&1 | tee "$E/stage-verify.txt"
    ;;
  live)
    dist/server/publish-release.sh "dist/Krunk-Setup-$V.exe" dist/Krunk-win-x64.zip "$V" \
      --volume live-browser_krunk-releases --min-supported 0.2.0 2>&1 | tee "$E/live-publish.log"
    ;;
  verify-live)
    mkdir -p "$E/live-readback"
    {
      echo "== https://krunk.209.wf after publishing ($(date -u +%FT%TZ))"
      for p in /updates/win-x64/latest.json /updates/win-x64/latest.json.sig /download/release.json "/download/$V/NOTES.md" "/download/$V/SHA256SUMS" "/download/$V/Krunk-Setup-$V.exe" "/download/$V/Krunk-win-x64.zip" /download/0.15.2/Krunk-Setup-0.15.2.exe "/download/$V/files.json" /; do
        h=$(curl -sI -r 0-0 "https://krunk.209.wf$p" | tr -d '\r')
        echo "$p  $(echo "$h" | awk 'NR==1{print $2}') $(echo "$h" | grep -i '^content-type' | cut -d' ' -f2-) cache=$(echo "$h" | grep -i '^cache-control' | cut -d' ' -f2-) cf=$(echo "$h" | grep -i '^cf-cache-status' | cut -d' ' -f2-)"
      done
      echo
      curl -s https://krunk.209.wf/updates/win-x64/latest.json -o "$E/live-readback/latest.json"
      curl -s https://krunk.209.wf/updates/win-x64/latest.json.sig -o "$E/live-readback/latest.json.sig"
      curl -s https://krunk.209.wf/download/release.json -o "$E/live-readback/release.json"
      cat "$E/live-readback/latest.json"; echo
      echo '$ verify served manifest with assets/keys/update-signing.pub'
      verify_manifest "$E/live-readback"
      echo '$ download the installer and zip from the site and hash them'
      curl -s "https://krunk.209.wf/download/$V/Krunk-Setup-$V.exe" -o /tmp/setup-0201.exe
      curl -s "https://krunk.209.wf/download/$V/Krunk-win-x64.zip" -o /tmp/k-0201.zip
      sha256sum /tmp/setup-0201.exe /tmp/k-0201.zip
      [ "$(sha256sum /tmp/setup-0201.exe | awk '{print $1}')" = "$(sha256sum "dist/Krunk-Setup-$V.exe" | awk '{print $1}')" ] && echo "installer = dist" || echo "installer DIFFERS"
      [ "$(sha256sum /tmp/k-0201.zip | awk '{print $1}')" = "$(sha256sum dist/Krunk-win-x64.zip | awk '{print $1}')" ] && echo "zip = dist" || echo "zip DIFFERS"
      echo '$ libcef.dll in the served zip vs this release engine (engine-files.sha256)'
      got=$(unzip -p /tmp/k-0201.zip Krunk-win-x64/libcef.dll | sha256sum | awk '{print $1}')
      echo "libcef.dll $got"; [ "$got" = "$(awk '$2 ~ /libcef.dll$/ {print $1}' engine-files.sha256)" ] && echo "libcef.dll byte-identical" || echo "libcef.dll DIFFERS"
      echo '$ release.json'; cat "$E/live-readback/release.json"; echo
      echo "\$ landing page mentions of $V: $(curl -s https://krunk.209.wf/ | grep -c "$V")"
      rm -f /tmp/setup-0201.exe /tmp/k-0201.zip
    } 2>&1 | tee "$E/live-verify.txt"
    ;;
  *) sed -n 2,7p "$0"; exit 2 ;;
esac
