#!/usr/bin/env bash
# Restores a dump into a compose project (docs/sync/PLAN.md §4.9):
#   dist/server/restore-db.sh --project NAME --latest
#   dist/server/restore-db.sh --project NAME --dump FILE[.age] [--age-identity KEYFILE] [--deletions FILE[.age]]
# --latest takes the newest daily dump from the stack's own krunk-backups volume; --dump a file
# from backup-db.sh. Stops api, replaces the database contents (pg_restore --clean --if-exists),
# deletes again every account deleted since (deletions.log), starts api.
# The restore is one transaction (0.11.7): if it fails, the database is as it was and api starts
# again. Deletions (0.13.0, PLAN §7): before restoring, the database's own list of deleted
# account ids (account_deletions) is merged into deletions.log in the backup volume; after it,
# every id in that log (and in --deletions, the list backup-db.sh saved beside an off-host dump)
# is deleted from the restored database, so a deleted account never comes back.
set -euo pipefail
project="" dump="" identity="" latest="" deletions=""
usage() { sed -n '2,8p' "$0" >&2; exit 2; }
while [ $# -gt 0 ]; do
  case "$1" in
    --project) project=$2; shift 2 ;;
    --dump) dump=$2; shift 2 ;;
    --latest) latest=1; shift ;;
    --age-identity) identity=$2; shift 2 ;;
    --deletions) deletions=$2; shift 2 ;;
    *) usage ;;
  esac
done
[ -n "$project" ] || usage
if [ -n "$latest" ]; then [ -z "$dump" ] || usage; else [ -f "$dump" ] || usage; fi
[ -z "$deletions" ] || [ -f "$deletions" ] || usage
find_svc() {
  docker ps -aq --filter "label=com.docker.compose.project=$project" --filter "label=com.docker.compose.service=$1"
}
db=$(find_svc db); api=$(find_svc api); backup=$(find_svc backup)
[ -n "$db" ] || { echo "no db container in project $project" >&2; exit 1; }
if [ -n "$latest" ]; then
  [ -n "$backup" ] || { echo "no backup container in project $project" >&2; exit 1; }
  # Names carry a UTC timestamp, so the last in sort order is the newest.
  file=$(docker exec "$backup" sh -c 'ls -1 /var/lib/postgresql/krunk-*.dump 2>/dev/null | sort | tail -n 1')
  [ -n "$file" ] || { echo "no dump in the backup volume of $project" >&2; exit 1; }
  echo "restoring ${file##*/}"
fi
if { [[ "$dump" == *.age ]] || [[ "$deletions" == *.age ]]; } && [ -z "$identity" ]; then
  echo "--age-identity is required for .age files" >&2
  exit 1
fi
psql_db() { docker exec -i "$db" psql -U krunk -d krunk -XAtq -v ON_ERROR_STOP=1 "$@"; }

# 1. Every deletion known now: the log in the backup volume, the live table, --deletions.
#    Lines are "<account id> <unix time>".
known=$(mktemp)
trap 'rm -f "$known"' EXIT
if [ -n "$backup" ]; then
  docker exec "$backup" sh -c 'cat /var/lib/postgresql/deletions.log 2>/dev/null || true' >> "$known"
fi
psql_db -c "SELECT account_id || ' ' || extract(epoch FROM deleted_at)::bigint FROM account_deletions" \
  >> "$known" 2>/dev/null || true
if [ -n "$deletions" ]; then
  if [[ "$deletions" == *.age ]]; then age -d -i "$identity" "$deletions" >> "$known"; else cat "$deletions" >> "$known"; fi
fi
grep -E '^[0-9a-f-]{36} [0-9]+$' "$known" | sort -u > "$known.ok" || true
mv "$known.ok" "$known"
if [ -n "$backup" ] && [ -s "$known" ]; then
  # Keep the merged list beside the dumps, so a later restore knows these deletions too.
  docker exec -i "$backup" sh -c 'umask 077; cat > /var/lib/postgresql/deletions.log.tmp &&
    mv /var/lib/postgresql/deletions.log.tmp /var/lib/postgresql/deletions.log' < "$known"
fi
echo "deletions known: $(wc -l < "$known")"

# 2. The restore itself.
[ -z "$api" ] || docker stop "$api" >/dev/null
trap 'rm -f "$known"; [ -z "$api" ] || docker start "$api" >/dev/null' EXIT
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

# 3. Delete those accounts again (one transaction). A dump from before 0.13.0 has no
#    account_deletions table yet: it is created as the 0007 expand step defines it.
if [ -s "$known" ]; then
  {
    echo "BEGIN;"
    echo "CREATE TABLE IF NOT EXISTS account_deletions (account_id uuid PRIMARY KEY, deleted_at timestamptz NOT NULL DEFAULT now());"
    echo "CREATE TEMP TABLE gone (account_id uuid, at bigint);"
    echo "COPY gone FROM STDIN WITH (DELIMITER ' ');"
    cat "$known"
    echo '\.'
    echo "SELECT 'accounts deleted again: ' || count(*) FROM accounts WHERE id IN (SELECT account_id FROM gone);"
    echo "DELETE FROM accounts WHERE id IN (SELECT account_id FROM gone);"
    echo "INSERT INTO account_deletions (account_id, deleted_at) SELECT DISTINCT ON (account_id) account_id, to_timestamp(at) FROM gone ORDER BY account_id, at ON CONFLICT DO NOTHING;"
    echo "COMMIT;"
  } | psql_db
fi
