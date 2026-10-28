#!/usr/bin/env bash
# AUDENIQ 백엔드 배포 — GHCR 이미지를 digest로 고정해 docker compose로 올린다.
#
#   ./deploy.sh ghcr.io/tae-ok-11/audeniq@sha256:...   # 이 이미지로 배포 (권장: digest)
#   ./deploy.sh ghcr.io/tae-ok-11/audeniq:sha-<commit>  # 태그도 가능 (받은 뒤 digest로 고정)
#   ./deploy.sh --rollback                              # 직전 이미지로 되돌리기
#   ./deploy.sh --status                                # 지금 이미지·컨테이너 상태
#
# 같은 폴더에 compose.production.yaml, compose.tls.yaml, bootstrap.sql, grants.sql, check-cpu.sh, pgbouncer/,
# production.env(비밀)가 있어야 한다. 앞단은 CloudFront → 서버 443(TLS_BIND)이고 터널(cloudflared)은 없다. production.env의 AUDENIQ_IMAGE 줄을 이 스크립트가 바꾼다.
# 비공개 패키지면 먼저 `docker login ghcr.io` (read:packages 토큰) 하거나,
# GHCR_USER와 GHCR_TOKEN_STDIN=1을 주고 토큰을 표준입력으로 넘긴다 (GitHub Actions 배포가 이렇게 한다).
set -euo pipefail

# Help is read-only and does not require production secrets or Docker.
case "${1:-}" in -h|--help) sed -n '2,12p' "$0"; exit 0 ;; esac

cd "$(dirname "$0")"
ENV_FILE=production.env
COMPOSE=(docker compose -p audeniq-production -f compose.production.yaml --env-file "$ENV_FILE")
# 서버에만 있는 조정은 compose.override.yaml에 둔다
[ -f compose.override.yaml ] && COMPOSE+=(-f compose.override.yaml)
HISTORY=deploy-history.log

die() { echo "deploy: $*" >&2; exit 1; }
# Fixed phase labels only: never record commands, environment or probe output.
TIMING_START=$SECONDS
TIMING_LAST=$SECONDS
TIMING_PHASE=preflight
REGISTRY_LOGGED_IN=0
timing_phase() {
  if [ "${DEPLOY_TIMING:-0}" = 1 ]; then
    printf 'deploy-timing phase=%s seconds=%s total_seconds=%s\n' \
      "$TIMING_PHASE" "$((SECONDS - TIMING_LAST))" "$((SECONDS - TIMING_START))" >&2
  fi
  TIMING_LAST=$SECONDS
  TIMING_PHASE=$1
}
deploy_exit() {
  local code=$?
  timing_phase finished
  if [ "${DEPLOY_TIMING:-0}" = 1 ]; then
    printf 'deploy-timing exit=%s total_seconds=%s\n' "$code" "$((SECONDS - TIMING_START))" >&2
  fi
  if [ "$REGISTRY_LOGGED_IN" = 1 ]; then docker logout "$REGISTRY" >/dev/null 2>&1 || true; fi
  return "$code"
}
trap deploy_exit EXIT
current_image() { sed -n 's/^AUDENIQ_IMAGE=//p' "$ENV_FILE" | tail -n 1; }

# Serialize releases before mutating environment, migrations or containers.
umask 077
exec 9>.deploy.lock
flock -n 9 || die "another deployment is running"
[ ! -L "$ENV_FILE" ] || die "production.env must not be a symlink"
[ -f "$ENV_FILE" ] || die "$ENV_FILE 이 없어요 (production.env.example 참고, chmod 600)"
[ "$(stat -c '%u %a' "$ENV_FILE")" = "$(id -u) 600" ] || die "production.env must be owned by the deploy user with mode 0600"
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
# 터널이 없어서 TLS_BIND가 없으면 바깥에서 api에 닿을 길이 없다
if [ -z "$(sed -n 's/^TLS_BIND=//p' "$ENV_FILE" | tail -n 1)" ]; then
  echo "deploy: 경고 — production.env에 TLS_BIND가 없어 api가 바깥(CloudFront)에서 닿지 않아요 (TLS_BIND=0.0.0.0:8443)" >&2
else
  [ -f compose.tls.yaml ] || die "TLS_BIND가 있는데 compose.tls.yaml 이 없어요 (저장소의 deploy/compose.tls.yaml을 함께 복사)"
  # TLS_ACME_DOMAIN이 있으면 api가 Let's Encrypt에서 직접 받는다. 없으면 ./tls 파일이 있어야 한다
  if [ -z "$(sed -n 's/^TLS_ACME_DOMAIN=//p' "$ENV_FILE" | tail -n 1)" ]; then
    if [ ! -s tls/fullchain.pem ] || [ ! -s tls/privkey.pem ]; then die "TLS_BIND가 있는데 인증서가 없어요: production.env에 TLS_ACME_DOMAIN(Let's Encrypt 자동) 또는 ./tls-cert.sh issue <주소> <메일>"; fi
  fi
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
  REGISTRY_LOGGED_IN=1
fi

timing_phase image_pull
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

timing_phase image_verification
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
# Binary presence and CPU metadata need only one shell-only container; no
# AUDENIQ binary executes until the host CPU has passed check-cpu.sh.
target_cpu=$(docker run --rm --network none --cap-drop ALL --read-only \
  --security-opt no-new-privileges --entrypoint sh "$PINNED" -c '
  for b in audeniq-api audeniq-worker audeniq-migrate audeniq-music; do
    command -v "$b" >/dev/null || { echo "missing $b" >&2; exit 1; }
  done
  if [ -f /usr/local/share/audeniq/target-cpu ]; then
    cat /usr/local/share/audeniq/target-cpu
  else
    # Legacy images have no metadata and used the default Rust CPU.
    case "$(uname -m)" in x86_64) echo x86-64 ;; aarch64) echo generic ;; *) exit 1 ;; esac
  fi') || die "이미지 CPU 정보 확인 실패: $PINNED"
bash ./check-cpu.sh "$image_arch" "$target_cpu"

# Refuse a kernel/container profile that would leave upload parsers unconfined.
docker run --rm --network none --cap-drop ALL --read-only   --security-opt no-new-privileges --entrypoint audeniq-admin "$PINNED" upload-sandbox-check   || die "업로드 샌드박스 실행 실패: Landlock ABI 3 이상과 seccomp 허용을 확인해 주세요"

# Resolve every loaded compose file (including inactive profiles) without
# printing the interpolated model: it contains production secrets. Only the
# explicitly built PgBouncer is allowed to retain a build definition. Building
# arbitrary overrides could replace the verified API/worker image locally.
command -v python3 >/dev/null || die "python3 is required to check Compose build definitions"
if ! extra_builds=$(AUDENIQ_IMAGE="$PINNED" "${COMPOSE[@]}" --profile '*' config --format json 2>/dev/null | python3 -c '
import json, sys
model = json.load(sys.stdin)
services = model["services"]
for name, service in sorted(services.items()):
    if "build" in service and service["build"] is not None and name != "pgbouncer":
        print(name)
' 2>/dev/null); then
  die "cannot validate Compose build definitions; check compose files and required environment values (config --format json requires Docker Compose v2)"
fi
if [ -n "$extra_builds" ]; then
  die "unsupported buildable Compose services: $extra_builds; prepare each image separately, set its image reference and remove its build definition before deploying with --no-build — rollback: ./deploy.sh --rollback"
fi

timing_phase environment_update
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
timing_phase dependency_pull
"${COMPOSE[@]}" pull --quiet --ignore-buildable \
  || echo "deploy: 경고 — postgres 등 이미지 갱신 실패, 서버에 있는 이미지로 계속" >&2
timing_phase pgbouncer_build
# Keep the base refresh and let Docker validate all build inputs/cache keys.
# Build once, then forbid implicit rebuilds in both up calls. If the build
# with base refresh fails, retry without --pull and retain the rollback hint.
if ! "${COMPOSE[@]}" build --pull --quiet pgbouncer; then
  echo "deploy: 경고 — pgbouncer build --pull 실패, --pull 없이 다시 빌드" >&2
  "${COMPOSE[@]}" build --quiet pgbouncer \
    || die "DB startup (postgres → migrate → grants → pgbouncer) 실패; 확인: docker compose logs migrate grants pgbouncer — 되돌리려면: ./deploy.sh --rollback"
fi
# Studio 화면 폴더(api가 읽기 전용으로 마운트): docker가 root 소유로 만들지 않게 배포 사용자가 먼저 만든다.
# 화면 파일은 studio-release.sh가 넣는다 (deploy/STUDIO_SERVING.md)
mkdir -p studio/releases
# Authenticate both runtime roles directly and through the pooler BEFORE
# replacing API/worker. Do not treat an open TCP socket as DB readiness.
timing_phase database_start_migrate_grants
if ! "${COMPOSE[@]}" up -d --no-build postgres migrate grants pgbouncer; then
  die "DB startup (postgres → migrate → grants → pgbouncer) 실패; 확인: docker compose logs migrate grants pgbouncer — 되돌리려면: ./deploy.sh --rollback"
fi
wait_database() {
  local deadline=$((SECONDS + 120)) result code
  while (( SECONDS < deadline )); do
    if result=$("${COMPOSE[@]}" exec -T pgbouncer /usr/local/bin/pgbouncer-entrypoint --healthcheck 2>/dev/null); then
      echo "deploy: $result"
      return 0
    else
      code=$?
    fi
    # Print only the probe's bounded labels, never arbitrary container output.
    result=$(printf '%s\n' "$result" | grep '^db_check path=' || true)
    case "$code" in
      20|21) die "DB login/config broken: $result; deploy/db-ready-diagnosis.md의 직접/풀러 인증 점검 — 되돌리려면: ./deploy.sh --rollback" ;;
    esac
    echo "deploy: DB not ready yet ($((deadline - SECONDS))s remaining): ${result:-pgbouncer probe unavailable}"
    sleep 1
  done
  die "DB readiness timeout (120s): ${result:-pgbouncer probe unavailable}; postgres/pgbouncer 로그 확인 — 되돌리려면: ./deploy.sh --rollback"
}
timing_phase database_readiness
wait_database
# All permitted build definitions were handled by the PgBouncer build above.
timing_phase runtime_start_and_drain
if ! "${COMPOSE[@]}" up -d --no-build --remove-orphans; then
  "${COMPOSE[@]}" logs --tail 80 migrate grants pgbouncer api worker || true
  die "compose up 실패 — 이전 이미지로 되돌리려면: ./deploy.sh --rollback"
fi
if [ "$av_enabled" = false ]; then
  # Also stop engines left by an earlier enabled deployment.
  "${COMPOSE[@]}" --profile antivirus stop antivirus antivirus-update
fi

timing_phase runtime_stability
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

timing_phase api_readiness
# 떠 있는 것만으로는 부족하다: api가 /ready(DB 포함)에 답할 때까지 기다린다 (compose healthcheck)
api_id=$("${COMPOSE[@]}" ps -q api)
health=none
ready_deadline=$((SECONDS + 120))
while (( SECONDS < ready_deadline )); do
  health=$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' "$api_id")
  case "$health" in
    healthy)
      # A previous healthy result must not hide a crashed/restarted runtime.
      for svc in api worker; do
        id=$("${COMPOSE[@]}" ps -q "$svc")
        [ -n "$id" ] || die "$svc disappeared during readiness gate"
        [ "$(docker inspect --format '{{.State.Running}} {{.RestartCount}}' "$id")" = "true ${start[$svc]}" ] || die "$svc restarted or stopped during readiness gate"
      done
      break ;;
    none) die "api healthcheck missing — 되돌리려면: ./deploy.sh --rollback" ;;
  esac
  # Catch a DB regression while the API is starting, with classified checks.
  if probe=$("${COMPOSE[@]}" exec -T pgbouncer /usr/local/bin/pgbouncer-entrypoint --healthcheck 2>/dev/null); then
    echo "deploy: API /ready waiting ($health); DB authenticated"
  else
    probe_code=$?
    case "$probe_code" in
      20|21) die "DB login/config broken: $(printf '%s\n' "$probe" | grep '^db_check path=' || true) — 되돌리려면: ./deploy.sh --rollback" ;;
      *) echo "deploy: API /ready waiting ($health); DB not ready yet" ;;
    esac
  fi
  sleep 1
done
case "$health" in
  healthy) ;;
  *)
    "${COMPOSE[@]}" logs --tail 80 api || true
    die "api가 준비되지 않았어요 ($health) — 되돌리려면: ./deploy.sh --rollback"
    ;;
esac

printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PINNED" >> "$HISTORY"
echo "deploy: 완료 $PINNED"
