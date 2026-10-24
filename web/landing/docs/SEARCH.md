# 검색 운영과 측정

2026-10-10 갱신. 이번 작업은 저장소 커밋까지이며 실서비스 배포·색인 제출·계정 설정은 실행하지 않았다.

## 적용한 스킬과 공식 기준

[fire-your-seo-agency](https://github.com/leopard627/fire-your-seo-agency/blob/main/SKILL.md)의 SEO, AEO, GEO, LLMO, Naver NEO 감사·구현·측정 절차를 적용했다. 공식 지침과 충돌하면 공식 지침을 우선한다.

- [Google 다국어·다지역 사이트](https://developers.google.com/search/docs/specialty/international/localized-versions): 언어별 고유 URL, 양방향 hreflang, x-default, 자동 리디렉션 금지.
- [Google AI 기능과 웹사이트](https://developers.google.com/search/docs/appearance/ai-features): AI Overviews·AI Mode에는 일반 Search 색인 요건 외 별도 파일이 필요하지 않다. llms.txt는 보조 자료다.
- [Google 검색 변경 기록](https://developers.google.com/search/updates): FAQ rich results는 표시를 전제로 하지 않는다. FAQPage는 본문과 같은 의미 정보로만 쓴다.
- [Bing Webmaster 지침](https://www.bing.com/webmasters/help/webmasters-guidelines-30fba23a)·[IndexNow](https://www.indexnow.org/documentation): 사이트맵 + 변경 URL 알림. 접수는 색인 확정이 아니다.
- [Naver 검색 robots](https://searchadvisor.naver.com/guide/seo-basic-robots)·[IndexNow](https://searchadvisor.naver.com/guide/indexnow-about).
- [OpenAI 봇](https://platform.openai.com/docs/bots), [Anthropic 크롤러](https://privacy.claude.com/en/articles/8896518-does-anthropic-crawl-data-from-the-web-and-how-can-site-owners-block-the-crawler), [Perplexity 봇](https://docs.perplexity.ai/guides/bots), [Google 크롤러 목록](https://developers.google.com/search/docs/crawling-indexing/google-common-crawlers), [Applebot](https://support.apple.com/en-us/119829).

## 크롤러 정책 (현재 유지)

robots.txt는 하나의 `User-agent: *` 그룹으로 모든 봇을 허용하고 /studio·/admin·/api·/login·/health만 제외한다. 이 정책은 소유자 결정(커밋 b1ba5c7)이며 이번 작업에서 넓히거나 좁히지 않았다.

| 용도 | 대표 User-agent / 토큰 | 이 사이트 |
| --- | --- | --- |
| 검색 색인 | Googlebot, Bingbot, Yeti(네이버), YandexBot, Baiduspider, Applebot | 허용 |
| AI 검색 색인·답변 | OAI-SearchBot(ChatGPT 검색), Claude-SearchBot, PerplexityBot. Copilot·ChatGPT 검색은 Bing 색인도 사용, Google AI Overviews/AI Mode·Gemini 검색 근거는 Googlebot 색인 사용 | 허용 |
| 사용자 요청 조회 | ChatGPT-User, Claude-User, Perplexity-User | 허용(사용자 요청형은 robots를 따르지 않을 수 있음) |
| 모델 학습 | GPTBot, ClaudeBot, CCBot, Google-Extended(토큰), Applebot-Extended(토큰) | 허용(소유자 결정) |

학습 수집 허용은 검색 노출과 별개다. 학습만 막으려면 위 학습용 토큰만 Disallow하는 그룹을 추가하면 되며, 검색·조회 봇은 그대로 둔다.

robots 외 차단 요소: 실서비스는 Cloudflare가 응답에 챌린지 스크립트를 주입한다. 크롤러 UA로 요청했을 때 모두 200이었지만 UA 위조 요청은 실제 검증 봇과 다르게 처리될 수 있다. Cloudflare 대시보드의 **Security → Bots(Bot Fight Mode)**, **AI Crawl Control / Block AI bots**, WAF 규칙이 검색·AI 검색 봇을 막지 않는지 소유자가 확인해야 한다.

## 배포 후 수동 절차 (계정 필요)

1. 배포를 별도로 결정한 뒤 `npm run check` → 배포 → `npm run verify:live`(24개 URL의 lang·hreflang·canonical·404·리디렉션 확인).
2. **www**: Cloudflare DNS에 `www.audeniq.com` 레코드(프록시)를 추가해야 apex로 308이 동작한다. 현재 DNS 레코드가 없어 접속이 안 된다.
3. **Google Search Console**: DNS TXT 소유권 인증이 이미 있다(google-site-verification). Domain 속성에서 `https://audeniq.com/sitemap.xml` 제출 → 페이지 색인 보고서에서 언어별 URL 확인 → 대표 URL(/, /en/, /ja/ 등) URL 검사 후 색인 요청. 국제 타기팅 보고서는 종료됐으므로 hreflang 오류는 URL 검사·색인 보고서로 본다.
4. **Bing Webmaster Tools**: Search Console에서 가져오기(Import)로 소유권·사이트맵 등록 → IndexNow 보고서와 URL 검사로 수신 확인. Copilot·ChatGPT 검색 노출의 기반이다.
5. **Naver Search Advisor**: 사이트 등록·소유 확인(HTML 메타 또는 파일, 발급된 실제 값만 사용) → 사이트맵·RSS(`/feed.xml`) 제출 → 웹페이지 수집 요청.
6. **Yandex Webmaster / Baidu 站长平台**: 필요하면 계정에서 등록. 토큰은 발급된 값만 추가한다.
7. **IndexNow**: Actions → *IndexNow — submit AUDENIQ public URLs* → `since`에 직전 배포 커밋을 넣으면 추가·변경·삭제된 URL만 제출, 비우면 전체 공개 URL 제출. CLI: `npm run indexnow -- --since <ref>` (`--dry-run`으로 목록만 확인). Google은 IndexNow를 사용하지 않으므로 sitemap lastmod와 Search Console로 관리한다.

## 측정

`measurement.csv`에는 확인한 값만 적는다. 접근할 수 없는 지표는 공란이며 0으로 추정하지 않는다. 기준선은 2026-10-10 행이고, 배포일과 배포 14일 뒤(집계 지연을 고려해 최근 2–3일 제외)에 같은 기간·지역·기기·쿼리 필터로 다시 잰다.

| 지표 | 측정 방법 | 권한 |
| --- | --- | --- |
| 언어별 색인 페이지 수 | Search Console 페이지 색인 보고서를 URL 접두어(/en/, /ja/ …)로 필터, Bing·Naver 색인 현황 | 계정 필요 |
| 언어별 노출·클릭·CTR | Search Console 실적 → 페이지 필터(접두어) + 국가 | 계정 필요 |
| 브랜드 vs 일반 검색 | 쿼리 필터 정규식 `(?i)audeniq|어데닉`(브랜드) / 그 외(일반) | 계정 필요 |
| 크롤링 오류 | Search Console 크롤링 통계·색인 오류, Bing 사이트 검사, Cloudflare 로그의 4xx/5xx | 계정 필요 |
| AI 크롤러 방문 | Cloudflare AI Crawl Control 또는 로그에서 OAI-SearchBot·Claude-SearchBot·PerplexityBot·GPTBot·ClaudeBot 요청 수 | Cloudflare 접근 |
| AI 검색 유입 | 분석 도구의 리퍼러(chatgpt.com, perplexity.ai, copilot.microsoft.com, gemini.google.com, claude.ai) — 분석 도구 미설치, 설치 시 동의·개인정보 처리방침 필요 | 도구 결정 필요 |
| AI 답변 인용 | 아래 질문을 ChatGPT(검색)·Perplexity·Copilot·Gemini·Google AI Overviews·Naver AI 브리핑에 입력해 날짜·지역·답변·인용 URL을 기록. 인용 없는 언급은 인용으로 세지 않음 | 수동, 누구나 |
| 기술 SEO 회귀 | `npm run check`(CI), 배포 후 `npm run verify:live` | 저장소 |
| 랩 성능 | Lighthouse 모바일(랩)과 실사용 CWV(Search Console CWV·CrUX, 75번째 백분위)를 구분해 기록 | 실사용은 트래픽 필요 |

AI 인용 기준 질문(언어별로 같은 의미로 입력): "AUDENIQ이 뭐야/What is AUDENIQ", "어데닉은 음원 배급사야/Is AUDENIQ a music distributor", "AUDENIQ Publishing은 뭐야", "지금 AUDENIQ으로 음원 발매할 수 있어/Is AUDENIQ accepting releases", "음원 배급과 음악 퍼블리싱 차이". 기대 답: 출시 준비 중, 아직 신청 불가, 배급·퍼블리싱 별도, 지원 플랫폼 미확정. 틀린 답(이미 운영 중, 특정 DSP 지원 등)도 기록해 콘텐츠 보강에 쓴다.

전환은 설문 링크(https://survey.audeniq.com/) 클릭과 설문 완료를 구분한다. 완료 수는 설문 서비스의 응답 수로 확인한다.

## 후속 콘텐츠

별도 블로그는 만들지 않았다. 실제 이용자 질문과 확정된 운영 정보를 근거로 홈 FAQ와 안내 페이지를 갱신한다. 한국어 원문(index.html·content/pages.json)을 고치면 빌드가 각 언어의 누락 번역을 알려 주므로 같은 커밋에서 번역도 갱신한다. 검색어만 바꾼 중복 페이지나 검토되지 않은 자동 발행은 하지 않는다.
