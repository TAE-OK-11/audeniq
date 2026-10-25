#!/usr/bin/env bash
# AUDENIQ 새 버전 올리기 — 운영 서버에서 이것 하나만 실행하면 된다.
#
#   ./update.sh                  # main의 최신 백엔드 이미지로 (이미 그 버전이면 아무것도 안 함)
#   ./update.sh <커밋>           # 그 커밋의 이미지로 (sha 앞 7자 이상, 백엔드가 바뀐 커밋이어야 이미지가 있다)
#   ./update.sh --check          # 올릴 새 버전이 있는지만 본다
#   ./update.sh --force          # 같은 버전이어도 다시 배포
#   BACKEND_CPU_VARIANT=x86-64-v3 ./update.sh   # CPU 최적화 이미지 (기본 portable)
#
# 1) GHCR에서 이미지를 받고 2) 이미지에 적힌 커밋(org.opencontainers.image.revision)의 배포 파일
#    (compose·grants·bootstrap·deploy.sh·pgbouncer/ …)을 GitHub에서 받아 이 폴더에 바꿔 넣고 3) ./deploy.sh로 배포한다.
#    이미지와 배포 파일이 늘 같은 커밋이라 마이그레이션·grants가 어긋나지 않는다.
# 비공개 저장소·패키지면: 먼저 `docker login ghcr.io` (read:packages 토큰) 하고, GitHub 파일을 받을 토큰(contents:read)을
#    GITHUB_TOKEN 환경 변수나 이 폴더의 github-token 파일(chmod 600)에 둔다.
# 되돌리기는 ./deploy.sh --rollback, 상태는 ./deploy.sh --status.
set -euo pipefail

cd "$(dirname "$0")"
REPO=${AUDENIQ_REPO:-TAE-OK-11/audeniq}
IMAGE=${AUDENIQ_IMAGE_REPO:-ghcr.io/tae-ok-11/audeniq}
VARIANT=${BACKEND_CPU_VARIANT:-portable}
FILES=(compose.production.yaml bootstrap.sql grants.sql deploy.sh check-cpu.sh pg-upgrade.sh backup.sh restore-backup.sh archive.sh update.sh
       pgbouncer/Dockerfile pgbouncer/entrypoint.sh)

die() { echo "update: $*" >&2; exit 1; }

check=false force=false commit=""
for a in "$@"; do
  case "$a" in
    --check) check=true ;;
    --force) force=true ;;
    -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
    -*) die "모르는 옵션: $a" ;;
    *) [[ "$a" =~ ^[0-9a-f]{7,40}$ ]] || die "커밋은 16진수 sha여야 해요: $a"; commit=$a ;;
  esac
done

case "$VARIANT" in
  portable) suffix="" ;;
  x86-64-v3|graviton4|ampere-a4|ampere-a1) suffix="-$VARIANT" ;;
  *) die "지원하지 않는 BACKEND_CPU_VARIANT: $VARIANT" ;;
esac

token=${GITHUB_TOKEN:-}
if [ -z "$token" ] && [ -f github-token ]; then token=$(tr -d '[:space:]' < github-token); fi
gh_curl() {
  if [ -n "$token" ]; then curl -fsSL --retry 3 -H "Authorization: Bearer $token" "$@"; else curl -fsSL --retry 3 "$@"; fi
}

# 짧은 sha는 GitHub에서 전체 sha로 (이미지 태그는 전체 sha)
if [ -n "$commit" ] && [ ${#commit} -lt 40 ]; then
  commit=$(gh_curl -H 'Accept: application/vnd.github.sha' "https://api.github.com/repos/$REPO/commits/$commit") \
    || die "커밋을 찾지 못했어요: $commit (비공개 저장소면 GITHUB_TOKEN 또는 github-token 파일)"
fi
if [ -n "$commit" ]; then tag="$IMAGE:sha-$commit$suffix"; else tag="$IMAGE:main$suffix"; fi

echo "update: 이미지 받는 중 $tag"
docker pull --quiet "$tag" >/dev/null || die "이미지를 받지 못했어요: $tag (docker login ghcr.io 했는지, 그 커밋에 백엔드 이미지가 있는지 확인)"
pinned=$(docker image inspect --format '{{range .RepoDigests}}{{println .}}{{end}}' "$tag" | grep -m1 "^$IMAGE@sha256:" || true)
[ -n "$pinned" ] || die "digest를 찾지 못했어요: $tag"
rev=$(docker image inspect --format '{{index .Config.Labels "org.opencontainers.image.revision"}}' "$pinned")
[[ "$rev" =~ ^[0-9a-f]{40}$ ]] || die "이미지에 커밋 정보가 없어요: $pinned"

current=""
[ -f production.env ] && current=$(sed -n 's/^AUDENIQ_IMAGE=//p' production.env | tail -n 1)
echo "update: 지금   ${current:-(없음)}"
echo "        새 버전 $pinned (커밋 ${rev:0:7})"
if [ "$current" = "$pinned" ] && [ "$force" = false ]; then
  echo "update: 이미 최신이에요 (다시 하려면 --force)"
  exit 0
fi
if [ "$check" = true ]; then
  echo "update: 올릴 새 버전이 있어요 — ./update.sh 로 배포"
  exit 0
fi

# 배포 파일은 임시 폴더에 모두 받은 뒤에만 바꿔 넣는다 (중간에 끊기면 섞이지 않게)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/pgbouncer"
for f in "${FILES[@]}"; do
  gh_curl -o "$tmp/$f" "https://raw.githubusercontent.com/$REPO/$rev/deploy/$f" \
    || die "배포 파일을 받지 못했어요: deploy/$f @ ${rev:0:7} (비공개 저장소면 GITHUB_TOKEN 또는 github-token 파일)"
done
mkdir -p pgbouncer
for f in "${FILES[@]}"; do
  if [ -f "$f" ] && cmp -s "$tmp/$f" "$f"; then continue; fi
  echo "update: 바뀜 $f"
  install -m 644 "$tmp/$f" "$f"
done
chmod +x deploy.sh update.sh pg-upgrade.sh check-cpu.sh backup.sh restore-backup.sh archive.sh pgbouncer/entrypoint.sh

exec ./deploy.sh "$pinned"
