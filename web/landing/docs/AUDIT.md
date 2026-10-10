# AUDENIQ 발견 가능성 감사

관찰일 2026-10-08. 대상 https://audeniq.com/ 및 main의 변경 전 커밋 6d1fe11b7058b093e9d69c904dce4c3b26b9c429. 공개 HTTP 응답으로 관찰했다. 구현 완료는 코드·로컬 검증 기준이며 배포 완료를 의미하지 않는다.

## 실서비스 기준선

| 항목 | 관찰 결과 |
| --- | --- |
| 홈페이지 | HTTP 200, 59,627 bytes, 이전 LIMBO 설명 노출 |
| robots | HTTP 200, 일반 검색 허용, STUDIO·health 제외 |
| sitemap | HTTP 200, 홈페이지 1개 URL, lastmod 없음 |
| llms.txt·llms-full.txt·feed.xml | HTTP 404 |
| 없는 페이지 | HTTP 404 |
| 홈페이지 검색 헤더 | noindex 없음, apex canonical |
| 노출·클릭·CTR·순위·AI 인용 | 계정·측정 자료 없음: 미확인 |

## 우선순위와 구현

| 우선순위 | 문제·영역 | 코드 결과 |
| --- | --- | --- |
| P0 | 최신 사업 설명·출시 상태 | 현재 사업 방향 유지, 모든 신규 문서에 준비 상태 표시 |
| P0 | 페이지 식별 | 고유 메타·canonical·H1, 실제 404, 별칭 정규화 |
| P1 | AEO 답변 부족 | 배급·퍼블리싱·레이블·소개·FAQ 공식 문서 5개와 홈페이지 연결 |
| P1 | 본문·스키마·AI 자료 불일치 | 공통 원본에서 HTML·FAQ/Service 스키마·AI 참고 텍스트 생성 |
| P1 | GEO 브랜드 관계 | 일관된 Organization·WebSite ID, provider·about·breadcrumb |
| P1 | LLMO 수집 정책 | 검색·조회 봇 허용, 지정 학습 봇 차단, 비공개 경로 제외 |
| P1 | Naver·검색 발견 경로 | Yeti 정책, 6개 URL sitemap·lastmod, 공식 안내 RSS |
| P2 | 변경 알림·배포 검증 | 수동 IndexNow·실제 배포 버전 검증 스크립트 준비 |
| P2 | 성과 자료 없음 | 미확인 수치가 빈 측정 템플릿·배포 14일 뒤 비교 절차 |

수수료율·가격, 허구 리뷰·성과 수치, 확정되지 않은 DSP 계약은 공개 콘텐츠와 스키마에 넣지 않았다. 검색엔진 인증 코드는 실제 발급 자료가 없어 추가하지 않았다. 최신 Google FAQ 특수 표시 종료를 반영했고 llms.txt를 Google AI 노출의 필수 조건으로 설명하지 않는다.

## 검증과 실행 상태

Node 테스트로 6개 페이지의 Worker 라우팅·canonical·본문/FAQ 스키마 일치·링크·미리보기 제외·404·봇 정책·금액/요율 제외를 확인한다. XML은 별도 파서로 sitemap·RSS 문법과 URL 개수를 확인한다. 최초 감사 때 브라우저 화면은 미측정이었으나, 2026-10-08 디자인 수정에서는 Chromium으로 안내 5개 페이지를 1440·900·390·320px에서 확인했다. 홈페이지의 실제 로고·글꼴 스택·브랜드 색상 일치, 가로 넘침 없음, 모바일 메뉴·FAQ 동작과 자산 오류 없음을 총 20개 화면 조건에서 검증했다. Lighthouse·실사용 Core Web Vitals는 미측정이다.

최종 요청 범위는 검증 후 GitHub 커밋만이다. Workers 배포·IndexNow 호출·검색 콘솔 제출은 실행하지 않는다. 실서비스의 이전 설명은 향후 배포해야 바뀐다.
