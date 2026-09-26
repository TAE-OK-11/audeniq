#!/usr/bin/env bash
# AUDENIQ 백엔드 배포 — GHCR 이미지를 digest로 고정해 docker compose로 올린다.
#
#   ./deploy.sh ghcr.io/tae-ok-11/audeniq@sha256:...   # 이 이미지로 배포 (권장: digest)
#   ./deploy.sh ghcr.io/tae-ok-11/audeniq:sha-<commit>  # 태그도 가능 (받은 뒤 digest로 고정)
#   ./deploy.sh --rollback                              # 직전 이미지로 되돌리기
#   ./deploy.sh --status                                # 지금 이미지·컨테이너 상태
#
# 같은 폴더에 compose.production.yaml, bootstrap.sql, grants.sql, production.env(비밀),
# tunnel-token(비밀)이 있어야 한다. production.env의 AUDENIQ_IMAGE 줄을 이 스크립트가 바꾼다.
# 비공개 패키지면 먼저 `docker login ghcr.io` (read:packages 토큰) 하거나,
# GHCR_USER와 GHCR_TOKEN_STDIN=1을 주고 토큰을 표준입력으로 넘긴다 (GitHub Actions 배포가 이렇게 한다).
set -euo pipefail

cd "$(dirname "$0")"
ENV_FILE=production.env
COMPOSE=(docker compose -p audeniq-production -f compose.production.yaml --env-file "$ENV_FILE")
HISTORY=deploy-history.log

die() { echo "deploy: $*" >&2; exit 1; }
current_image() { sed -n 's/^AUDENIQ_IMAGE=//p' "$ENV_FILE" | tail -n 1; }

[ -f "$ENV_FILE" ] || die "$ENV_FILE 이 없어요 (production.env.example 참고, chmod 600)"
[ -f compose.production.yaml ] || die "compose.production.yaml 이 없어요"

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
if ! "${COMPOSE[@]}" up -d --remove-orphans; then
  "${COMPOSE[@]}" logs --tail 80 migrate grants api worker || true
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
