#!/usr/bin/env bash
# AUDENIQ Studio 화면 배포 — 빌드 결과(web/studio/public)를 이 서버에 올린다. api는 재시작하지 않는다.
#
#   ./studio-release.sh install <커밋> <studio.tar.gz>   # releases/<커밋>에 풀고 current를 바꾼다
#   ./studio-release.sh rollback                          # 직전 릴리스로 되돌리기
#   ./studio-release.sh status                            # 지금 릴리스와 목록
#
# api 컨테이너는 ./studio를 /srv/studio로 읽기 전용 마운트하고 STUDIO_DIR=/srv/studio/current를 서빙한다
# (compose.production.yaml, crates/core/src/studio_static.rs). current는 상대 심링크라 컨테이너 안에서도 맞다.
# 심링크를 바꾸면 다음 요청부터 새 화면이다. 새 릴리스를 읽지 못하면 api는 이전 화면을 계속 준다.
# GitHub Actions(Studio deploy)가 main 병합마다 실행한다. 자세한 내용: deploy/STUDIO_SERVING.md
set -euo pipefail

cd "$(dirname "$0")"
DIR=studio
KEEP=${STUDIO_KEEP_RELEASES:-5}
HISTORY=$DIR/release-history.log

die() { echo "studio-release: $*" >&2; exit 1; }
current() { readlink "$DIR/current" 2>/dev/null | sed 's|^releases/||'; }

# 심링크를 한 번에 바꾼다 (rename은 원자적: 요청 중간에 빈 경로가 보이지 않는다)
switch_to() {
  local rel=$1
  [ -f "$DIR/releases/$rel/index.html" ] || die "릴리스가 없거나 index.html이 없어요: $rel"
  ln -sfn "releases/$rel" "$DIR/current.tmp"
  mv -Tf "$DIR/current.tmp" "$DIR/current"
  echo "$(date -u +%FT%TZ) $rel" >> "$HISTORY"
  echo "studio-release: current → $rel"
}

# 화면이 가리키는 /assets/* 파일이 모두 있는지 (반쯤 풀린 압축 등)
check_release() {
  local root=$1 ref missing=0
  [ -f "$root/index.html" ] || die "index.html이 없어요"
  while IFS= read -r ref; do
    if [ ! -f "$root/$ref" ]; then echo "studio-release: 없음 /$ref" >&2; missing=1; fi
  done < <(grep -oE '/assets/[A-Za-z0-9._-]+' "$root/index.html" | sed 's|^/||' | sort -u)
  [ "$missing" = 0 ] || die "index.html이 가리키는 파일이 없어요"
}

prune() {
  local cur prev rel n=0
  cur=$(current)
  prev=$(awk -v cur="$cur" '$2 != cur { p = $2 } END { print p }' "$HISTORY" 2>/dev/null || true)
  # 최근 것부터 KEEP개는 남기고, 지금·직전 릴리스는 늘 남긴다
  while IFS= read -r rel; do
    n=$((n + 1))
    [ "$n" -le "$KEEP" ] && continue
    [ "$rel" = "$cur" ] || [ "$rel" = "$prev" ] && continue
    rm -rf "${DIR:?}/releases/${rel:?}"
    echo "studio-release: 지움 $rel"
  done < <(ls -1t "$DIR/releases")
}

mkdir -p "$DIR/releases"
case "${1:-}" in
  install)
    rel=${2:-}; archive=${3:-}
    [[ "$rel" =~ ^[0-9a-f]{7,40}$ ]] || die "커밋은 16진수 sha여야 해요: $rel"
    [ -f "$archive" ] || die "압축 파일이 없어요: $archive"
    if [ "$(current)" = "$rel" ]; then echo "studio-release: 이미 $rel 이에요"; exit 0; fi
    tmp="$DIR/releases/.$rel.tmp"
    rm -rf "$tmp"
    mkdir -p "$tmp"
    trap 'rm -rf "$tmp"' EXIT
    # 압축 안에 절대 경로·..·링크가 있으면 거절한다
    if tar -tvzf "$archive" | cut -c1 | grep -q '[^-d]'; then die "압축에 링크나 특수 파일이 있어요"; fi
    if tar -tzf "$archive" | grep -Eq '^/|(^|/)\.\.(/|$)'; then die "압축에 바깥 경로가 있어요"; fi
    tar -xzf "$archive" -C "$tmp" --no-same-owner --no-same-permissions
    chmod -R u=rwX,go=rX "$tmp"
    check_release "$tmp"
    rm -rf "${DIR:?}/releases/${rel:?}"
    mv "$tmp" "$DIR/releases/$rel"
    trap - EXIT
    switch_to "$rel"
    prune
    ;;
  rollback)
    [ -s "$HISTORY" ] || die "되돌릴 기록이 없어요 ($HISTORY)"
    cur=$(current)
    prev=$(awk -v cur="$cur" '$2 != cur { p = $2 } END { print p }' "$HISTORY")
    [ -n "$prev" ] || die "지금과 다른 이전 릴리스가 기록에 없어요"
    switch_to "$prev"
    ;;
  status)
    echo "current: $(current || true)"
    ls -1t "$DIR/releases"
    ;;
  *)
    sed -n '2,11p' "$0"; exit 2
    ;;
esac
