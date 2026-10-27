#!/usr/bin/env bash
# CI(main)에서 화면을 S3 + CloudFront로 배포: deploy/aws/ci-publish.sh <admin|landing> (studio는 백엔드가 서빙해서 건너뜀)
# AWS_UI_ROLE_ARN 이 없으면 (아직 AWS로 옮기기 전) 건너뛴다.
#   GitHub Actions: aws-actions/configure-aws-credentials 가 자격증명을 먼저 넣어 둔다
#   CircleCI: OIDC 토큰(CIRCLE_OIDC_TOKEN_V2)으로 여기서 역할을 맡는다
set -euo pipefail
app=${1:?사용법: ci-publish.sh <app>}
if [ -z "${AWS_UI_ROLE_ARN:-}" ]; then
  echo "AWS_UI_ROLE_ARN 미설정 — $app 화면의 S3/CloudFront 배포를 건너뜀"
  exit 0
fi
export AWS_REGION=${AWS_REGION:-${AWS_UI_REGION:-ap-northeast-2}}

if ! command -v aws >/dev/null; then
  # 버전과 SHA-256을 고정한다 (올릴 때: 같은 이름의 .sig를 AWS CLI 서명 키
  # FB5D B77F D5C1 18B8 0511 ADA8 A631 0ACC 4672 475C로 확인한 뒤 해시를 바꾼다)
  v=2.37.9
  case "$(uname -m)" in
    x86_64) sum=6b3a6a3d7bb3997928f0bdf7b866914224abf242c2e54e1dcebbe84bee64f356 ;;
    aarch64) sum=e7d2cca3622af4765871fe9ce7eeed03acd09dca10a99ebc46e92f79d78765c0 ;;
    *) echo "지원하지 않는 아키텍처: $(uname -m)" >&2; exit 1 ;;
  esac
  tmp=$(mktemp -d)
  curl -fsSL "https://awscli.amazonaws.com/awscli-exe-linux-$(uname -m)-$v.zip" -o "$tmp/awscli.zip"
  echo "$sum  $tmp/awscli.zip" | sha256sum -c --quiet
  unzip -q "$tmp/awscli.zip" -d "$tmp"
  sudo "$tmp/aws/install" >/dev/null
fi

if [ -z "${AWS_ACCESS_KEY_ID:-}" ] && [ -n "${CIRCLE_OIDC_TOKEN_V2:-}" ]; then
  read -r AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY AWS_SESSION_TOKEN < <(
    aws sts assume-role-with-web-identity --role-arn "$AWS_UI_ROLE_ARN" \
      --role-session-name "circleci-${CIRCLE_BUILD_NUM:-local}" \
      --web-identity-token "$CIRCLE_OIDC_TOKEN_V2" --duration-seconds 900 \
      --query 'Credentials.[AccessKeyId,SecretAccessKey,SessionToken]' --output text)
  export AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY AWS_SESSION_TOKEN
fi

node --experimental-strip-types "$(cd "$(dirname "$0")" && pwd)/publish.ts" "$app"
