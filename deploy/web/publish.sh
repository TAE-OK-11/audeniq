#!/usr/bin/env bash
# 빌드된 화면을 서버의 web(nginx)로 올린다: deploy/web/publish.sh <studio|admin|survey|landing>...
#   DEPLOY_HOST, DEPLOY_USER (필수), DEPLOY_PATH (기본 /opt/audeniq), SSH_KEY (기본 ~/.ssh/deploy_key)
#
# - 파일은 <DEPLOY_PATH>/web/site/<app>/ 에 덮어쓴다. 지난 배포의 해시 에셋은 지우지 않는다
#   (열려 있던 화면이 예전 청크를 계속 받게). HTML은 마지막에 올려 없는 에셋을 가리키지 않게 한다.
# - .br 사이드카·_headers·_redirects 는 올리지 않는다 (nginx·Cloudflare가 압축, 헤더·리다이렉트는 nginx 설정)
# - nginx 설정(deploy/web/nginx.mjs)이 바뀌었으면 올리고 web만 다시 띄운다 (./deploy.sh --web)
set -euo pipefail
[ $# -gt 0 ] || { sed -n '2,8p' "$0"; exit 2; }
: "${DEPLOY_HOST:?DEPLOY_HOST}" "${DEPLOY_USER:?DEPLOY_USER}"
path=${DEPLOY_PATH:-/opt/audeniq}
key=${SSH_KEY:-$HOME/.ssh/deploy_key}
root=$(cd "$(dirname "$0")/../.." && pwd)
ssh_=(ssh -i "$key" "$DEPLOY_USER@$DEPLOY_HOST")

dir_of() {
  case "$1" in
    studio) echo web/studio/public ;;
    admin) echo web/admin/dist ;;
    survey) echo web/survey/public ;;
    landing) echo web/landing/public ;;
    *) echo "알 수 없는 앱: $1" >&2; exit 2 ;;
  esac
}

for app in "$@"; do
  src="$root/$(dir_of "$app")"
  [ -f "$src/index.html" ] || { echo "$src/index.html 이 없어요. 먼저 빌드하세요." >&2; exit 1; }
  dest="$path/web/site/$app"
  skip=(--exclude=_headers --exclude=_redirects --exclude='*.br' --exclude=.DS_Store)
  # 1) HTML이 아닌 파일 2) HTML
  tar -C "$src" "${skip[@]}" --exclude='*.html' -czf - . | "${ssh_[@]}" "mkdir -p '$dest' && tar -xzf - -C '$dest'"
  (cd "$src" && find . -name '*.html' -print0 | tar --null -T - -czf -) | "${ssh_[@]}" "tar -xzf - -C '$dest'"
  echo "올림: $app → $DEPLOY_HOST:$dest"
done

conf=$(mktemp)
trap 'rm -f "$conf"' EXIT
node "$root/deploy/web/nginx.mjs" > "$conf"
"${ssh_[@]}" "mkdir -p '$path/web/nginx' && cat > '$path/web/nginx/audeniq.conf.template.new'" < "$conf"
"${ssh_[@]}" "cd '$path' && if cmp -s web/nginx/audeniq.conf.template.new web/nginx/audeniq.conf.template; then rm web/nginx/audeniq.conf.template.new; echo 'nginx 설정 그대로'; else mv web/nginx/audeniq.conf.template.new web/nginx/audeniq.conf.template && ./deploy.sh --web && echo 'nginx 설정 바뀜 → web 다시 띄움'; fi"
