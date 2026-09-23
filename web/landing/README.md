# AUDENIQ — Cloudflare Workers 배포·기본 SEO

기존 홍보 페이지와 AUDENIQ STUDIO 디자인·애니메이션은 그대로 보존했습니다. **홍보 페이지는 검색 대상**, 아티스트 포털 STUDIO와 미리보기 주소는 **검색 제외**입니다. STUDIO의 `noindex`는 검색 방지일 뿐, 인증·접근제어나 개인정보 보호 기능이 아닙니다.

## 배포 경로 (권장: Wrangler로 Worker + 정적 assets 배포)

현재 프로젝트는 **하나의 Worker가 호스트 이름을 읽어 홍보 사이트와 STUDIO를 분기**합니다.

| 요청 URL | 제공 화면/동작 |
| --- | --- |
| `https://audeniq.com/` | 검색 가능한 홍보 페이지 |
| `https://studio.audeniq.com/` | STUDIO (검색 제외) |
| `https://<worker>.<account>.workers.dev/` | 홍보 페이지 미리보기 (HTTP `X-Robots-Tag: noindex`) |
| `https://<worker>.<account>.workers.dev/studio/` | STUDIO 미리보기 (검색 제외) |
| `https://www.audeniq.com/` | `https://audeniq.com/`으로 308 리디렉션 |

사용 환경: Node.js 20+ 또는 Bun 1.4.x, Cloudflare 계정, Wrangler 로그인.

```bash
# 프로젝트 폴더 내에서
npm install
npm run check
npx wrangler login
npm run deploy
```

배포 후 Cloudflare 대시보드에서 Worker `audeniq-web`에 `audeniq.com`과 `studio.audeniq.com`을 **Custom Domain**으로 연결합니다. `www.audeniq.com`까지 308 정규 주소 리디렉션을 사용하려면 이 호스트도 같은 Worker에 연결하세요. DNS·도메인 소유권·실제 배포는 이 ZIP 생성만으로 완료되지 않습니다.

로컬 미리보기: `npm run dev` → 홍보 `/`, STUDIO `/studio/`.

**Cloudflare 대시보드의 ZIP 드래그 앤 드롭 정적 업로드 주의:** ZIP 최상위에도 `index.html`, `studio/index.html`, `robots.txt`, `sitemap.xml`, `assets/`를 두었으므로 정적 파일로 업로드하는 용도에는 사용할 수 있습니다. 그러나 **정적 ZIP 업로드만으로는 `src/worker.js`의 호스트별 라우팅, www 리디렉션 및 미리보기용 X-Robots-Tag가 실행되지 않습니다.** 이 동작이 필요한 현재 구성에서는 위의 Wrangler 배포를 사용하세요. `public/`은 Wrangler가 사용하는 정적 파일 디렉터리이며, 최상위 복사본과 내용이 같습니다.

## SEO에 포함된 설정

- 홍보 페이지: 검색 허용, 한국어 title/description, `https://audeniq.com/` 정규 URL, Open Graph / X(Twitter) 공유 메타정보.
- `assets/social-preview.png`: 현재 브랜드 로고를 사용한 **실제 1200×630 PNG**. favicon SVG/ICO도 포함.
- JSON-LD: 과장된 평점이나 확인되지 않은 사업자 정보 대신 `Organization`, `WebSite` 기본 정보만 표기.
- `https://audeniq.com/robots.txt`: 검색 허용 + STUDIO/health 크롤링 제한 + 사이트맵 위치 제공.
- `https://audeniq.com/sitemap.xml`: 현재 **실제로 공개할 수 있는 단일 홍보 페이지(`/`)만** 등록. `#about` 같은 문서 내 앵커나 STUDIO는 개별 색인 URL로 넣지 않았습니다.
- Worker: STUDIO·workers.dev·별도 도메인 미리보기는 `X-Robots-Tag: noindex` 적용, 미리보기용 robots.txt는 전체 차단. 없는 홍보 URL은 `404`, `/index.html`은 `/`로 리디렉션.

### 중요: 공식 도메인이 변경될 경우

현재 공식 주소는 **`https://audeniq.com/`을 사용할 예정이라는 전제로** 설정했습니다. 실제로 구매·연결할 공식 도메인이 다르다면 **배포 전에** 다음 내용을 함께 교체하세요.

1. `src/worker.js` 맨 위 `PRIMARY_HOST` 상수.
2. `index.html`과 `public/index.html`의 canonical, OG URL·이미지 URL·구조화 데이터 URL.
3. `sitemap.xml`, `robots.txt` 및 `public/`의 동명 파일.
4. 이 README의 연결할 커스텀 도메인.

일부만 바꾸면 검색엔진에서 주소가 충돌하거나 SNS 공유 이미지가 깨질 수 있습니다. **구매한 도메인을 실제로 연결하기 전까지는 도메인에 대한 실서비스 검색·공유 동작을 확인할 수 없습니다.**

## 배포 후 직접 확인할 주소

브라우저로 `https://audeniq.com/`, `/robots.txt`, `/sitemap.xml`, `/assets/social-preview.png`가 200으로 열리는지 확인하고, `https://studio.audeniq.com/`의 응답 헤더에 `X-Robots-Tag: noindex`가 있는지 확인하세요. 이후 Google Search Console의 도메인 소유권 인증과 `https://audeniq.com/sitemap.xml` 제출을 직접 진행하면 됩니다. 색인 여부나 순위는 검색엔진이 결정하며 자동으로 보장되지 않습니다.
