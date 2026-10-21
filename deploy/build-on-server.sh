#!/usr/bin/env bash
# CI(CircleCI·GitHub Actions)를 쓸 수 없을 때: 운영 서버에서 직접 백엔드 이미지를 빌드해 배포한다.
# CI 빌드와 같은 deploy/Dockerfile·release 프로필이고, 서버 아키텍처(amd64/arm64) 하나만 만든다.
#
#   ./build-on-server.sh                # origin/main 최신 커밋
#   ./build-on-server.sh <커밋 해시>     # 그 커밋
#
# 한 번 준비 (서버에서):
#   git clone https://github.com/TAE-OK-11/audeniq.git /opt/audeniq-src
#     (비공개 저장소면 Contents 읽기 토큰으로: git clone https://x-access-token:<토큰>@github.com/TAE-OK-11/audeniq.git ...
#      clone 뒤 `git -C /opt/audeniq-src remote set-url origin https://github.com/TAE-OK-11/audeniq.git` 로 토큰을 지우고
#      fetch 때는 GIT_ASKPASS나 credential helper를 쓴다)
#   엔진 저장소(audeniq-media) 읽기 토큰을 파일로: install -m 600 /dev/null ~/.audeniq-engine-token, 토큰을 넣는다
#   GHCR에 올릴 수 있게 로그인 (write:packages): echo '<PAT>' | docker login ghcr.io -u <github 아이디> --password-stdin
#
# 환경 변수 (선택):
#   AUDENIQ_SRC          저장소 위치 (기본 /opt/audeniq-src)
#   DEPLOY_PATH          배포 폴더 (기본 /opt/audeniq, production.env·tunnel-token이 있는 곳)
#   ENGINE_TOKEN_FILE    엔진 저장소 읽기 토큰 파일 (기본 ~/.audeniq-engine-token)
#   BACKEND_CPU_VARIANT  portable(기본), ampere-a4, ampere-a1, graviton4, x86-64-v3 — CI와 같은 이름
#   BUILD_ONLY=1         이미지만 만들고 배포는 하지 않는다 (출력된 digest로 나중에 ./deploy.sh)
#
# 빌드는 CPU·메모리를 많이 쓴다 (release: fat LTO). 첫 빌드는 수십 분 걸릴 수 있고, 그동안 서비스가 느려질 수 있다.
# 메모리가 8GB보다 적으면 스왑을 먼저 늘린다. 같은 서버의 다음 빌드는 Docker 캐시로 빨라진다.
set -euo pipefail

die() { echo "build-on-server: $*" >&2; exit 1; }

SRC=${AUDENIQ_SRC:-/opt/audeniq-src}
DEPLOY_PATH=${DEPLOY_PATH:-/opt/audeniq}
TOKEN_FILE=${ENGINE_TOKEN_FILE:-$HOME/.audeniq-engine-token}
IMAGE=ghcr.io/tae-ok-11/audeniq

[ -d "$SRC/.git" ] || die "저장소가 없어요: $SRC (머리말의 git clone 참고)"
[ -f "$DEPLOY_PATH/production.env" ] || die "배포 폴더가 아니에요: $DEPLOY_PATH (production.env 없음)"
[ -s "$TOKEN_FILE" ] || die "엔진 저장소 읽기 토큰 파일이 없어요: $TOKEN_FILE"
[ "$(stat -c %a "$TOKEN_FILE")" = 600 ] || die "토큰 파일은 chmod 600이어야 해요: $TOKEN_FILE"
docker buildx version >/dev/null 2>&1 || die "docker buildx가 필요해요 (Docker 23 이상)"

case "$(uname -m)" in
  x86_64) arch=amd64 ;;
  aarch64) arch=arm64 ;;
  *) die "지원하지 않는 서버 아키텍처: $(uname -m)" ;;
esac
variant=${BACKEND_CPU_VARIANT:-portable}
case "$arch:$variant" in
  amd64:portable|amd64:) cpu=x86-64 ;;
  amd64:x86-64-v3) cpu=x86-64-v3 ;;
  arm64:portable|arm64:) cpu=generic ;;
  arm64:ampere-a4) cpu=ampere1a ;;
  arm64:ampere-a1) cpu=neoverse-n1 ;;
  arm64:graviton4) cpu=neoverse-v2 ;;
  *) die "이 서버($arch)에 맞지 않는 BACKEND_CPU_VARIANT: $variant" ;;
esac
# 빌드 전에 이 서버 CPU가 그 대상을 실행할 수 있는지 확인한다 (deploy.sh와 같은 검사)
bash "$SRC/deploy/check-cpu.sh" "$arch" "$cpu"

echo "build-on-server: 저장소 받는 중"
git -C "$SRC" fetch --quiet origin main
target=${1:-origin/main}
git -C "$SRC" checkout --quiet --detach "$target"
sha=$(git -C "$SRC" rev-parse HEAD)
[ -z "$(git -C "$SRC" status --porcelain)" ] || die "저장소에 커밋하지 않은 변경이 있어요: $SRC"
tag="$IMAGE:sha-$sha-server-$arch${variant:+-$variant}"
echo "build-on-server: $sha ($arch, cpu=$cpu) → $tag"

meta=$(mktemp)
trap 'rm -f "$meta"' EXIT
docker buildx build \
  --file "$SRC/deploy/Dockerfile" \
  --platform "linux/$arch" \
  --build-arg CARGO_PROFILE=release \
  --build-arg TARGET_CPU="$cpu" \
  --secret id=github_token,src="$TOKEN_FILE" \
  --label org.opencontainers.image.title=audeniq \
  --label "org.opencontainers.image.revision=$sha" \
  --label io.audeniq.target-cpu="$cpu" \
  --label io.audeniq.built-on=server \
  --tag "$tag" \
  --push \
  --metadata-file "$meta" \
  "$SRC"
digest=$(sed -n 's/.*"containerimage.digest": *"\([^"]*\)".*/\1/p' "$meta" | head -n 1)
[ -n "$digest" ] || die "올린 이미지의 digest를 찾지 못했어요"
ref="$IMAGE@$digest"
echo "build-on-server: 이미지 $ref"

if [ "${BUILD_ONLY:-}" = 1 ]; then
  echo "build-on-server: 배포하려면: cd $DEPLOY_PATH && ./deploy.sh $ref"
  exit 0
fi

# 배포 파일은 이미지(마이그레이션)와 같은 커밋이어야 한다. 비밀 파일(production.env, tunnel-token)은 건드리지 않는다.
echo "build-on-server: 같은 커밋의 배포 파일을 $DEPLOY_PATH 로 복사"
for f in compose.production.yaml bootstrap.sql grants.sql deploy.sh check-cpu.sh pg-upgrade.sh backup.sh restore-backup.sh archive.sh build-on-server.sh; do
  [ -f "$SRC/deploy/$f" ] && install -m "$( [ "${f##*.}" = sh ] && echo 755 || echo 644 )" "$SRC/deploy/$f" "$DEPLOY_PATH/$f"
done
rm -rf "$DEPLOY_PATH/pgbouncer.new" && cp -r "$SRC/deploy/pgbouncer" "$DEPLOY_PATH/pgbouncer.new"
rm -rf "$DEPLOY_PATH/pgbouncer" && mv "$DEPLOY_PATH/pgbouncer.new" "$DEPLOY_PATH/pgbouncer"

cd "$DEPLOY_PATH"
exec ./deploy.sh "$ref"
