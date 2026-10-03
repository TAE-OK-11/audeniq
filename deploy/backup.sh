#!/usr/bin/env bash
# AUDENIQ DB 백업 — pg_dump 스트림을 age 공개키로 암호화하여 보관한다.
#
#   ./backup.sh              # /var/backups/audeniq 에 덤프, KEEP_DAYS(기본 14)일 지난 것 삭제
#   BACKUP_DIR=/mnt/x ./backup.sh
#
# cron 예: 30 18 * * * /opt/audeniq/backup.sh >> /var/log/audeniq-backup.log 2>&1  (UTC 18:30 = KST 03:30)
# BACKUP_RECIPIENTS_FILE: age 공개키만 있는 파일. 복호화 개인키는 서버 밖에 보관.
# 복원 검증: BACKUP_IDENTITY_FILE=/secure/identity ./restore-backup.sh FILE.dump.age --verify
set -euo pipefail

BACKUP_DIR=${BACKUP_DIR:-/var/backups/audeniq}
KEEP_DAYS=${KEEP_DAYS:-14}
CONTAINER=${CONTAINER:-audeniq-production-postgres-1}
: "${BACKUP_RECIPIENTS_FILE:?set age public recipient file}"
[[ "$KEEP_DAYS" =~ ^[0-9]+$ ]] && (( KEEP_DAYS >= 1 && KEEP_DAYS <= 365 )) || { echo 'invalid KEEP_DAYS' >&2; exit 1; }
command -v age >/dev/null || { echo 'age must be installed before running a backup' >&2; exit 1; }
# Validate recipients before pg_dump starts; refusal must leave no plaintext dump.
age -R "$BACKUP_RECIPIENTS_FILE" </dev/null >/dev/null

umask 077
mkdir -p "$BACKUP_DIR"
out="$BACKUP_DIR/audeniq_prod-$(date -u +%Y%m%dT%H%M%SZ).dump.age"
tmp="$out.partial"
trap 'rm -f "$tmp"' EXIT

docker exec "$CONTAINER" pg_dump -U audeniq_owner -d audeniq_prod -Fc | age -R "$BACKUP_RECIPIENTS_FILE" > "$tmp"
# pipefail verifies both pg_dump and authenticated encryption completed.
# A separate restore drill validates the archive with an off-server private key.
mv "$tmp" "$out"

find "$BACKUP_DIR" -name 'audeniq_prod-*.dump.age' -mtime +"$KEEP_DAYS" -delete
echo "backup: $(date -u +%FT%TZ) $out $(du -h "$out" | cut -f1)"
