#!/usr/bin/env bash
# AUDENIQ DB 백업 — production postgres를 pg_dump(custom 형식)로 떠서 보관한다.
#
#   ./backup.sh              # /var/backups/audeniq 에 덤프, KEEP_DAYS(기본 14)일 지난 것 삭제
#   BACKUP_DIR=/mnt/x ./backup.sh
#
# cron 예: 30 18 * * * /opt/audeniq/backup.sh >> /var/log/audeniq-backup.log 2>&1  (UTC 18:30 = KST 03:30)
# 복원:   docker exec -i audeniq-production-postgres-1 pg_restore -U audeniq_owner -d audeniq_prod --clean --if-exists < 덤프
# 주의: 계좌번호는 PAYOUT_ACCOUNT_KEY로 암호화돼 있다. 키 없이 복원하면 계좌번호를 읽을 수 없으니
# production.env는 이 덤프와 다른 곳에 따로 보관한다. 같은 서버에만 두면 서버를 잃을 때 함께 잃는다.
set -euo pipefail

BACKUP_DIR=${BACKUP_DIR:-/var/backups/audeniq}
KEEP_DAYS=${KEEP_DAYS:-14}
CONTAINER=${CONTAINER:-audeniq-production-postgres-1}

umask 077
mkdir -p "$BACKUP_DIR"
out="$BACKUP_DIR/audeniq_prod-$(date -u +%Y%m%dT%H%M%SZ).dump"
tmp="$out.partial"

docker exec "$CONTAINER" pg_dump -U audeniq_owner -d audeniq_prod -Fc > "$tmp"
# 덤프가 읽히는지 목차로 확인하고 나서야 완성본으로 이름을 바꾼다
docker exec -i "$CONTAINER" pg_restore --list < "$tmp" > /dev/null
mv "$tmp" "$out"

find "$BACKUP_DIR" -name 'audeniq_prod-*.dump' -mtime +"$KEEP_DAYS" -delete
find "$BACKUP_DIR" -name '*.partial' -mmin +60 -delete
echo "backup: $(date -u +%FT%TZ) $out $(du -h "$out" | cut -f1)"
