# AUDENIQ 공개 웹사이트

`audeniq.com`의 한국어 공식 홈페이지와 서비스 안내를 제공하는 Cloudflare Worker입니다. 공개 문서는 정적 HTML이며 JavaScript 없이 읽을 수 있습니다. STUDIO는 별도 Worker(`web/studio`)에서 운영합니다.

## 공개 페이지와 원본

| URL | 내용 | 수정 원본 |
| --- | --- | --- |
| `/` | 홈페이지와 11개 FAQ | `index.html` |
| `/music-distribution/` | 음원 배급·발매 준비 | `content/pages.json` |
| `/publishing/` | 작곡가·작사가의 음악 퍼블리싱 | `content/pages.json` |
| `/label-services/` | 독립 레이블의 카탈로그·발매·정산 | `content/pages.json` |
| `/about/` | 브랜드·현재 상태·공식 연락처 | `content/pages.json` |
| `/faq/` | 홈페이지와 같은 질문·답변 | 홈페이지 FAQ에서 생성 |

수수료율과 가격은 공개 콘텐츠에 넣지 않습니다. 출시 준비 상태와 계약 미확정 범위를 유지합니다.

## 수정·검증

Node.js 20+로 실행합니다. SEO 생성·테스트에는 외부 패키지가 필요하지 않습니다.

```bash
cd web/landing
npm run build:seo
npm run check
```

`content/pages.json`의 `updated`는 실제 검토·수정일입니다. 빌드 때마다 날짜를 바꾸지 않습니다. 생성기는 서비스 HTML과 같은 문구의 JSON-LD, 링크, sitemap·RSS·robots, llms.txt·llms-full.txt, 공개 라우트·버전 목록을 함께 생성합니다. `public/`의 생성 파일을 직접 수정하지 마세요. 홈페이지·공유 이미지·아이콘과 발견용 파일의 최상위 복사본도 유지합니다.

공식 안내 페이지는 홈페이지의 글꼴·색상·헤더·푸터·카드·FAQ 스타일을 공유합니다. `content/guide.css`는 안내 페이지의 반응형 배치만 추가하며, 빌드에서 홈페이지 스타일과 합쳐 `assets/guide.css`와 공개 복사본을 생성합니다. 현재 공식 SVG 로고를 JavaScript 없이 바로 표시하고 모바일 메뉴는 기본 HTML details로 동작합니다. RSS는 화면에 표시하지 않고 발견용 메타와 파일로 유지합니다.

테스트는 HTTP 상태·canonical·메타정보·한 개의 H1·본문/FAQ 스키마 일치·내부 링크·404·STUDIO 리디렉션·미리보기 noindex·검색/학습 봇 정책·공개 콘텐츠의 금액/요율 제외를 확인합니다.

## SEO·AEO·GEO·LLMO·Naver NEO

- 고유 title/description·canonical·Open Graph·X 공유 정보.
- 일관된 Organization·WebSite ID 및 WebPage/AboutPage/FAQPage·Service·BreadcrumbList. 가격·평점·미확정 계약 없음.
- Googlebot·Bingbot·Naver Yeti와 AI 검색/사용자 조회 봇 허용. STUDIO·admin·API·login·health 제외.
- GPTBot·ClaudeBot·Google-Extended·CCBot·Applebot-Extended의 학습 수집 차단. robots는 협조적인 크롤러 정책이며 인증을 대체하지 않습니다.
- 공개 HTML 6개만 sitemap에 등록. RSS는 공식 안내 업데이트입니다.
- llms.txt와 llms-full.txt는 보조 텍스트입니다. Google AI 검색 노출의 필수 조건이나 순위 보장 수단으로 설명하지 않습니다.
- FAQ 스키마는 실제 본문과 일치하며 2026년에 종료된 Google FAQ rich results 표시를 전제로 하지 않습니다.

감사는 [docs/AUDIT.md](docs/AUDIT.md), 계정 설정·성과 측정은 [docs/SEARCH.md](docs/SEARCH.md)를 참고하세요.

## 라우팅과 향후 수동 배포

이번 변경은 커밋만 하며 배포는 실행하지 않습니다. 자동 배포 워크플로를 추가하지 않았습니다.

| 요청 | 동작 |
| --- | --- |
| 공식 호스트의 공개 문서 | 검색 가능한 HTML |
| www 호스트 | apex로 308, 알려진 별칭을 한 번에 정규화 |
| index.html 또는 슬래시 없는 서비스 별칭 | canonical로 308, 쿼리 유지 |
| 공개 목록에 없는 페이지·파일 | 실제 404 + noindex |
| workers.dev 등 미리보기 | HTML HTTP noindex, robots 전체 차단, sitemap 404 |
| STUDIO 호스트·/studio·?site=studio | studio.audeniq.com으로 308 |

별도로 배포를 결정했을 때 Cloudflare 권한으로 기존 Worker `audeniq-web`을 배포하세요.

```bash
npm run deploy
npm run verify:live
npm run indexnow
```

`verify:live`는 실제 사이트와 현재 빌드를 비교합니다. `indexnow`는 배포된 버전·공개 키·URL을 검증한 뒤에만 검색엔진에 갱신 알림을 보냅니다. 빌드·테스트·커밋에서는 호출하지 않습니다. 색인과 순위는 검색엔진이 결정합니다.

도메인이 바뀌면 생성기의 origin·Worker PRIMARY_HOST·리디렉션·서비스 출처 링크·문서를 함께 갱신합니다. STUDIO의 검색 차단은 별도 프로젝트 설정도 확인하세요.
