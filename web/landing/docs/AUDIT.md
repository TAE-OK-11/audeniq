# AUDENIQ 발견 가능성 감사

## 2026-10-10 글로벌 SEO·AEO·GEO·LLMO·NEO 감사

대상: https://audeniq.com 공개 홍보 페이지와 언어별 버전. 기준 커밋 `b652121`(main). 관찰은 공개 HTTP 응답·크롤러 User-Agent 요청·로컬 Worker 렌더링 기준이며 배포 완료를 뜻하지 않는다. 적용 절차는 [fire-your-seo-agency](https://github.com/leopard627/fire-your-seo-agency)의 SKILL.md와 references(seo·aeo·geo·llmo·neo-naver·content·measure)를 따랐다. 플러그인 설치 명령은 대화형 CLI 전용이라 이 환경에서는 저장소 문서를 직접 읽어 같은 절차를 수행했다.

### 변경 전 상태 (실서비스 관찰)

| 항목 | 결과 | 판정 |
| --- | --- | --- |
| HTML 원본 본문·title·description·canonical·H1 | 6개 한국어 페이지 모두 JS 없이 읽힘, 고유 메타, 단일 H1 | 정상 |
| robots.txt / X-Robots-Tag | 모든 봇 허용, /studio·/admin·/api·/login·/health 제외. 공개 페이지에 noindex 없음 | 정상 |
| sitemap.xml | 6개 URL, lastmod 있음, hreflang 없음 | 정상(다국어 없음) |
| JSON-LD | Organization·WebSite·WebPage/FAQPage·Service·Breadcrumb, 본문과 일치 | 정상 |
| OG·X 카드 | 1200×630 이미지, 고유 제목·설명 | 정상 |
| 크롤러 UA 응답 | Googlebot·bingbot·Yeti·OAI-SearchBot·PerplexityBot·Claude-SearchBot·GPTBot·YandexBot·Baiduspider 모두 200 | 정상(UA 기준) |
| 404·중복 URL | 없는 경로 404+noindex, index.html·슬래시 없는 별칭 308 1홉 | 정상 |
| www.audeniq.com | DNS 레코드 없음(접속 불가). Worker의 308 규칙은 준비됨 | 수동 조치 필요 |
| IndexNow | 키 파일 200, 수동 워크플로 존재. 매번 전체 URL 제출 | 개선 |
| 다국어 | 한국어만 존재. /en/ 404, hreflang 없음 | 누락(P0) |
| AEO 질문 | "어데닉" 표기 0회, "음원 배급사인가?"·"AUDENIQ Publishing은 무엇?" 직접 답 없음 | 누락(P1) |
| 브랜드 한·영 관계 | JSON-LD alternateName 없음 | 누락(P1) |
| 설문 CTA | Google Forms 외부 링크 | 변경 요청 반영 |
| 모바일 성능(랩) | 홈 Lighthouse 성능 88·TBT 390ms(실서비스, Cloudflare 챌린지 스크립트 포함). 느린 모바일에서 메뉴가 열린 채 그려졌다 닫히는 CLS 0.388 재현 | 개선(P1) |
| 접근성(랩) | 안내 페이지 관련 카드 라벨 대비 2.64:1, FAQ 영역 파란 글자 4.46:1 | 개선(P2) |
| 문서 불일치 | README가 학습 봇 차단이라고 설명하나 실제 robots는 전체 허용(소유자 커밋 b1ba5c7) | 정리 |
| 실사용 CWV·노출·클릭·AI 인용 | 계정·데이터 접근 없음 | 미측정 |

### 구현 결과

| 우선순위 | 영역 | 결과 |
| --- | --- | --- |
| P0 | 다국어 | 14개 언어 홈페이지(ko 루트 유지 + /en/ /ja/ /zh-cn/ /zh-tw/ /es/ /pt-br/ /fr/ /de/ /id/ /vi/ /th/ /hi/ /ar/), ko·en 안내 페이지 5종씩. 총 24개 URL |
| P0 | 국제화 신호 | 언어별 html lang·dir, 자기 canonical, 양방향 hreflang + x-default(/en/), sitemap xhtml:link, og:locale·alternate, 푸터 언어 메뉴(같은 페이지의 언어 버전으로 이동). 자동 리디렉션 없음 |
| P1 | AEO | FAQ 12개로 재정리: 첫 문장 직답, "AUDENIQ(어데닉)", 음원 배급사 여부, AUDENIQ Publishing, 지금 발매 가능 여부, 지원 플랫폼 미확정 명시. 모든 언어 동일 사실 |
| P1 | GEO·LLMO | Organization·WebSite alternateName "어데닉", llms.txt 한·영 요약·정확히 지켜야 할 사실·언어별 URL, llms-full.txt에 ko·en 전문 |
| P1 | 성능·CLS | 모바일 메뉴 초기 상태를 head에서 확정해 CLS 0.388→0, 인라인 base64 로고 2개 제거(정적 SVG, 실행 중 교체 스크립트 삭제), CJK·베트남어는 Pretendard 미로드 |
| P1 | IndexNow | 페이지별 해시를 매니페스트에 기록, `--since <ref>`로 추가·변경·삭제 URL만 제출. 기존 키 유지 |
| P2 | 접근성 | 안내 페이지 대비 4.5:1 이상으로 보정 |
| P2 | 회귀 검증 | 언어별 크롤러 관점 테스트(test/i18n.test.ts) 추가 |
| 요청 | 설문 | 모든 언어의 설문 버튼을 https://survey.audeniq.com/ 으로 변경 |

확정되지 않은 DSP·Merlin 계약, 가격·수수료, 리뷰·평점, 실적 수치는 어떤 언어에도 넣지 않았다. 각 번역은 한국어 원문에 없는 사실을 추가하지 않는다(원문 문장 단위 대응).

### 검증 (로컬, 2026-10-10)

- `npm run check`: 25개 테스트 통과 — 24개 URL 200, 언어별 lang/dir/canonical/title/description 고유성, hreflang 상호 참조·sitemap 일치, 미번역 한글 0, 번역 누락·잔여 키 0, JSON-LD 1개·본문 일치·가격/평점 없음, 언어 메뉴 링크 200, 언어 URL 별칭 308·없는 하위 경로 404, Accept-Language 무시, robots 비차단, 설문 링크.
- `deploy/aws/test/aws.test.ts`(CloudFront 라우팅 동일성) 7개, `web/shared/transport.test.ts` 6개 통과.
- Chromium 렌더링: 14개 언어 홈 + 안내 페이지를 320·390·1440px에서 가로 넘침 0, 스크립트·자산 오류 0. 아랍어 RTL 미러링, JS 켜짐/꺼짐 모바일 메뉴 동작 확인.
- Lighthouse 13(랩, 모바일 기본 스로틀링, 같은 머신에서 로컬 Worker 기준. 실사용 CWV 아님):

| 페이지 | 변경 전 | 변경 후 |
| --- | --- | --- |
| / | 성능 99·접근성 100·SEO 100, LCP 1.7–1.8s, CLS 0, 78KiB | 성능 98–99·접근성 100·SEO 100, LCP 1.7–1.9s, CLS 0, 78KiB |
| /about/ | 성능 100·접근성 95 | 성능 100·접근성 100 |
| /en/ /ja/ /hi/ | — | 성능 98–99·접근성 100·SEO 100, CLS 0 |
| /ar/ | — | 성능 98·접근성 100·SEO 100, CLS 0 (수정 전 간헐 0.388) |

랩 측정은 네트워크·CPU 조건에 따라 변한다. 느린 모바일 재현(CPU 6×, 150ms 지연)에서 메뉴로 인한 CLS 0.388이 모든 언어에서 0으로 줄었다. 실서비스 수치는 Cloudflare가 주입하는 챌린지·Insights 스크립트 때문에 로컬보다 TBT가 크다(변경 전 실서비스 TBT 390ms 중 챌린지 스크립트 645ms 실행).

## 2026-10-08 감사 (이전)

관찰일 2026-10-08, 기준 커밋 6d1fe11. 홈페이지 1개 URL sitemap, llms·RSS 404였던 상태에서 공식 안내 5개 페이지·공통 원본 생성기·robots·IndexNow 스크립트를 도입했다. 상세는 git 기록을 참고한다.
