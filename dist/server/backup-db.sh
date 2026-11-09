#!/usr/bin/env bash
# Dumps the sync database (docs/sync/PLAN.md §4.9) of a compose project to this machine:
#   dist/server/backup-db.sh --project NAME --out DIR [--age-recipient AGE_PUBLIC_KEY]
# Writes DIR/krunk-<project>-<UTC timestamp>.dump (pg_dump custom format), encrypted to the
# age recipient as .dump.age when one is given (needs `age` on PATH). The stack's backup service
# already keeps 14 daily dumps in its krunk-backups volume (OPEN_QUESTIONS Q4); this is for
# copies that leave the host, and those must be encrypted: dumps hold emails and IPs.
set -euo pipefail
project="" out="" recipient=""
while [ $# -gt 0 ]; do
  case "$1" in
    --project) project=$2; shift 2 ;;
    --out) out=$2; shift 2 ;;
    --age-recipient) recipient=$2; shift 2 ;;
    *) sed -n '2,7p' "$0" >&2; exit 2 ;;
  esac
done
[ -n "$project" ] && [ -n "$out" ] || { sed -n '2,7p' "$0" >&2; exit 2; }
db=$(docker ps -q --filter "label=com.docker.compose.project=$project" --filter "label=com.docker.compose.service=db")
[ -n "$db" ] || { echo "no running db container in project $project" >&2; exit 1; }
# Private from the start: the folder too (0.11.7). A dump is written to .part and renamed only
# when complete, so a failed dump never looks like a backup.
umask 077
mkdir -p "$out"
name="$out/krunk-$project-$(date -u +%Y%m%dT%H%M%SZ).dump"
trap 'rm -f "$name.part"' EXIT
if [ -n "$recipient" ]; then
  command -v age >/dev/null || { echo "age is not installed" >&2; exit 1; }
  docker exec "$db" pg_dump -U krunk -d krunk -Fc | age -r "$recipient" > "$name.part"
  mv "$name.part" "$name.age"
  echo "wrote $name.age ($(stat -c %s "$name.age") bytes, encrypted)"
else
  docker exec "$db" pg_dump -U krunk -d krunk -Fc > "$name.part"
  mv "$name.part" "$name"
  echo "wrote $name ($(stat -c %s "$name") bytes, NOT encrypted: pass --age-recipient for real backups)"
fi
