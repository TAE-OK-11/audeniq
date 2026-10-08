# AUDENIQ 디자인 템플릿

홍보 페이지(`web/landing`), STUDIO(`web/studio`), 설문(`web/survey`)이 같이 쓰는 디자인 언어입니다. 세 사이트에 공통으로 나오는 요소는 여기 한 곳에서만 모양을 정하고, 각 사이트는 배치(여백·크기·줄바꿈)만 덧붙입니다.

| 파일 | 내용 |
| --- | --- |
| `tokens.css` | 색, 글꼴, 모서리, 그림자, 움직임 곡선. 예전 이름(`--blue`, `--ink` 등)도 같은 값을 가리켜 기존 CSS가 그대로 동작합니다. |
| `components.css` | 공통 요소. 클래스는 모두 `aq-`로 시작합니다. |
| `controls.css` | 체크박스·라디오·선택 상자·날짜 입력 (원래 STUDIO 파일). `web/admin`은 같은 내용의 복사본을 씁니다. |

## 어떤 값을 골랐나

정한 순서는 **STUDIO에 이미 있는 요소는 STUDIO 모양 그대로 → 홍보 페이지 → STUDIO → 설문**입니다.

| 요소 | 클래스 | 출처 |
| --- | --- | --- |
| 색 팔레트, 글꼴 | `tokens.css` | 세 사이트 공통 값 · 글꼴 목록은 홍보 페이지 |
| 헤더 (흰 바탕, 아래 줄, 스크롤 그림자, 로고 158px) | `aq-header`, `aq-header__inner`, `aq-brand` | STUDIO |
| 헤더 라벨 (`STUDIO`, `RESEARCH`) | `aq-header-pill` | STUDIO |
| 헤더 링크 (`사전 설문 안내`, `공식 홈페이지`) | `aq-header-link` | 홍보 페이지 (STUDIO에 없음) |
| 기본 버튼 · 화살표 원 | `aq-button`, `aq-button__arrow` | STUDIO `.button`, `.aq-hero-cta` |
| 큰 버튼 | `aq-button--lg` | STUDIO 모바일 큰 버튼 크기 |
| 보조 버튼 | `aq-button--soft` | STUDIO `.button.secondary` |
| 작은 제목 | `aq-eyebrow` (`--lg`는 홍보 페이지 크기) | STUDIO `.eyebrow` |
| 상단 패널 (옅은 파랑, 오른쪽 위 빛) | `aq-hero` | STUDIO 홈 패널 |
| 카드 | `aq-card` | STUDIO `.surface` |
| 배지 | `aq-chip`, `aq-chip--blue/green/orange/violet/gray` | STUDIO `.status-chip` |
| 푸터 바탕 · 맨 아래 한 줄 | `aq-footer`, `aq-footer__bar` | 홍보 페이지 (STUDIO에 없음) |
| 본문으로 건너뛰기 | `aq-skip-link` | STUDIO |
| 키보드 초점 테두리 | `:focus-visible` (우선순위 0) | STUDIO |

STUDIO 화면 자체는 그대로입니다. STUDIO는 이 템플릿의 원본이라 토큰만 여기서 불러오고, 기존 클래스(`.portal-header`, `.button`, `.surface`, `.status-chip`)는 위 표의 템플릿 요소와 같은 모양을 냅니다.

## 사이트별 연결

- **홍보 페이지**: `scripts/build-seo.mjs`가 `tokens.css`와 `components.css`를 `index.html`의 `<!-- AUDENIQ_DESIGN_START -->` 블록에 넣습니다(한 요청 유지). 공식 안내 페이지는 같은 스타일을 담은 `assets/guide.css`를 씁니다. 템플릿을 고친 뒤 `npm run build:seo`를 실행하세요. 이 블록은 직접 고치지 않습니다.
- **STUDIO**: `app/src/App.tsx`에서 토큰·공통 요소·입력 요소를 불러옵니다. 빌드할 때 쓰지 않는 공통 요소는 자동으로 빠집니다.
- **설문**: `app/src/style.css` 맨 위의 `@import`. 템플릿을 고친 뒤 `bun run build`로 `public/`을 다시 만드세요.

## 마크업

```html
<header class="aq-header">
  <div class="aq-header__inner">
    <a class="aq-brand" href="/"><img src="/assets/AUDENIQ_Logo_Light.svg" alt="AUDENIQ"></a>
    <span class="aq-header-pill">RESEARCH</span>
    <div class="aq-header__end">
      <a class="aq-header-link" href="https://audeniq.com/">공식 홈페이지 <span aria-hidden="true">↗</span></a>
    </div>
  </div>
</header>

<section class="aq-hero">
  <p class="aq-eyebrow aq-eyebrow--lg">AUDENIQ RESEARCH</p>
  <h1>…</h1>
  <a class="aq-button aq-button--lg" href="…">참여하기 <span class="aq-button__arrow" aria-hidden="true">→</span></a>
</section>

<article class="aq-card">… <span class="aq-chip aq-chip--blue">준비 중</span></article>

<footer class="aq-footer">
  <div class="(사이트 컨테이너)">
    <div class="aq-footer__bar"><small>© AUDENIQ</small><a href="#top">맨 위로 ↑</a></div>
  </div>
</footer>
```

스크롤하면 헤더에 `is-scrolled`를 붙여 그림자를 켭니다.

## 규칙

- 공통 요소의 색·모서리·글자 크기를 사이트 CSS에서 다시 정하지 않습니다. 바꿀 일이 있으면 이 폴더를 고쳐 세 사이트에 함께 반영합니다.
- 사이트 CSS는 템플릿 뒤에 와서 여백, 너비, 줄바꿈 같은 배치만 덧붙입니다.
- 새 공통 요소는 같은 순서로 출처를 정합니다. STUDIO에 이미 있으면 STUDIO 모양을 그대로 옮깁니다.
- STUDIO CSS에는 `--aq-blue`, `--aq-ink`, `--aq-ease`처럼 이름이 같은 변수가 있습니다. 값이 같은 것만 같은 이름을 쓰고, 값이 다른 것은 다른 이름을 씁니다(`--aq-text-muted`, `--aq-soft-bg`).
