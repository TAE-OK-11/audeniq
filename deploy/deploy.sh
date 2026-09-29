#!/usr/bin/env bash
# AUDENIQ 백엔드 배포 — GHCR 이미지를 digest로 고정해 docker compose로 올린다.
#
#   ./deploy.sh ghcr.io/tae-ok-11/audeniq@sha256:...   # 이 이미지로 배포 (권장: digest)
#   ./deploy.sh ghcr.io/tae-ok-11/audeniq:sha-<commit>  # 태그도 가능 (받은 뒤 digest로 고정)
#   ./deploy.sh --rollback                              # 직전 이미지로 되돌리기
#   ./deploy.sh --status                                # 지금 이미지·컨테이너 상태
#
# 같은 폴더에 compose.production.yaml, bootstrap.sql, grants.sql, pgbouncer/, production.env(비밀),
# tunnel-token(비밀)이 있어야 한다. production.env의 AUDENIQ_IMAGE 줄을 이 스크립트가 바꾼다.
# 비공개 패키지면 먼저 `docker login ghcr.io` (read:packages 토큰) 하거나,
# GHCR_USER와 GHCR_TOKEN_STDIN=1을 주고 토큰을 표준입력으로 넘긴다 (GitHub Actions 배포가 이렇게 한다).
set -euo pipefail

cd "$(dirname "$0")"
ENV_FILE=production.env
COMPOSE=(docker compose -p audeniq-production -f compose.production.yaml --env-file "$ENV_FILE")
# 서버에만 있는 조정 (예: 터널 토큰 전 임시 포트 매핑)은 compose.override.yaml에 둔다
[ -f compose.override.yaml ] && COMPOSE+=(-f compose.override.yaml)
HISTORY=deploy-history.log

die() { echo "deploy: $*" >&2; exit 1; }
current_image() { sed -n 's/^AUDENIQ_IMAGE=//p' "$ENV_FILE" | tail -n 1; }

[ -f "$ENV_FILE" ] || die "$ENV_FILE 이 없어요 (production.env.example 참고, chmod 600)"
[ -f compose.production.yaml ] || die "compose.production.yaml 이 없어요"
[ -f pgbouncer/Dockerfile ] || die "pgbouncer/ 폴더가 없어요 (저장소의 deploy/pgbouncer를 함께 복사)"
# PostgreSQL 18은 새 볼륨을 쓴다: 옛 17 데이터가 있는데 아직 옮기지 않았으면 빈 DB로 뜨지 않게 멈춘다
if docker volume inspect audeniq-production_pg_prod >/dev/null 2>&1 \
   && ! docker volume inspect audeniq-production_pg18_prod >/dev/null 2>&1; then
  die "PostgreSQL 17 데이터가 있어요. 먼저 ./pg-upgrade.sh 로 18로 옮겨 주세요"
fi

case "${1:-}" in
  --status)
    echo "image: $(current_image)"
    "${COMPOSE[@]}" ps -a
    exit 0
    ;;
  --rollback)
    [ -s "$HISTORY" ] || die "되돌릴 기록이 없어요 ($HISTORY)"
    prev=$(awk -v cur="$(current_image)" '$2 != cur { ref = $2 } END { print ref }' "$HISTORY")
    [ -n "$prev" ] || die "현재와 다른 이전 이미지가 기록에 없어요"
    echo "deploy: 직전 이미지로 되돌려요 → $prev"
    echo "deploy: 주의: 새 이미지가 적용한 DB 마이그레이션은 되돌리지 않아요"
    set -- "$prev"
    ;;
  ''|-h|--help)
    sed -n '2,12p' "$0"; exit 2
    ;;
esac

REF=$1
REGISTRY=${DEPLOY_REGISTRY:-ghcr.io}
case "$REF" in
  "$REGISTRY"/*) ;;
  *) die "$REGISTRY 이미지만 배포해요: $REF" ;;
esac

if [ "${GHCR_TOKEN_STDIN:-}" = 1 ]; then
  [ -n "${GHCR_USER:-}" ] || die "GHCR_USER가 필요해요"
  docker login "$REGISTRY" -u "$GHCR_USER" --password-stdin >/dev/null
  trap 'docker logout "$REGISTRY" >/dev/null 2>&1 || true' EXIT
fi

echo "deploy: 이미지 받는 중 $REF"
docker pull --quiet "$REF" >/dev/null
repo=${REF%@*}; repo=${repo%:*}
if [[ "$REF" == *@sha256:* ]]; then
  PINNED=$REF
else
  digest=$(docker image inspect --format '{{range .RepoDigests}}{{println .}}{{end}}' "$REF" | grep -m1 "^$repo@sha256:" || true)
  [ -n "$digest" ] || die "digest를 찾지 못했어요: $REF"
  PINNED=$digest
fi

# 이미지가 필요한 바이너리를 갖췄는지 먼저 확인 (잘못된 이미지로 DB 마이그레이션이 도는 것을 막는다)
docker run --rm --entrypoint sh "$PINNED" -c \
  'for b in audeniq-api audeniq-worker audeniq-migrate ffprobe; do command -v "$b" >/dev/null || { echo "missing $b"; exit 1; }; done' \
  || die "이미지 확인 실패: $PINNED"

# 이미지가 빌드된 CPU 기준선을 이 서버가 지원하는지 확인한다. x86-64-v3 이미지를 AVX2가 없는
# CPU에서 돌리면 시작하자마자 SIGILL로 죽는다 (마이그레이션 전에 막는다). 기록이 없는 예전
# 이미지는 일반 x86-64다.
target_cpu=$(docker run --rm --entrypoint cat "$PINNED" /usr/local/share/audeniq/target-cpu 2>/dev/null || echo x86-64)
case "$target_cpu" in
  x86-64) need="" ;;
  x86-64-v2) need="cx16 lahf_lm popcnt sse4_1 sse4_2 ssse3" ;;
  x86-64-v3) need="cx16 lahf_lm popcnt sse4_1 sse4_2 ssse3 avx avx2 bmi1 bmi2 f16c fma abm movbe xsave" ;;
  *) die "알 수 없는 CPU 기준선: $target_cpu" ;;
esac
flags=" $(grep -m1 '^flags' /proc/cpuinfo | cut -d: -f2) "
missing=""
for f in $need; do
  [[ "$flags" == *" $f "* ]] || missing="$missing $f"
done
[ -z "$missing" ] || die "이 서버 CPU는 이미지 기준선 $target_cpu 를 지원하지 않아요 (없는 기능:$missing). TARGET_CPU를 비워 다시 빌드하세요"

PREVIOUS=$(current_image)
tmp=$(mktemp "$ENV_FILE.XXXXXX")
chmod 600 "$tmp"
if grep -q '^AUDENIQ_IMAGE=' "$ENV_FILE"; then
  sed "s#^AUDENIQ_IMAGE=.*#AUDENIQ_IMAGE=$PINNED#" "$ENV_FILE" > "$tmp"
else
  { cat "$ENV_FILE"; echo "AUDENIQ_IMAGE=$PINNED"; } > "$tmp"
fi
mv "$tmp" "$ENV_FILE"

echo "deploy: $PREVIOUS"
echo "     → $PINNED"
# migrate → grants가 성공해야 api·worker가 새 이미지로 바뀐다 (compose depends_on)
# --build: pgbouncer 이미지는 서버에서 deploy/pgbouncer로 만든다 (바뀐 게 없으면 캐시)
if ! "${COMPOSE[@]}" up -d --build --remove-orphans; then
  "${COMPOSE[@]}" logs --tail 80 migrate grants pgbouncer api worker || true
  die "compose up 실패 — 이전 이미지로 되돌리려면: ./deploy.sh --rollback"
fi

# api·worker가 새 이미지로 재시작 없이 30초 동안 떠 있는지 지켜본다
declare -A start
for svc in api worker; do
  id=$("${COMPOSE[@]}" ps -q "$svc")
  [ -n "$id" ] || die "$svc 컨테이너가 없어요"
  [ "$(docker inspect --format '{{.Config.Image}}' "$id")" = "$PINNED" ] || die "$svc 가 새 이미지로 바뀌지 않았어요"
  start[$svc]=$(docker inspect --format '{{.RestartCount}}' "$id")
done
bad=""
for _ in $(seq 1 15); do
  sleep 2
  for svc in api worker; do
    id=$("${COMPOSE[@]}" ps -q "$svc")
    read -r status restarts < <(docker inspect --format '{{.State.Status}} {{.RestartCount}}' "$id")
    if [ "$status" != running ] || [ "$restarts" != "${start[$svc]}" ]; then bad="$bad $svc($status, 재시작 $restarts)"; fi
  done
  [ -z "$bad" ] || break
done
if [ -n "$bad" ]; then
  "${COMPOSE[@]}" logs --tail 80 api worker || true
  die "컨테이너가 안정적으로 뜨지 않았어요:$bad — 되돌리려면: ./deploy.sh --rollback"
fi

printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PINNED" >> "$HISTORY"
echo "deploy: 완료 $PINNED"
