# AUDENIQ Studio CloudFront 설정 기록

2026-10-27 라이브 컷오버 기준, CloudFront 측 설정을 코드와 함께 남긴다.
백엔드 재개발 시 이 설정 전체를 검토할 것. main에 병합하지 않고 브랜치로만 둔다.

## 분배

- ID: E29B1S5744RYI0
- 도메인: d3m674ye8gesq5.cloudfront.net
- 이름: AUDENIQ Studio CloudFront trial
- 상태: Enabled, PriceClass_100
- 대체 도메인: studio.audeniq.com
- 인증서: *.audeniq.com 와일드카드 (ACM, TLSv1.2_2021)
  - studio 전용 인증서가 아니라 와일드카드가 연결되어 있다.
  - studio.audeniq.com은 와일드카드에 포함되어 정상 동작한다.

## 오리진 (1개)

- 이름: api-origin.audeniq.com-mvqm79ra3ic
- 도메인: api-origin.audeniq.com (Custom origin)
- 프로토콜: HTTPS-only, 443, 최소 TLSv1.2, 경로 없음
- 커스텀 헤더: x-audeniq-site, x-audeniq-service (값은 시크릿, 여기에 적지 않음)

## 동작 (2개)

- Default(*): 캐시 정책 Managed-CachingOptimized, 오리진 요청 정책 없음
- /api/*: 캐시 정책 Managed-CachingDisabled,
  오리진 요청 정책 Managed-AllViewerExceptHostHeader
- 둘 다: 뷰어 HTTP→HTTPS 리디렉트, 동일 오리진, 응답 헤더 정책 없음

## CloudFront Function: audeniq-studio-edge

- 런타임: cloudfront-js-2.0, LIVE 스테이지로 게시됨
- 연결: Default(*) + /api/* 양쪽 viewer-request
- 코드: 이 디렉터리의 `studio-edge-function.js` (배포된 코드와 동일)
- 하는 일 (2가지뿐):
  1. 브라우저가 보낸 x-audeniq-* 헤더 전부 제거 (위조 방지)
  2. 실제 시청자 IP를 x-audeniq-client-ip로 주입
- x-audeniq-service / x-audeniq-site는 오리진 커스텀 헤더가 붙이므로 함수에서 다루지 않는다.
- 나머지(SPA 폴백·리다이렉트·캐시·보안 헤더·well-known)는 백엔드(studio_static.rs)가 한다.

## 응답 헤더 정책 (CORS)

- audeniq-studio-api-cors: 존재하나 미연결 상태로 유지
- 결정(2026-10-27): Business 플랜 업그레이드 안 함, /api 동작에 응답 헤더 정책 연결 안 함
- 이유: Studio는 same-origin(SPA도 API도 studio.audeniq.com)이라 CORS가 필요 없음.
  Free 플랜에서는 커스텀 응답 헤더 정책 연결이 막혀 있다(Business부터).
  나중에 크로스 오리진이 필요해지면 백엔드에서 직접 헤더를 붙이는 것이 가장 싸다.
- 참고: 관리형 CORS 정책은 Allow-Origin이 `*`라 쿠키 인증 요청에는 쓸 수 없다.

## DNS (2026-10-27 21:55 공용 DNS 기준)

- studio.audeniq.com: CNAME → d3m674ye8gesq5.cloudfront.net (DNS-only, CloudFront 직접)
- api-origin.audeniq.com: Cloudflare 프록시 (터널 경유)

## 현재 상태 및 남은 것 (2026-10-27 21:53 결정: 현 상태 유지)

- 사이트는 CloudFront + 터널 오리진 경유로 동작 중.
- /api는 CloudFront 경유 시 Cloudflare 확인 페이지가 나올 수 있다(curl 기준, 미해결).
  해결 경로: Cloudflare api-origin 존에 CloudFront IP 허용·우회 규칙,
  또는 백엔드 TLS 완성 + DNS-only 직접 연결.
- 백엔드는 사용자가 나중에 새로 개발한다.
- 미결정: failover 방식(Premium 전용이라 Free에서는 불가),
  로깅(S3 버킷 필요), WAF(별도 비용).

## 관련 문서

- `deploy/STUDIO_SERVING.md`: 백엔드 정적 서빙 전환 기록
- `deploy/studio-release.sh`, `deploy/studio-deploy.yml`: Studio 배포 관련
