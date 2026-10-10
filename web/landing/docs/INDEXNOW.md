# IndexNow 직접 제출 — AUDENIQ

AUDENIQ의 공식 공개 사이트 `https://audeniq.com`은 Cloudflare Crawler Hints에 의존하지 않고도 **IndexNow API로 직접 제출**할 수 있습니다. 키 파일과 직접 제출 스크립트는 이미 포함되어 있습니다.

## 현재 구성

- 키 원본: `web/landing/content/indexnow-key.txt`
- 공개 키: `https://audeniq.com/3cbb8c797ee29db8d62ab89f9dfc17b8.txt`
- 공개 페이지 목록: `web/landing/content/generated-manifest.json` (현재 6개)
- 제출 스크립트: `web/landing/scripts/submit-indexnow.mjs`
- 호출 엔드포인트: `https://api.indexnow.org/indexnow` (한 엔드포인트만 호출하면 참여 검색엔진에 공유)
- 자동 배포 **없음**: 커밋, 빌드, 테스트로는 제출되지 않습니다.

IndexNow 소유권 키는 공개 파일에 들어가는 값이므로 GitHub Actions Secret에 따로 복제할 필요는 없습니다. 단, 이 키 파일이 실제 도메인 루트에서 공개적으로 읽혀야 합니다.

## GitHub Actions에서 클릭 한 번으로 제출

1. 먼저 실제 운영사이트에 해당 커밋의 사이트 빌드를 **별도로 배포**합니다. 이 워크플로는 배포하지 않습니다.
2. GitHub 저장소 → **Actions** → **IndexNow — submit AUDENIQ public URLs** → **Run workflow** → `main` → **Run workflow**.
3. 실행 로그의 `Verify live site matches this revision` 단계에서 배포 상태와 공개 경로를 확인합니다.
4. 다음 단계에서 `IndexNow accepted 6 public URLs (HTTP 200 또는 202)` 로그가 나오면 검색엔진이 제출을 수신한 것입니다. 실제 색인은 검색엔진이 별도로 결정합니다.

운영사이트가 아직 현재 커밋과 일치하지 않거나 키 파일/공개 페이지를 찾을 수 없으면 **API 제출 전에 실패**하도록 설계되어 있습니다. 실패했다면 배포 및 URL을 먼저 확인하고 다시 실행하세요. 429 오류는 과도한 요청일 수 있으므로 연속 반복 실행하지 마세요.

## CLI로 직접 제출

```bash
cd web/landing
npm run check
npm run verify:live
npm run indexnow
```

`npm run indexnow`는 다음을 검사한 뒤에만 실제로 POST 요청을 보냅니다.

1. 배포된 `/site-version.json` 해시가 이 커밋의 공개 페이지 빌드 해시와 일치
2. `/<key>.txt` 파일을 200으로 읽고 내용이 키와 일치
3. 공개 URL이 각각 200이고 `X-Robots-Tag: noindex`가 없음

제출 대상은 `sitemap.xml`에 들어있는 **홈·배급·퍼블리싱·레이블·소개·FAQ** 공개 페이지뿐입니다. `studio.audeniq.com`, 관리자, 로그인, 정산, API URL은 제출하지 않습니다. 삭제된 페이지의 변경 알림은 현재 스크립트의 검사 조건(200 확인)과 별도로 처리해야 합니다.

## 전송과 색인을 구분해서 확인

- **전송 완료**: Actions 로그 `IndexNow accepted ...` (HTTP 200/202)
- **수집·색인 확인**: [Bing Webmaster Tools](https://www.bing.com/webmasters/)에서 사이트 소유권을 인증한 뒤 IndexNow 및 URL 검사 화면 확인
- **직접 키 검사**: 브라우저에서 위 공개 키 URL이 정확한 키 문자열만 반환하는지 확인
- **Google**: IndexNow 대상이 아니므로 Search Console에 `https://audeniq.com/sitemap.xml` 별도 등록

Cloudflare에서 Crawler Hints가 켜져 있어도 직접 제출은 별개로 동작합니다. 중복 알림을 줄이고 싶다면 Cloudflare 대시보드에서 Crawler Hints를 꺼도 되지만, 이는 사이트 설정이며 이 저장소의 코드 변경에는 포함되지 않습니다.
