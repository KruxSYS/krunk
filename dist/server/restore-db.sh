#!/usr/bin/env bash
# Restores a dump made by backup-db.sh into a compose project (docs/sync/PLAN.md §4.9):
#   dist/server/restore-db.sh --project NAME --dump FILE[.age] [--age-identity KEYFILE]
# Stops api, replaces the database contents (pg_restore --clean --if-exists), starts api.
# Afterwards re-apply account deletions newer than the dump (deletions.log, from S10 on).
set -euo pipefail
project="" dump="" identity=""
while [ $# -gt 0 ]; do
  case "$1" in
    --project) project=$2; shift 2 ;;
    --dump) dump=$2; shift 2 ;;
    --age-identity) identity=$2; shift 2 ;;
    *) sed -n '2,6p' "$0" >&2; exit 2 ;;
  esac
done
[ -n "$project" ] && [ -f "$dump" ] || { sed -n '2,6p' "$0" >&2; exit 2; }
find_svc() {
  docker ps -aq --filter "label=com.docker.compose.project=$project" --filter "label=com.docker.compose.service=$1"
}
db=$(find_svc db); api=$(find_svc api)
[ -n "$db" ] || { echo "no db container in project $project" >&2; exit 1; }
[ -z "$api" ] || docker stop "$api" >/dev/null
if [[ "$dump" == *.age ]]; then
  [ -n "$identity" ] || { echo "--age-identity is required for .age dumps" >&2; exit 1; }
  age -d -i "$identity" "$dump" | docker exec -i "$db" pg_restore -U krunk -d krunk --clean --if-exists --no-owner
else
  docker exec -i "$db" pg_restore -U krunk -d krunk --clean --if-exists --no-owner < "$dump"
fi
[ -z "$api" ] || docker start "$api" >/dev/null
echo "restored $dump into $project"
