#!/usr/bin/env bash
# The Linux side of the 090-sync VM flow (0.9.0): the test client (tools/sync-cli) as the other
# devices of the staging test account.
#   scripts/vm-090-cli.sh EMAIL PASSWORD WORKDIR step
# Steps:
#   restore-q   link CLI Q (approved as the account), restore with the passphrase Krunk made,
#               sync; list what came; add the bookmark "From CLI" and sync it
#   join-r      link CLI R, then (background) join with the code relayed from the VM's sas.txt
#               (WRONG=1: a wrong code is relayed)
#   after-r     CLI R syncs bookmarks and passwords (shared by Krunk)
#   wait-lock   (background) when the VM says the vault is locked: CLI R adds a login, syncs,
#               tells the VM (cli-added)
#   after-remove  CLI Q follows the rotation; the removed CLI R tries the new records
cd "$(dirname "$0")/.." || exit 1
email=$1; pw=$2; work=$3; step=$4
cli=tools/sync-cli/target/debug/krunk-sync-cli
export NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules
mkdir -p "$work"
vm() { scripts/vm.sh sh "$1" </dev/null | tr -d '\r'; }
link() { # name
  rm -rf "${work:?}/$1"
  "$cli" link --dir "$work/$1" --server http://dind:10017 --label "Test $1" --timeout 120 </dev/null > "$work/$1-link.log" 2>&1 &
  local pid=$! code=""
  for _ in $(seq 1 40); do
    code=$(sed -n 's/^user_code: //p' "$work/$1-link.log" | head -1)
    [ -n "$code" ] && break; sleep 0.5
  done
  node server/e2e/approve.cjs http://dind:10017 stage-browser "$email" "$pw" "$code" </dev/null
  wait $pid; echo "link $1: exit $?"
}
secrets() { [ -f "$work/secrets.txt" ] || scripts/vm.sh get C:/pipeline/krunk/p090u/p090u-secrets.txt "$work/secrets.txt" </dev/null; }
case "$step" in
  restore-q)
    secrets
    link Q
    "$cli" restore --dir "$work/Q" --passphrase "$(sed -n 1p "$work/secrets.txt" | tr -d '\r')" </dev/null | grep -v '^passphrase\|^recovery'
    "$cli" sync --dir "$work/Q" --name "CLI Q" </dev/null | grep -E '^(pulled|pushed|epoch)'
    echo "-- CLI Q has:"; "$cli" bookmark list --dir "$work/Q" </dev/null; "$cli" history list --dir "$work/Q" </dev/null
    "$cli" bookmark add --dir "$work/Q" --title "From CLI" --url https://from-cli.example/ </dev/null
    "$cli" sync --dir "$work/Q" --name "CLI Q" </dev/null | grep -E '^(pulled|pushed)' ;;
  join-r)
    link R
    rm -f "$work/sas.txt"; vm "Remove-Item C:/pipeline/krunk/sas.txt -ErrorAction SilentlyContinue" >/dev/null
    # (Background jobs keep no handle on the caller's stdout: a pipe would wait for them.)
    ( "$cli" join --dir "$work/R" --code-file "$work/sas.txt" --vault --name "CLI R" --timeout 540 </dev/null > "$work/R-join.log" 2>&1; echo "exit $?" >> "$work/R-join.log" ) >/dev/null 2>&1 &
    ( for _ in $(seq 1 90); do
        s=$(vm "Get-Content C:/pipeline/krunk/sas.txt -ErrorAction SilentlyContinue" | tr -d ' \n')
        if [ -n "$s" ]; then
          # WRONG=1: the user types a wrong code (the approval must end, nothing admitted).
          [ "${WRONG:-0}" = 1 ] && s=$(printf '%06d' $(( (10#$s + 1) % 1000000 )))
          echo "$s" > "$work/sas.txt"; exit 0
        fi; sleep 2
      done ) >/dev/null 2>&1 </dev/null &
    echo "join started" ;;
  after-r)
    cat "$work/R-join.log"
    "$cli" sync --dir "$work/R" --name "CLI R" --vault </dev/null | grep -E '^(pulled|pushed|pw_)'
    echo "-- CLI R has:"; "$cli" bookmark list --dir "$work/R" </dev/null | grep -c '^bookmark'; "$cli" password list --dir "$work/R" </dev/null | sed 's/ | [^|]* | \([0-9a-f]*\)$/ | <password> | \1/' ;;
  wait-lock)
    ( for _ in $(seq 1 150); do
        if [ -n "$(vm "if (Test-Path C:/pipeline/krunk/locked) { 'y' }")" ]; then
          "$cli" password add --dir "$work/R" --origin https://cli-added.example --user carol --password "Cli-added-1" </dev/null
          "$cli" sync --dir "$work/R" --name "CLI R" --vault </dev/null | grep -E '^pw_pushed'
          vm "Set-Content C:/pipeline/krunk/cli-added 1" >/dev/null
          echo "added while locked"; exit 0
        fi; sleep 2
      done ) > "$work/wait-lock.log" 2>&1 &
    echo "waiting for the lock" ;;
  after-remove)
    "$cli" sync --dir "$work/Q" --name "CLI Q" </dev/null | grep -E '^(epoch|min_record_epoch|pulled|rotated)'
    "$cli" dump-records --dir "$work/Q" --out "$work/after-remove.json" </dev/null
    echo "-- the removed CLI R:"; "$cli" try-decrypt --dir "$work/R" --file "$work/after-remove.json" --vault </dev/null
    "$cli" sync --dir "$work/R" --vault </dev/null; echo "R sync exit $?" ;;
esac
