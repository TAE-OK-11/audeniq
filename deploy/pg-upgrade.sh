#!/usr/bin/env bash
# PostgreSQL 17 -> 18 업그레이드 (한 번만). 기존 데이터 볼륨(pg_prod)은 그대로 두고,
# 17로 덤프한 뒤 새 볼륨(pg18_prod)의 18에 복원한다. 실패하면 compose.production.yaml의
# postgres 이미지·볼륨을 17로 되돌리면 옛 데이터로 바로 돌아간다.
#
#   ./pg-upgrade.sh            # 업그레이드 (api·worker 잠시 멈춤)
#   ./pg-upgrade.sh --check    # 할 일이 있는지만 확인
#
# 끝나면 ./deploy.sh <이미지> 로 migrate·grants·서비스를 올린다.
set -euo pipefail

cd "$(dirname "$0")"
ENV_FILE=${ENV_FILE:-production.env}
PROJECT=${COMPOSE_PROJECT:-audeniq-production}
DB=${DB_NAME:-audeniq_prod}
OWNER=${DB_OWNER:-audeniq_owner}
OLD_IMAGE=${OLD_PG_IMAGE:-postgres:17-bookworm}
OLD_VOL="${PROJECT}_pg_prod"
NEW_VOL="${PROJECT}_pg18_prod"
COMPOSE=(docker compose -p "$PROJECT" -f compose.production.yaml --env-file "$ENV_FILE")
[ -f compose.override.yaml ] && COMPOSE+=(-f compose.override.yaml)
TEMP=audeniq-pg17-dump
TABLES="identity.users identity.orgs catalog.releases catalog.tracks catalog.assets distribution.distribution_packages distribution.identifier_assignments finance.ledger_entries operations.audit_events"

die() { echo "pg-upgrade: $*" >&2; exit 1; }
vol_exists() { docker volume inspect "$1" >/dev/null 2>&1; }

vol_exists "$OLD_VOL" || { echo "pg-upgrade: 옛 볼륨 $OLD_VOL 이 없어요 — 할 일 없음"; exit 0; }
if vol_exists "$NEW_VOL"; then
  echo "pg-upgrade: $NEW_VOL 이 이미 있어요 — 업그레이드가 끝난 상태예요"
  exit 0
fi
[ "${1:-}" = --check ] && { echo "pg-upgrade: $OLD_VOL(17) → $NEW_VOL(18) 업그레이드가 필요해요"; exit 3; }
[ -f "$ENV_FILE" ] || die "$ENV_FILE 이 없어요"

counts() { # $1 = container
  local sql="" t
  for t in $TABLES; do sql="$sql SELECT '$t', count(*) FROM $t UNION ALL"; done
  docker exec "$1" psql -U "$OWNER" -d "$DB" -AtX -c "${sql% UNION ALL}"
}
wait_ready() { # $1 = container
  for _ in $(seq 1 60); do
    docker exec "$1" pg_isready -U "$OWNER" -d "$DB" -q && return 0
    sleep 1
  done
  die "$1 이 준비되지 않았어요"
}

echo "pg-upgrade: 서비스 멈춤 (api·worker·pgbouncer·tunnel·postgres)"
"${COMPOSE[@]}" stop api worker pgbouncer tunnel postgres >/dev/null 2>&1 || true

echo "pg-upgrade: 옛 데이터를 17로 열어 덤프"
docker rm -f "$TEMP" >/dev/null 2>&1 || true
trap 'docker rm -f "$TEMP" >/dev/null 2>&1 || true' EXIT
docker run -d --name "$TEMP" -v "$OLD_VOL:/var/lib/postgresql/data" "$OLD_IMAGE" >/dev/null
wait_ready "$TEMP"
before=$(counts "$TEMP")
DUMP="pg17-$(date -u +%Y%m%dT%H%M%SZ).dump"
( umask 077; docker exec "$TEMP" pg_dump -U "$OWNER" -d "$DB" -Fc > "$DUMP" )
[ -s "$DUMP" ] || die "덤프가 비었어요"
docker rm -f "$TEMP" >/dev/null
echo "pg-upgrade: 덤프 $DUMP ($(du -h "$DUMP" | cut -f1))"

echo "pg-upgrade: 18을 새 볼륨으로 시작 (bootstrap.sql이 역할을 만든다)"
"${COMPOSE[@]}" up -d postgres >/dev/null
pg=$("${COMPOSE[@]}" ps -q postgres)
[ -n "$pg" ] || die "postgres 컨테이너가 없어요"
wait_ready "$pg"
docker exec "$pg" postgres --version

echo "pg-upgrade: 복원"
docker exec "$pg" dropdb -U "$OWNER" --if-exists "$DB"
docker exec -i "$pg" pg_restore -U "$OWNER" -d postgres --create --exit-on-error < "$DUMP"
after=$(counts "$pg")
if [ "$before" != "$after" ]; then
  echo "before:"; echo "$before"; echo "after:"; echo "$after"
  die "행 수가 달라요 — 옛 볼륨 $OLD_VOL 은 그대로예요 (compose를 17로 되돌리면 복구)"
fi
while IFS= read -r line; do echo "  $line"; done <<< "$after"
echo "pg-upgrade: 완료. 옛 볼륨 $OLD_VOL 과 덤프 $DUMP 는 확인 후 직접 지워 주세요."
echo "pg-upgrade: 이제 ./deploy.sh <이미지> 로 migrate·grants와 서비스를 올려요."
