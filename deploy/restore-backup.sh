#!/usr/bin/env bash
# Decrypt only into pipes. --verify lists the archive without changing a DB.
set -euo pipefail
: "${BACKUP_IDENTITY_FILE:?set off-server age private identity file}"
input=${1:?usage: restore-backup.sh FILE.dump.age --verify|--restore-empty}
mode=${2:---verify}
CONTAINER=${CONTAINER:-audeniq-production-postgres-1}
TARGET_DB=${TARGET_DB:-audeniq_restore}
command -v age >/dev/null
case "$mode" in --verify|--restore-empty) ;; *) echo 'unknown restore mode' >&2; exit 2;; esac
age -d -i "$BACKUP_IDENTITY_FILE" "$input" | docker exec -i "$CONTAINER" pg_restore --list >/dev/null
if [ "$mode" = --restore-empty ]; then
  # The operator creates an isolated empty target first. No --clean/drop here.
  [ "$TARGET_DB" != audeniq_prod ] && [[ "$TARGET_DB" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] || { echo 'isolated target DB required' >&2; exit 1; }
  count=$(docker exec "$CONTAINER" psql -U audeniq_owner -d "$TARGET_DB" -Atc "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%' AND c.relkind IN ('r','p','v','m','S','f')")
  [ "$count" = 0 ] || { echo 'target DB must be empty' >&2; exit 1; }
  age -d -i "$BACKUP_IDENTITY_FILE" "$input" | docker exec -i "$CONTAINER" pg_restore -U audeniq_owner -d "$TARGET_DB" --exit-on-error --single-transaction
fi
echo 'Encrypted backup verified'
