#!/usr/bin/env bash
# AUDENIQ 백엔드 배포 — GHCR 이미지를 digest로 고정해 docker compose로 올린다.
#
#   ./deploy.sh ghcr.io/tae-ok-11/audeniq@sha256:...   # 이 이미지로 배포 (권장: digest)
#   ./deploy.sh ghcr.io/tae-ok-11/audeniq:sha-<commit>  # 태그도 가능 (받은 뒤 digest로 고정)
#   ./deploy.sh --rollback                              # 직전 이미지로 되돌리기
#   ./deploy.sh --status                                # 지금 이미지·컨테이너 상태
#
# 같은 폴더에 compose.production.yaml, bootstrap.sql, grants.sql, check-cpu.sh, pgbouncer/, production.env(비밀),
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
# 시크릿 매니저(AWS/GCP/OCI)에서 tmpfs로 받은 비밀값: production.env 뒤에 읽어 같은 이름을 덮는다
SECRETS_ENV_FILE=$(sed -n 's/^SECRETS_ENV_FILE=//p' "$ENV_FILE" | tail -n 1)
if [ -n "$SECRETS_ENV_FILE" ]; then
  if [ ! -f "$SECRETS_ENV_FILE" ] || [ -L "$SECRETS_ENV_FILE" ]; then
    die "시크릿을 먼저 tmpfs에 준비해 주세요 (audeniq-secrets secrets materialize): $SECRETS_ENV_FILE"
  fi
  [ "$(stat -f -c %T "$SECRETS_ENV_FILE")" = tmpfs ] || die "SECRETS_ENV_FILE은 tmpfs에 있어야 해요"
  [ "$(stat -c '%u %a' "$SECRETS_ENV_FILE")" = "$(id -u) 600" ] || die "SECRETS_ENV_FILE은 배포 사용자 소유 0600이어야 해요"
  ! grep -q '^AUDENIQ_IMAGE=' "$SECRETS_ENV_FILE" || die "SECRETS_ENV_FILE에 AUDENIQ_IMAGE를 둘 수 없어요"
  COMPOSE+=(--env-file "$SECRETS_ENV_FILE")
fi
[ -f compose.production.yaml ] || die "compose.production.yaml 이 없어요"
# AWS CloudFront가 부르는 HTTPS(서버 443)를 켰으면 포트를 여는 파일을 붙인다 (deploy/aws/README.md)
if [ -n "$(sed -n 's/^TLS_BIND=//p' "$ENV_FILE" | tail -n 1)" ]; then
  [ -f compose.tls.yaml ] || die "TLS_BIND가 있는데 compose.tls.yaml 이 없어요 (저장소의 deploy/compose.tls.yaml을 함께 복사)"
  [ -s tls/fullchain.pem ] && [ -s tls/privkey.pem ] || die "TLS_BIND가 있는데 ./tls 인증서가 없어요 (./tls-cert.sh issue <주소> <메일>)"
  COMPOSE+=(-f compose.tls.yaml)
fi
[ -f pgbouncer/Dockerfile ] || die "pgbouncer/ 폴더가 없어요 (저장소의 deploy/pgbouncer를 함께 복사)"
[ -f check-cpu.sh ] || die "check-cpu.sh 가 없어요 (저장소의 deploy/check-cpu.sh를 함께 복사)"
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

# Only enabled KMS requires the cloud-materialized tmpfs keyring.
read_feature() {
  local value
  value=$(sed -n "s/^$1=//p" "$ENV_FILE" | tail -n 1)
  value=${value:-false}
  case "$value" in true|false) printf '%s' "$value" ;; *) die "$1 은 true 또는 false여야 해요" ;; esac
}
kms_enabled=$(read_feature PAYOUT_KMS_ENABLED)
av_enabled=$(read_feature UPLOAD_AV_ENABLED)
if [ "$av_enabled" = true ]; then COMPOSE+=(--profile antivirus); fi
keydir=$(sed -n 's/^PAYOUT_KEYRING_DIR=//p' "$ENV_FILE" | tail -n 1)
keydir=${keydir:-/dev/shm/audeniq-secrets}
if [ "$kms_enabled" = true ]; then
  keyfile=$(sed -n 's/^PAYOUT_KEYRING_FILE=//p' "$ENV_FILE" | tail -n 1)
  [ "$keyfile" = /run/audeniq-keys/payout-keyring.json ] || die "PAYOUT_KEYRING_FILE=/run/audeniq-keys/payout-keyring.json 을 설정해 주세요"
  [ -f "$keydir/payout-keyring.json" ] || die "KMS 키를 먼저 tmpfs에 준비해 주세요 (audeniq-secrets kms materialize)"
  [ "$(stat -f -c %T "$keydir")" = tmpfs ] || die "키 폴더는 tmpfs여야 해요"
else
  # 개인정보(이메일·실명·연락처·예금주)는 암호문으로만 저장한다(0102): 키 없이는 API가 기동하지 않는다.
  local_key=$(sed -n 's/^PAYOUT_ACCOUNT_KEY=//p' "$ENV_FILE" | tail -n 1)
  local_ring=$(sed -n 's/^PAYOUT_KEYRING_FILE=//p' "$ENV_FILE" | tail -n 1)
  if [ -z "$local_key" ] && [ -z "$local_ring" ] && ! grep -qs '^PAYOUT_ACCOUNT_KEY=.' "${SECRETS_ENV_FILE:-/nonexistent}"; then
    die "개인정보 암호화 키가 없어요: PAYOUT_KMS_ENABLED=true(권장) 또는 PAYOUT_ACCOUNT_KEY를 설정해 주세요"
  fi
fi

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

# Architecture and CPU checks must precede the sandbox check and migrations:
# even audeniq-admin can SIGILL if a tuned image lands on the wrong CPU.
image_arch=$(docker image inspect --format '{{.Architecture}}' "$PINNED")
case "$image_arch" in
  amd64|arm64) ;;
  *) die "지원하지 않는 이미지 아키텍처: $image_arch" ;;
esac
# Refuse cross-architecture images even when the host has QEMU/binfmt enabled.
case "$(uname -m):$image_arch" in
  x86_64:amd64|aarch64:arm64) ;;
  *) die "서버와 이미지의 아키텍처가 달라요: $(uname -m)/$image_arch" ;;
esac
target_cpu=$(docker run --rm --network none --entrypoint sh "$PINNED" -c '
  if [ -f /usr/local/share/audeniq/target-cpu ]; then
    cat /usr/local/share/audeniq/target-cpu
  else
    # Legacy images have no metadata and used the default Rust CPU.
    case "$(uname -m)" in x86_64) echo x86-64 ;; aarch64) echo generic ;; *) exit 1 ;; esac
  fi') || die "이미지 CPU 정보 확인 실패: $PINNED"
bash ./check-cpu.sh "$image_arch" "$target_cpu"

# 이미지가 필요한 바이너리를 갖췄는지 먼저 확인 (잘못된 이미지로 DB 마이그레이션이 도는 것을 막는다)
docker run --rm --entrypoint sh "$PINNED" -c \
  'for b in audeniq-api audeniq-worker audeniq-migrate audeniq-music; do command -v "$b" >/dev/null || { echo "missing $b"; exit 1; }; done' \
  || die "이미지 확인 실패: $PINNED"

# Refuse a kernel/container profile that would leave upload parsers unconfined.
docker run --rm --network none --cap-drop ALL --read-only   --security-opt no-new-privileges --entrypoint audeniq-admin "$PINNED" upload-sandbox-check   || die "업로드 샌드박스 실행 실패: Landlock ABI 3 이상과 seccomp 허용을 확인해 주세요"

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
# DSP 전송 중이면 worker는 그 전송이 끝날 때까지(최대 WORKER_DELIVERY_DRAIN_SECONDS) 기다렸다가 내려간다
# compose up은 서버에 이미 있는 이미지를 다시 받지 않아, postgres는 처음 받은 18.x에, pgbouncer는 처음 만든
# Debian 패키지에 머문다. 배포마다 보안 패치를 받는다: 태그 이미지는 새로 받고, pgbouncer는 바탕 이미지를
# 새로 받아 만든다(바뀌었을 때만 패키지를 다시 설치). 새 postgres 이미지면 postgres가 한 번 다시 시작한다.
# 레지스트리에 닿지 않으면 지금 이미지로 배포를 계속한다
"${COMPOSE[@]}" pull --quiet --ignore-buildable \
  || echo "deploy: 경고 — postgres 등 이미지 갱신 실패, 서버에 있는 이미지로 계속" >&2
"${COMPOSE[@]}" build --pull --quiet pgbouncer \
  || echo "deploy: 경고 — pgbouncer 바탕 이미지 갱신 실패, 캐시로 계속" >&2
# Studio 화면 폴더(api가 읽기 전용으로 마운트): docker가 root 소유로 만들지 않게 배포 사용자가 먼저 만든다.
# 화면 파일은 studio-release.sh가 넣는다 (deploy/STUDIO_SERVING.md)
mkdir -p studio/releases
# --build: pgbouncer 이미지는 서버에서 deploy/pgbouncer로 만든다 (바뀐 게 없으면 캐시)
if ! "${COMPOSE[@]}" up -d --build --remove-orphans; then
  "${COMPOSE[@]}" logs --tail 80 migrate grants pgbouncer api worker || true
  die "compose up 실패 — 이전 이미지로 되돌리려면: ./deploy.sh --rollback"
fi
if [ "$av_enabled" = false ]; then
  # Also stop engines left by an earlier enabled deployment.
  "${COMPOSE[@]}" --profile antivirus stop antivirus antivirus-update
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

# 떠 있는 것만으로는 부족하다: api가 /ready(DB 포함)에 답할 때까지 기다린다 (compose healthcheck)
api_id=$("${COMPOSE[@]}" ps -q api)
health=none
for _ in $(seq 1 30); do
  health=$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' "$api_id")
  case "$health" in healthy|none) break ;; esac
  sleep 2
done
case "$health" in
  healthy|none) ;;
  *)
    "${COMPOSE[@]}" logs --tail 80 api || true
    die "api가 준비되지 않았어요 ($health) — 되돌리려면: ./deploy.sh --rollback"
    ;;
esac

printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PINNED" >> "$HISTORY"
echo "deploy: 완료 $PINNED"
