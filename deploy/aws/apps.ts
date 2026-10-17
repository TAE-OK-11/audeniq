// 화면(UI)은 S3, /api 는 백엔드(ALB)로 보내는 CloudFront 배포 목록 (README.md)
//   dir      S3에 올릴 빌드 결과 (저장소 루트 기준)
//   headers  Cloudflare `_headers` 파일 — 보안 헤더(CloudFront 응답 헤더 정책)와 Cache-Control(S3 메타데이터)의 원본
//   api      백엔드로 보낼 경로. 비어 있으면 S3만
//   site     백엔드에 x-audeniq-site 로 알리는 화면 이름 (crate::sites)
//   apiPrefix 백엔드 경로가 다를 때 /api 를 바꿀 접두사
// 설문(survey.audeniq.com)은 베타 전까지 D1 Worker 그대로라 여기 없다.
export const APPS = {
  studio: {
    aliases: ['studio.audeniq.com'],
    dir: 'web/studio/public',
    headers: 'web/studio/public/_headers',
    function: 'studio.js',
    api: ['/api/*', '/ready'],
    site: 'studio',
  },
  admin: {
    aliases: ['admin.audeniq.com'],
    dir: 'web/admin/dist',
    headers: 'web/admin/public/_headers',
    function: 'admin.js',
    api: ['/api/*'],
    site: 'admin',
  },
  landing: {
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

export const stackName = app => `audeniq-${app}-ui`;

/** Cloudflare `_headers` → [{ path, headers: [[name, value]] }] */
export function parseHeaders(text) {
  const rules = [];
  for (const line of text.split('\n')) {
    if (!line.trim() || line.trim().startsWith('#')) continue;
    if (!/^\s/.test(line)) { rules.push({ path: line.trim(), headers: [] }); continue; }
    const at = line.indexOf(':');
    if (at < 0 || !rules.length) throw new Error(`_headers: 해석할 수 없는 줄: ${line}`);
    rules.at(-1).headers.push([line.slice(0, at).trim(), line.slice(at + 1).trim()]);
  }
  return rules;
}

/**
 * _headers의 Cache-Control 규칙 → S3 업로드 그룹 (aws s3 sync --include 패턴, 먼저 올릴 것부터).
 * "/"·"/index.html" 같은 HTML 진입점은 맨 마지막에 올려, 새 HTML이 아직 없는 에셋을 가리키지 않게 한다.
 */
export function cacheGroups(rules) {
  const groups = [];
  for (const { path, headers } of rules) {
    const value = headers.find(([name]) => name.toLowerCase() === 'cache-control')?.[1];
    if (!value || path === '/*') continue;
    const pattern = path === '/' ? 'index.html' : path.replace(/^\//, '');
    if (pattern.endsWith('.html')) continue;
    groups.push({ include: pattern, cacheControl: value });
  }
  return groups;
}
