#!/usr/bin/env bash
# CloudFront + S3 스택 만들기/갱신: deploy/aws/deploy-stack.sh <studio|admin|survey|landing>
#   AWS_UI_CERT_ARN       us-east-1 ACM 인증서 ARN (처음 만들 때 필수, 이후 생략하면 기존 값)
#   API_ORIGIN_DOMAIN     /api 를 받을 백엔드 ALB의 HTTPS 주소 (예: api-origin.audeniq.com, 랜딩은 없음)
#   EDGE_SERVICE_SECRET   백엔드 EDGE_SERVICE_SECRET 과 같은 값 (CloudFront가 x-audeniq-service로 붙인다)
# 지정하지 않은 값은 기존 스택 값을 그대로 쓴다.
set -euo pipefail
app=${1:?사용법: deploy-stack.sh <studio|admin|survey|landing>}
here=$(cd "$(dirname "$0")" && pwd)
template=$(mktemp --suffix=.json)
trap 'rm -f "$template"' EXIT
node "$here/template.mjs" "$app" > "$template"

params=()
[ -n "${AWS_UI_CERT_ARN:-}" ] && params+=("AcmCertificateArn=$AWS_UI_CERT_ARN")
[ -n "${API_ORIGIN_DOMAIN:-}" ] && params+=("ApiOriginDomain=$API_ORIGIN_DOMAIN")
[ -n "${EDGE_SERVICE_SECRET:-}" ] && params+=("ServiceSecret=$EDGE_SERVICE_SECRET")

aws cloudformation deploy \
  --stack-name "audeniq-$app-ui" \
  --template-file "$template" \
  --no-fail-on-empty-changeset \
  ${params:+--parameter-overrides "${params[@]}"}

aws cloudformation describe-stacks --stack-name "audeniq-$app-ui" \
  --query 'Stacks[0].Outputs[].[OutputKey,OutputValue]' --output table
