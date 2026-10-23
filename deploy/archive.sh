#!/usr/bin/env bash
# AUDENIQ 원본 보관·무결성 고정·복구 백업 (호스트 cron에서 실행) — docs/DOCUMENT_ARCHIVE.md
#
#   ./archive.sh daily        봉인 체인 끝을 WORM 버킷에 고정(anchor) + 봉인 검증 + 오늘의 pg_dump를 WORM에 사본
#   ./archive.sh monthly      지난달 원본 보관(서명된 manifest) + 보관본 전체 검증(--deep)
#   ./archive.sh wal          PostgreSQL WAL(PITR) 조각을 age로 암호화해 WORM에 올리고 로컬 사본 삭제
#   ./archive.sh basebackup   PITR 기준 백업(pg_basebackup)을 age로 암호화해 WORM에 올림
#   ./archive.sh purge        보존기한이 지난 보관 객체 파기(개인정보 보호법 제21조)
#
# cron 예 (UTC):
#   10 19 * * *  /opt/audeniq/archive.sh daily       >> /var/log/audeniq-archive.log 2>&1   (KST 04:10, 백업 뒤)
#   40 19 1 * *  /opt/audeniq/archive.sh monthly     >> /var/log/audeniq-archive.log 2>&1   (매월 1일 KST 04:40)
#   */15 * * * * /opt/audeniq/archive.sh wal         >> /var/log/audeniq-archive.log 2>&1   (PG_ARCHIVE_MODE=on일 때)
#   20 20 * * 0  /opt/audeniq/archive.sh basebackup  >> /var/log/audeniq-archive.log 2>&1   (매주 일요일)
#   50 19 2 * *  /opt/audeniq/archive.sh purge       >> /var/log/audeniq-archive.log 2>&1
#
# 설정(production.env 또는 SECRETS_ENV_FILE): ARCHIVE_S3_ENDPOINT, ARCHIVE_S3_BUCKET,
# ARCHIVE_S3_ACCESS_KEY_ID, ARCHIVE_S3_SECRET_ACCESS_KEY, ARCHIVE_S3_REGION, ARCHIVE_OBJECT_LOCK,
# BACKUP_RECIPIENTS_FILE(age 공개키). 보관 버킷 자격증명은 API 컨테이너 환경에 두지 않고 실행할 때만 넘긴다.
set -euo pipefail

ENV_FILE=${ENV_FILE:-/opt/audeniq/production.env}
API=${API_CONTAINER:-audeniq-production-api-1}
PG=${PG_CONTAINER:-audeniq-production-postgres-1}
BACKUP_DIR=${BACKUP_DIR:-/var/backups/audeniq}
OPERATOR=${ARCHIVE_OPERATOR:-archive-cron}
die() { echo "archive: $*" >&2; exit 1; }
[ -f "$ENV_FILE" ] || die "ENV_FILE 없음: $ENV_FILE"

get() {
  local v
  v=$(sed -n "s/^$1=//p" "$ENV_FILE" | tail -n 1)
  local secrets
  secrets=$(sed -n 's/^SECRETS_ENV_FILE=//p' "$ENV_FILE" | tail -n 1)
  if [ -z "$v" ] && [ -n "$secrets" ] && [ -f "$secrets" ]; then
    v=$(sed -n "s/^$1=//p" "$secrets" | tail -n 1)
  fi
  printf '%s' "$v"
}

owner_pw=$(get POSTGRES_PASSWORD)
[ -n "$owner_pw" ] || die "POSTGRES_PASSWORD 없음"
[ -n "$(get ARCHIVE_S3_BUCKET)" ] || die "ARCHIVE_S3_BUCKET이 비어 있어요 (docs/DOCUMENT_ARCHIVE.md)"

admin() {
  docker exec \
    -e DATABASE_URL="postgres://audeniq_owner:${owner_pw}@postgres/audeniq_prod" \
    -e ARCHIVE_S3_ENDPOINT="$(get ARCHIVE_S3_ENDPOINT)" \
    -e ARCHIVE_S3_BUCKET="$(get ARCHIVE_S3_BUCKET)" \
    -e ARCHIVE_S3_ACCESS_KEY_ID="$(get ARCHIVE_S3_ACCESS_KEY_ID)" \
    -e ARCHIVE_S3_SECRET_ACCESS_KEY="$(get ARCHIVE_S3_SECRET_ACCESS_KEY)" \
    -e ARCHIVE_S3_REGION="$(get ARCHIVE_S3_REGION)" \
    -e ARCHIVE_OBJECT_LOCK="$(get ARCHIVE_OBJECT_LOCK)" \
    "$API" audeniq-admin --operator "$OPERATOR" "$@"
}

recipients() {
  local f
  f=${BACKUP_RECIPIENTS_FILE:-$(get BACKUP_RECIPIENTS_FILE)}
  if [ -z "$f" ] || [ ! -f "$f" ]; then
    die "BACKUP_RECIPIENTS_FILE(age 공개키) 없음"
  fi
  command -v age >/dev/null || die "age가 설치돼 있어야 해요"
  age -R "$f" </dev/null >/dev/null
  printf '%s' "$f"
}

# 호스트 파일 하나를 WORM 버킷 KEY로 (API 컨테이너를 거쳐) 올린다.
upload() {
  local file=$1 key=$2 days=$3 inside
  inside="/tmp/audeniq-archive-$$-$(basename "$file")"
  docker cp "$file" "$API:$inside"
  if admin archive put-backup "$inside" "$key" --retain-days "$days"; then
    docker exec "$API" rm -f "$inside"
  else
    docker exec "$API" rm -f "$inside"
    return 1
  fi
}

umask 077
case "${1:-}" in
  daily)
    admin archive anchor
    admin seal verify
    # 이름이 고정 형식(audeniq_prod-*.dump.age)이라 ls -t로 최신 파일을 고른다
    # shellcheck disable=SC2012
    latest=$(ls -1t "$BACKUP_DIR"/audeniq_prod-*.dump.age 2>/dev/null | head -n 1 || true)
    if [ -n "$latest" ]; then
      upload "$latest" "backups/$(basename "$latest")" 35
    else
      echo "archive: pg_dump 백업이 아직 없어요 ($BACKUP_DIR)"
    fi
    ;;
  monthly)
    admin archive run
    admin archive verify --deep
    ;;
  wal)
    rcpt=$(recipients)
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    # archive_command가 쌓은 WAL 조각 (가장 최근 것은 쓰는 중일 수 있어 건너뛴다)
    mapfile -t segs < <(docker exec "$PG" sh -c 'cd /var/lib/postgresql/wal-archive 2>/dev/null && ls -1 | sort | head -n -1' || true)
    for seg in "${segs[@]}"; do
      [[ "$seg" =~ ^[0-9A-F]{24}(\.partial|\.backup|\.history)?$|^[0-9A-F]{8}\.history$ ]] || continue
      docker exec "$PG" cat "/var/lib/postgresql/wal-archive/$seg" | age -R "$rcpt" > "$work/$seg.age"
      upload "$work/$seg.age" "pitr/wal/$seg.age" 35
      docker exec "$PG" rm -f "/var/lib/postgresql/wal-archive/$seg"
      rm -f "$work/$seg.age"
    done
    echo "archive: wal $(date -u +%FT%TZ) uploaded=${#segs[@]}"
    ;;
  basebackup)
    rcpt=$(recipients)
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    name="base-$(date -u +%Y%m%dT%H%M%SZ).tar.gz.age"
    docker exec "$PG" pg_basebackup -U audeniq_owner -D - -Ft -X fetch -z | age -R "$rcpt" > "$work/$name"
    upload "$work/$name" "pitr/base/$name" 35
    echo "archive: basebackup $(date -u +%FT%TZ) $name $(du -h "$work/$name" | cut -f1)"
    ;;
  purge)
    admin archive purge-expired
    ;;
  *)
    die "사용: archive.sh daily|monthly|wal|basebackup|purge"
    ;;
esac
