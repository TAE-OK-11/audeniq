// CloudFront 배포 목록 (README.md)
// - origin 'backend': 화면·API 모두 백엔드 api(443, crates/core/src/edge_tls.rs)가 서빙한다. S3 없음 (Studio)
// - origin 's3': 화면은 S3, `api` 경로만 백엔드로 (admin, landing)
//   dir      S3에 올릴 빌드 결과 (저장소 루트 기준)
//   headers  Cloudflare `_headers` 파일 — 보안 헤더(CloudFront 응답 헤더 정책)와 Cache-Control(S3 메타데이터)의 원본
//   api      백엔드로 보낼 경로. 비어 있으면 S3만
//   site     백엔드에 x-audeniq-site 로 알리는 화면 이름 (crate::sites)
// 백엔드로 가는 요청은 모두 functions/edge.js 가 헤더를 다듬는다(프록시 Worker의 web/shared/transport.ts 규칙).
// 설문(survey.audeniq.com)은 베타 전까지 D1 Worker 그대로라 여기 없다.
interface Common {
  aliases: string[];
  /** 백엔드로 가는 /api 응답에 붙일 CORS Origin. 없으면 CORS 헤더를 지운다 (functions/edge.js) */
  corsOrigin?: string;
  /** /api 응답을 늘 no-store·nosniff로 (관리자 콘솔) */
  apiNoStore?: boolean;
}

/** 화면도 백엔드가 서빙 (SPA 폴백·_redirects·_headers 모두 백엔드, crates/core/src/studio_static.rs) */
export interface BackendApp extends Common {
  origin: 'backend';
  site: string;
  /** Public static paths; origin headers determine fingerprinted vs mutable TTL. */
  cached: string[];
}

interface S3Base extends Common {
  origin: 's3';
  dir: string;
  /** deploy/aws/functions 안의 뷰어 요청 함수 파일 */
  function: string;
  api: string[];
  /** api 가 있으면 필요 (없으면 x-audeniq-site 헤더 값이 빠진다) */
  site?: string;
  /** 같은 함수를 viewer-response 에도 건다 */
  viewerResponse?: boolean;
  /** PAGE_PATHS·FILE_PATHS 를 내보내는 모듈 (저장소 루트 기준) — 함수의 ROUTES 자리표시에 넣는다 */
  routes?: string;
  /** 404 오류 응답 페이지 경로 */
  notFoundPage?: string;
}

/** 보안 헤더 원본: `_headers` 파일 경로나 그 내용 중 하나 */
export type S3App = S3Base & (
  | { headers: string; headersText?: undefined }
  | { headersText: string; headers?: undefined }
);

export type App = BackendApp | S3App;

export type AppName = 'studio' | 'admin' | 'landing';

export const APPS: Record<AppName, App> = {
  studio: {
    origin: 'backend',
    aliases: ['studio.audeniq.com'],
    site: 'studio',
    cached: [
      '/assets/*', '/static/*', '/favicon.ico', '/robots.txt',
      ...['png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'avif', 'ico',
        'woff', 'woff2', 'ttf', 'otf', 'mp3', 'mp4', 'pdf'].map(ext => `/*.${ext}`),
    ],
    corsOrigin: 'https://studio.audeniq.com',
  },
  admin: {
    origin: 's3',
    aliases: ['admin.audeniq.com'],
    dir: 'web/admin/dist',
    headers: 'web/admin/public/_headers',
    function: 'admin.js',
    api: ['/api/*'],
    site: 'admin',
    apiNoStore: true,
  },
  landing: {
    origin: 's3',
    aliases: ['audeniq.com', 'www.audeniq.com'],
    dir: 'web/landing/public',
    // 랜딩은 _headers 대신 web/landing/src/worker.ts 의 withSecurityHeaders 와 같은 값
    headersText: [
      '/*',
      '  X-Content-Type-Options: nosniff',
      '  Referrer-Policy: strict-origin-when-cross-origin',
      '  Permissions-Policy: camera=(), microphone=(), geolocation=(), payment=()',
      "  Content-Security-Policy: frame-ancestors 'none'; base-uri 'self'; object-src 'none'",
      '  Cross-Origin-Opener-Policy: same-origin',
      '  X-AUDENIQ-Site: landing',
    ].join('\n'),
    function: 'landing.js',
    viewerResponse: true,
    routes: 'web/landing/src/public-routes.ts',
    api: [],
  },
};

/** 백엔드로 가는 경로가 있는지 */
export const usesBackend = (app: App): boolean => app.origin === 'backend' || app.api.length > 0;

export const isAppName = (name: string | undefined): name is AppName =>
  name !== undefined && Object.hasOwn(APPS, name);

export const stackName = (app: string): string => `audeniq-${app}-ui`;

export interface HeaderRule {
  path: string;
  headers: [name: string, value: string][];
}

export interface CacheGroup {
  include: string;
  cacheControl: string;
}

/** Cloudflare `_headers` → [{ path, headers: [[name, value]] }] */
export function parseHeaders(text: string): HeaderRule[] {
  const rules: HeaderRule[] = [];
  for (const line of text.split('\n')) {
    if (!line.trim() || line.trim().startsWith('#')) continue;
    if (!/^\s/.test(line)) { rules.push({ path: line.trim(), headers: [] }); continue; }
    const at = line.indexOf(':');
    const rule = rules.at(-1);
    if (at < 0 || !rule) throw new Error(`_headers: 해석할 수 없는 줄: ${line}`);
    rule.headers.push([line.slice(0, at).trim(), line.slice(at + 1).trim()]);
  }
  return rules;
}

/**
 * _headers의 Cache-Control 규칙 → S3 업로드 그룹 (aws s3 sync --include 패턴, 먼저 올릴 것부터).
 * "/"·"/index.html" 같은 HTML 진입점은 맨 마지막에 올려, 새 HTML이 아직 없는 에셋을 가리키지 않게 한다.
 */
export function cacheGroups(rules: HeaderRule[]): CacheGroup[] {
  const groups: CacheGroup[] = [];
  for (const { path, headers } of rules) {
    const value = headers.find(([name]) => name.toLowerCase() === 'cache-control')?.[1];
    if (!value || path === '/*') continue;
    const pattern = path === '/' ? 'index.html' : path.replace(/^\//, '');
    if (pattern.endsWith('.html')) continue;
    groups.push({ include: pattern, cacheControl: value });
  }
  return groups;
}
