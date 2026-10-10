#!/usr/bin/env bash
# CI(main)에서 화면을 S3 + CloudFront로 배포: deploy/aws/ci-publish.sh <studio|admin|survey|landing>
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
  tmp=$(mktemp -d)
  curl -fsSL "https://awscli.amazonaws.com/awscli-exe-linux-$(uname -m).zip" -o "$tmp/awscli.zip"
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

node "$(cd "$(dirname "$0")" && pwd)/publish.mjs" "$app"
