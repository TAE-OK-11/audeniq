# 검색 운영과 측정

2026-10-08 검토. 이번 작업은 저장소 커밋까지이며 실서비스 배포·색인 제출·계정 설정은 실행하지 않았다.

## 적용한 스킬과 최신 기준

[fire-your-seo-agency](https://github.com/leopard627/fire-your-seo-agency/blob/main/SKILL.md)의 SEO, AEO, GEO, LLMO, Naver NEO 감사·구현·측정 접근을 적용했다. 확인되지 않은 통계나 사례를 만들지 않고 질문에 먼저 답하는 공식 서비스 안내를 작성했다.

- [Google AI 검색 최적화](https://developers.google.com/search/docs/fundamentals/ai-optimization-guide): 수집·색인·유용한 콘텐츠 원칙을 적용한다. llms.txt를 Google AI 노출의 필수 조건으로 설명하지 않는다.
- [Google 검색 변경 기록](https://developers.google.com/search/updates): 2026년 5월 FAQ rich results 지원 종료를 반영한다. FAQ 스키마는 본문과 일치하는 의미 정보로 사용하며 특수 검색 표시를 약속하지 않는다.
- [OpenAI 봇](https://developers.openai.com/api/docs/bots): 검색용 OAI-SearchBot과 학습용 GPTBot을 구분한다. 사용자 요청형 조회는 robots만으로 완전히 통제할 수 없다.
- [Anthropic 봇](https://privacy.claude.com/en/articles/10007498-how-does-anthropic-crawl-data-from-the-web-and-how-can-site-owners-block-the-crawler): 학습용 ClaudeBot과 조회용 Claude-SearchBot·Claude-User를 구분한다.
- [Naver robots](https://searchadvisor.naver.com/guide/seo-basic-robots)·[IndexNow](https://searchadvisor.naver.com/guide/indexnow-about): Yeti에 공개 HTML과 발견 경로를 제공한다.
- [IndexNow 프로토콜](https://www.indexnow.org/documentation): 공개 키로 호스트를 확인하고 실제 게시된 URL만 알린다. 접수는 색인 확정을 뜻하지 않는다.

## 배포 후 수동 절차

1. 별도로 배포를 결정한 뒤 npm run check, 배포, npm run verify:live 순으로 실제 도메인 응답을 확인한다.
2. Google Search Console·Bing Webmaster Tools·Naver Search Advisor 소유권 인증을 확인한다. 계정에서 받은 실제 토큰만 DNS 또는 HTML에 추가한다.
3. 각 계정에 https://audeniq.com/sitemap.xml을 제출한다. 네이버에 RSS 제출 기능이 제공되면 https://audeniq.com/feed.xml도 제출한다.
4. 대표 URL의 검사·수집 상태를 확인하고 필요하면 색인 요청을 보낸다. npm run indexnow는 배포 버전·공개 키·6개 URL을 검증한 뒤 공유 API로 갱신 알림을 보낸다. 빌드·테스트·커밋에서는 실행하지 않는다.
5. 실제 배포일과 14일 뒤에 같은 기간·지역·기기·쿼리 필터로 측정한다. 이번에는 미배포이므로 재측정일이 아직 확정되지 않았다.

## 측정 기록

measurement.csv에는 확인한 값만 적는다. 접근할 수 없는 지표는 공란으로 남기며 0으로 추정하지 않는다.

| 영역 | 확인할 항목 | 출처 |
| --- | --- | --- |
| SEO | 색인 페이지, 노출, 클릭, CTR, 쿼리별 순위 | 검색 콘솔 |
| Naver | 수집·색인, sitemap·RSS, 유입 검색어 | Search Advisor·연결된 분석 도구 |
| AEO/GEO | 같은 질문별 언급, 실제 인용 URL, 답변 정확성 | AI 검색의 실제 답변 기록 |
| LLMO | 봇 상태 코드와 robots 정책 | Cloudflare 로그·실제 응답 |
| 전환 | 설문 링크 클릭과 완료를 구분 | 동의·설정이 확인된 분석 도구·Google Forms |

추적 도구는 임의 설치하지 않았다. AI 검색 질문·시각·지역·답변·인용 URL을 함께 기록하고 인용 없는 브랜드 언급을 인용으로 세지 않는다. 기준 질문은 AUDENIQ의 서비스, Publishing 대상, 배급과 퍼블리싱의 차이, 현재 발매 신청 가능 여부다.

## 후속 콘텐츠

실제 이용자 질문과 확정된 운영 정보를 근거로 발매 자료 체크리스트, 배급사 이전 확인 항목, 작품 정보 정리, 레이블의 팀 업무 안내를 확장할 수 있다. 서비스 담당자가 사실·권리·계약 범위를 검토한 뒤 게시한다. 검색어만 바꾼 중복 글이나 검토되지 않은 자동 발행은 하지 않는다. 공통 원본을 수정해 홈페이지·FAQ·서비스 페이지·AI 텍스트를 함께 갱신한다.
