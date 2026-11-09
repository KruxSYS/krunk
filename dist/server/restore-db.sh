#!/usr/bin/env bash
# Restores a dump into a compose project (docs/sync/PLAN.md §4.9):
#   dist/server/restore-db.sh --project NAME --latest
#   dist/server/restore-db.sh --project NAME --dump FILE[.age] [--age-identity KEYFILE]
# --latest takes the newest daily dump from the stack's own krunk-backups volume; --dump a file
# from backup-db.sh. Stops api, replaces the database contents (pg_restore --clean --if-exists),
# starts api. Afterwards re-apply account deletions newer than the dump (deletions.log, from S10).
# The restore is one transaction (0.11.7): if it fails, the database is as it was and api starts
# again.
set -euo pipefail
project="" dump="" identity="" latest=""
usage() { sed -n '2,7p' "$0" >&2; exit 2; }
while [ $# -gt 0 ]; do
  case "$1" in
    --project) project=$2; shift 2 ;;
    --dump) dump=$2; shift 2 ;;
    --latest) latest=1; shift ;;
    --age-identity) identity=$2; shift 2 ;;
    *) usage ;;
  esac
done
[ -n "$project" ] || usage
if [ -n "$latest" ]; then [ -z "$dump" ] || usage; else [ -f "$dump" ] || usage; fi
find_svc() {
  docker ps -aq --filter "label=com.docker.compose.project=$project" --filter "label=com.docker.compose.service=$1"
}
db=$(find_svc db); api=$(find_svc api)
[ -n "$db" ] || { echo "no db container in project $project" >&2; exit 1; }
if [ -n "$latest" ]; then
  backup=$(find_svc backup)
  [ -n "$backup" ] || { echo "no backup container in project $project" >&2; exit 1; }
  # Names carry a UTC timestamp, so the last in sort order is the newest.
  file=$(docker exec "$backup" sh -c 'ls -1 /var/lib/postgresql/krunk-*.dump 2>/dev/null | sort | tail -n 1')
  [ -n "$file" ] || { echo "no dump in the backup volume of $project" >&2; exit 1; }
  echo "restoring ${file##*/}"
fi
if [[ "$dump" == *.age ]] && [ -z "$identity" ]; then
  echo "--age-identity is required for .age dumps" >&2
  exit 1
fi
[ -z "$api" ] || docker stop "$api" >/dev/null
trap '[ -z "$api" ] || docker start "$api" >/dev/null' EXIT
restore() { docker exec -i "$db" pg_restore -U krunk -d krunk --clean --if-exists --no-owner --single-transaction; }
if [ -n "$latest" ]; then
  docker exec "$backup" cat "$file" | restore
  dump=${file##*/}
elif [[ "$dump" == *.age ]]; then
  age -d -i "$identity" "$dump" | restore
else
  restore < "$dump"
fi
echo "restored $dump into $project"
