#!/usr/bin/env bash
# 백엔드 HTTPS 인증서 (crates/core/src/edge_tls.rs, deploy/aws/README.md): CloudFront가 오리진으로 부르는 주소의 인증서.
# CloudFront는 공인 CA 인증서만 믿어서 자체 서명·Cloudflare Origin CA는 안 된다. 기본은 Let's Encrypt를
# DNS-01(Cloudflare DNS API)로 받아 서버 포트 80을 열지 않는다. api는 ./tls 파일을 5분마다 다시 읽어 재시작 없이 바꾼다.
#
#   ./tls-cert.sh issue origin.audeniq.com 메일주소   처음 발급 (tls-cloudflare.ini 필요, 아래)
#   ./tls-cert.sh renew                               갱신(만료 30일 전부터만 실제로 바꾼다) — cron 매일
#   ./tls-cert.sh install fullchain.pem privkey.pem   다른 곳에서 받은 인증서 (예: ACM 내보내기 인증서)
#   ./tls-cert.sh status                              발급된 인증서·만료일
#
# tls-cloudflare.ini (chmod 600): Cloudflare API 토큰, 권한은 해당 영역(Zone)의 DNS:Edit 하나
#   dns_cloudflare_api_token = <토큰>
# cron 예: 17 4 * * * cd /opt/audeniq && ./tls-cert.sh renew >> tls-cert.log 2>&1
set -euo pipefail
cd "$(dirname "$0")"
# certbot/dns-cloudflare v5.8.0 (amd64·arm64 공용 인덱스). 올릴 때 태그와 digest를 함께 바꾼다
IMAGE=${CERTBOT_IMAGE:-certbot/dns-cloudflare:v5.8.0@sha256:c45edb002b883da1a1235abb205dff474a7a1a459d878e8d5fdc7f9d83073aea}
NAME=audeniq-origin
# api 컨테이너 사용자 (deploy/Dockerfile USER 10001)
API_UID=10001

die() { echo "tls-cert: $*" >&2; exit 1; }
mkdir -p letsencrypt tls

# 받은 PEM 두 개를 /tls(= ./tls)에 api 사용자만 읽을 수 있게 놓는다. 이름을 바꿔 넣어 반쯤 쓴 파일을 읽지 않게 한다.
# certbot이 새 인증서를 받았을 때만(발급·실제 갱신) 컨테이너 안에서 돈다: --deploy-hook
copy_script() { # <fullchain> <privkey>
  echo "set -e; grep -q 'BEGIN CERTIFICATE' \"$1\"; grep -q 'PRIVATE KEY' \"$2\";" \
    "install -o $API_UID -g $API_UID -m 0400 \"$1\" /tls/.fullchain.pem.new;" \
    "install -o $API_UID -g $API_UID -m 0400 \"$2\" /tls/.privkey.pem.new;" \
    "mv -f /tls/.privkey.pem.new /tls/privkey.pem; mv -f /tls/.fullchain.pem.new /tls/fullchain.pem;" \
    "echo 'tls-cert: ./tls 인증서 바꿈 — api가 5분 안에 새 인증서를 써요 (로그: TLS certificate reloaded)'"
}
# shellcheck disable=SC2016 # certbot이 hook을 실행할 때 $RENEWED_LINEAGE를 채운다
HOOK=$(copy_script '$RENEWED_LINEAGE/fullchain.pem' '$RENEWED_LINEAGE/privkey.pem')

certbot() {
  docker run --rm \
    -v "$PWD/letsencrypt:/etc/letsencrypt" \
    -v "$PWD/tls-cloudflare.ini:/run/cloudflare.ini:ro" \
    -v "$PWD/tls:/tls" \
    "$IMAGE" "$@"
}

# 이미 받은 인증서를 다시 놓는다 (./tls가 비었을 때, 다른 곳에서 받은 인증서)
place() { # <docker -v 값> <fullchain> <privkey>
  docker run --rm --entrypoint sh -v "$1" -v "$PWD/tls:/tls" "$IMAGE" -c "$(copy_script "$2" "$3")"
}

need_credentials() {
  [ -f tls-cloudflare.ini ] || die "tls-cloudflare.ini 가 없어요 (이 파일 맨 위 설명)"
  [ "$(stat -c %a tls-cloudflare.ini)" = 600 ] || die "tls-cloudflare.ini 는 chmod 600 이어야 해요"
}

case "${1:-}" in
  issue)
    domain=${2:?사용법: tls-cert.sh issue <도메인> <메일>}
    email=${3:?사용법: tls-cert.sh issue <도메인> <메일>}
    need_credentials
    certbot certonly --non-interactive --agree-tos -m "$email" \
      --dns-cloudflare --dns-cloudflare-credentials /run/cloudflare.ini --dns-cloudflare-propagation-seconds 30 \
      --key-type ecdsa --cert-name "$NAME" -d "$domain" --deploy-hook "$HOOK"
    ;;
  renew)
    need_credentials
    # 실제로 갱신했을 때만 deploy-hook이 ./tls 를 바꾼다 (이전 발급 때의 hook은 renewal 설정에 남지만 여기서 다시 넣는다)
    certbot renew --non-interactive --cert-name "$NAME" --deploy-hook "$HOOK"
    if [ ! -s tls/fullchain.pem ]; then
      place "$PWD/letsencrypt:/etc/letsencrypt:ro" "/etc/letsencrypt/live/$NAME/fullchain.pem" "/etc/letsencrypt/live/$NAME/privkey.pem"
    fi
    ;;
  install)
    chain=${2:?사용법: tls-cert.sh install <fullchain.pem> <privkey.pem>}
    key=${3:?사용법: tls-cert.sh install <fullchain.pem> <privkey.pem>}
    [ -f "$chain" ] && [ -f "$key" ] || die "파일이 없어요: $chain $key"
    dir=$(mktemp -d)
    trap 'rm -rf "$dir"' EXIT
    install -m 0600 "$chain" "$dir/fullchain.pem"
    install -m 0600 "$key" "$dir/privkey.pem"
    place "$dir:/in:ro" /in/fullchain.pem /in/privkey.pem
    ;;
  status)
    if [ -d "letsencrypt/live/$NAME" ]; then
      docker run --rm -v "$PWD/letsencrypt:/etc/letsencrypt:ro" "$IMAGE" certificates --cert-name "$NAME"
    fi
    ls -l tls/
    ;;
  *)
    sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
    ;;
esac
