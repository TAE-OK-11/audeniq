# AUDENIQ Survey — 정적 자산 우선 / D1 비공개 / 초저 CPU 배포본

## 핵심 변경 사항

기존 설문 18문항, 6단계 UI, V4 응답 검증, D1 테이블, Turnstile 검증, 12개월 삭제 크론을 그대로 둡니다. `survey.audeniq.com`의 HTML·CSS·JS·로고·파비콘·robots.txt·404 페이지는 Cloudflare Static Assets에서 **Worker 스크립트 실행 없이** 제공합니다. `run_worker_first`는 `["/api/*", "/health"]`로 제한했습니다. 존재하지 않는 경로에 대한 일부 비탐색 요청이 Worker로 떨어질 수 있습니다. Worker와 Cloudflare 인프라를 모두 포함한 모든 CPU가 0이라는 뜻은 아닙니다.

기존에는 페이지가 열릴 때마다 `/api/config`를 호출했습니다. 지금은 **공개 가능한** Turnstile Site Key를 정적 HTML 메타태그에 넣으므로 일반 페이지 조회 시 API 호출이 없습니다. 실제 응답 제출 `POST /api/responses`, 운영자 점검용 `GET /api/config`, `GET /health` 및 매일 보관기간 삭제 크론만 Worker 코드를 실행합니다. 비밀 키는 Worker Secret으로, DB는 Worker의 `env.DB`로만 접근하며 브라우저에 DB ID나 권한을 제공하지 않습니다. D1 읽기·쓰기 작업은 설문 제출/일일 삭제 때 서버에서 처리합니다. `/api/config`는 배포 준비 여부와 공개 Site Key만 반환합니다.

정적 요청용 보안 헤더는 `public/_headers`에서 설정했습니다. `html_handling=auto-trailing-slash`로 `/`에 `public/index.html`을 제공하고, `not_found_handling=404-page`로 없는 탐색 주소도 정적 404를 제공합니다. `/api/*` 요청은 정적 응답으로 빠지지 않도록 Worker-first로 지정했습니다.

## 운영 중인 /root/audeniq-survey 업데이트 — **기존 DB 보존**

다음은 ZIP을 `/root/AUDENIQ-survey-V5-motion-Workers-D1.zip`에 올린 후 실행할 명령입니다. 기존 `audeniq-web` 및 `audeniq-studio` 프로젝트는 손대지 않습니다.

```bash
cd /root/audeniq-survey
cp -a wrangler.jsonc wrangler.jsonc.before-motion
cp -a /root/audeniq-survey "/root/audeniq-survey-backup-$(date +%Y%m%d-%H%M%S)"
unzip -o /root/AUDENIQ-survey-V5-motion-Workers-D1.zip -d /root/audeniq-survey
bun install
bun scripts/restore-existing-config.mjs wrangler.jsonc.before-motion
bun run check
bun run deploy
```

**주의:** 기존 `wrangler.jsonc`를 단순히 복원하면 구버전 `run_worker_first:true`까지 복원되어 최적화가 무효화됩니다. 대신 `restore-existing-config.mjs`를 사용해 기존 D1 ID와 공개 키만 새 설정에 이식해야 합니다. 기존 원격 DB에 **새 DB 생성 또는 기존 마이그레이션 재적용은 필요하지 않습니다.** Worker Secret은 클라우드 계정에 별도로 보관하므로 설정 JSON·ZIP에 입력하지 않습니다. 재배포 후 `/api/config` 응답이 `ready:true`인지 확인하세요.

처음 설치라면 `bun scripts/set-config.mjs '실제_D1_ID' '실제_Turnstile_공개_Site_Key'`를 먼저 실행하면 워커 설정과 **정적 HTML 공개 Site Key가 동시에 변경**됩니다. Turnstile Secret Key는 `bunx wrangler secret put TURNSTILE_SECRET_KEY`로 Cloudflare에 별도 등록해야 합니다.

## 배포 후 검증

```bash
curl -sS -o /dev/null -w '홈페이지 HTTP %{http_code}\n' https://survey.audeniq.com/
curl -sS -o /dev/null -w 'CSS HTTP %{http_code}\n' https://survey.audeniq.com/style.css
curl -sS -o /dev/null -w '404 HTTP %{http_code}\n' https://survey.audeniq.com/this-page-does-not-exist
curl -sS https://survey.audeniq.com/api/config
# 실제 설문 테스트 제출 후에만 실행 (전후 차이를 비교)
bunx wrangler d1 execute audeniq-survey --remote --command "SELECT COUNT(*) AS total FROM survey_responses;"
```

브라우저 개발자 도구 Network에서 설문 페이지를 열었을 때 `/api/config` 요청이 없고, **제출 시에만** `/api/responses`가 호출되는지 확인합니다. `/api/config`는 운영자가 별도 호출하면 Worker 사용량이 발생합니다. 기존 D1 데이터와 제출 접수번호의 중복 방지 로직은 그대로 유지됩니다. Cloudflare Workers Analytics에서 페이지 조회와 제출 시의 Worker 호출 수를 실제로 비교해 보세요. 로컬 자동화 테스트 통과는 실서비스 네트워크에서의 최종 정상 제출을 대신하지 않습니다.


## V5: 홍보 홈페이지 스타일 애니메이션
- 상단 소개 문구 및 응답 활용 안내: 스크롤 진입 시 위로 12~16px 부드럽게 등장, 760ms cubic-bezier(.22,.61,.36,1) (홍보 홈페이지와 동일).
- 진행률과 **현재 보이는 단계의 질문 카드**만 차례로 등장. 선택지, 텍스트 입력창 등 실제 클릭 요소에는 등장 애니메이션을 걸지 않아 선택/취소 표시를 방해하지 않음.
- 이전/다음 페이지, 제출 완료 화면에도 가벼운 전환 효과. 기존 선택 상태, D1 처리, Turnstile 구현은 변경하지 않음.
- 줄임 모션 선호, 화면 비활성화, 인쇄, 키보드 포커스, JS 비활성 시 콘텐츠 표시 보장.
- 애니메이션은 `public/motion.js`에서 **방문자의 브라우저**에서만 실행. Worker 코드/CPU와 D1 쿼리를 추가하지 않음.
- 기존 운영 D1/Turnstile 유지: 기존 설정 백업 후 ZIP 적용, `bun scripts/restore-existing-config.mjs wrangler.jsonc.before-motion` → `bun run check` → `bun run deploy`. `bunx wrangler d1 create`나 migrations 재실행 불필요.


## V5.1: 홍보 페이지와 설문 헤더 일치
- 로고 SVG는 기존 홍보 홈페이지와 동일한 파일을 유지합니다. 데스크톱 158px/80px, 태블릿(761–900px) 138px/68px, 모바일 138px/68px 및 x 위치(-8px 오프셋)를 맞췄습니다.
- 모바일의 중복 리서치 라벨을 숨겨 버튼과 로고가 좁은 화면에서 서로 밀지 않도록 했습니다.
- 설문 페이지에서 로고를 누르면 **현재 설문 페이지 맨 위**(`#top`)로 이동합니다. 오른쪽 `공식 홈페이지` 버튼은 기존처럼 홈페이지로 연결됩니다. 문항 입력값과 현재 설문 단계는 초기화하지 않습니다.
- 로고·헤더 수정은 정적 HTML/CSS만 변경하므로 Worker API/D1/Turnstile 로직에는 변화가 없습니다.
- 기존 운영 프로젝트에서는 전체 ZIP보다 **V5.1 헤더 패치 ZIP**에 포함된 `public/index.html`과 `public/style.css`만 덮어쓰면 D1·Turnstile 설정을 안전하게 유지할 수 있습니다.
