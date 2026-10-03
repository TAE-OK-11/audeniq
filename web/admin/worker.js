/**
 * AUDENIQ ADMIN Worker
 * - 정적 에셋(React 빌드 dist/)은 Workers Static Assets가 서빙 (없는 화면 경로는 index.html로 SPA 폴백)
 * - /api/* 는 관리자 콘솔이 쓰는 경로만 백엔드로 전달한다
 *     로그인·세션: /api/auth/login, /api/auth/logout, /api/auth/csrf, /api/me
 *     스태프 API:  /api/staff/*
 *
 * 출처(Origin) 처리
 *   백엔드는 APP_ORIGIN(스튜디오 주소) 하나만 받는다. 이 Worker가 먼저 요청 Origin이 관리자 주소와 같은지
 *   확인한 뒤(교차 출처 차단), 백엔드에는 BACKEND_APP_ORIGIN으로 바꿔 전달한다. CSRF 토큰·세션 쿠키 검사는
 *   백엔드가 그대로 한다. 세션 쿠키(__Host-, SameSite=Strict)는 관리자 도메인에만 저장된다.
 *
 * 설정 (wrangler.jsonc vars / secrets)
 *   BACKEND_URL         백엔드 주소 (기본 https://api-origin.audeniq.com)
 *   BACKEND_APP_ORIGIN  백엔드 APP_ORIGIN (기본 https://studio.audeniq.com)
 *   EDGE_SERVICE_SECRET 백엔드 서비스 비밀 (wrangler secret put EDGE_SERVICE_SECRET)
 */

import { route as contentRoute, handleContent } from '../studio/worker.js';
import { backendOrigin, clientEncoding, passThroughEncoding, readLimitedBody, requireHttps, secureResponse, serveAssets } from '../shared/transport.js';

const ALLOWED = [/^\/api\/auth\/(login|logout|csrf)$/, /^\/api\/me$/, /^\/api\/staff(\/.*)?$/];
const FORWARD_HEADERS = ['cookie', 'content-type', 'accept', 'x-csrf-token'];
const MAX_BODY = 64 * 1024;

const error = (status, code) => Response.json(
  { error: { code } },
  { status, headers: { 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff' } },
);

export async function proxy(request, env, url) {
  const refused = requireHttps(request);
  if (refused) return refused;
  if (!ALLOWED.some(re => re.test(url.pathname))) return error(404, 'NOT_FOUND');
  const read = request.method === 'GET' || request.method === 'HEAD';
  if (!read && request.method !== 'POST') return error(405, 'METHOD_NOT_ALLOWED');
  // 상태 변경 요청은 이 관리자 페이지에서 온 것만 (다른 사이트의 폼·스크립트 차단)
  if (!read) {
    if (request.headers.get('origin') !== url.origin) return error(403, 'FORBIDDEN');
    const site = request.headers.get('sec-fetch-site');
    if (site && site !== 'same-origin') return error(403, 'FORBIDDEN');
  }

  const headers = new Headers();
  for (const name of FORWARD_HEADERS) {
    const v = request.headers.get(name);
    if (v) headers.set(name, v);
  }
  headers.set('accept-encoding', passThroughEncoding(clientEncoding(request)));
  headers.set('origin', env.BACKEND_APP_ORIGIN || 'https://studio.audeniq.com');
  headers.set('sec-fetch-site', 'same-origin');
  const ip = request.headers.get('cf-connecting-ip');
  if (ip) headers.set('x-audeniq-client-ip', ip);
  if (env.EDGE_SERVICE_SECRET) headers.set('x-audeniq-service', env.EDGE_SERVICE_SECRET);

  let body;
  if (!read) {
    try { body = await readLimitedBody(request, MAX_BODY); }
    catch (e) { return error(e instanceof RangeError ? 413 : 400, 'PAYLOAD_TOO_LARGE'); }
  }
  let res;
  try {
    res = await fetch(backendOrigin(env) + url.pathname + url.search, { method: request.method, headers, body, redirect: 'manual' });
  } catch {
    return error(502, 'BACKEND_UNAVAILABLE');
  }
  const out = new Headers(res.headers);
  out.set('Cache-Control', 'no-store');
  out.set('X-Content-Type-Options', 'nosniff');
  out.delete('Access-Control-Allow-Origin');
  out.delete('Access-Control-Allow-Credentials');
  return new Response(res.body, { status: res.status, headers: out,
    ...(out.has('content-encoding') ? { encodeBody: 'manual' } : {}) });
}

export default {
  async fetch(request, env) {
    const refused = requireHttps(request);
    if (refused) return refused;
    const url = new URL(request.url);
    const content = contentRoute(request.method, url.pathname);
    if (content) return secureResponse(await handleContent(request, env, content));
    if (url.pathname.startsWith('/api/')) return secureResponse(await proxy(request, env, url));
    return secureResponse(await serveAssets(request, env));
  },
};
