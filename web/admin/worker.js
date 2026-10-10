/**
 * AUDENIQ ADMIN Worker — 화면 서빙과 백엔드 전달만 한다
 * - 정적 에셋(React 빌드 dist/)은 Workers Static Assets가 서빙 (없는 화면 경로는 index.html로 SPA 폴백)
 * - /api/* 는 백엔드로 x-audeniq-site: admin 을 붙여 넘긴다. 관리자 콘솔이 쓰는 경로만 열고,
 *   쓰기는 관리자 주소(ADMIN_ORIGIN)에서 온 것만 받는 규칙은 백엔드가 한다 (crates/core/src/sites.rs)
 *
 * 설정 (wrangler.jsonc vars / secrets)
 *   BACKEND_URL         백엔드 주소 (기본 https://api-origin.audeniq.com)
 *   EDGE_SERVICE_SECRET 백엔드 서비스 비밀 (wrangler secret put EDGE_SERVICE_SECRET)
 */
import { proxyToBackend, requireHttps, secureResponse, serveAssets } from '../shared/transport.js';

export async function proxy(request, env) {
  const out = await proxyToBackend(request, env, { site: 'admin' });
  out.headers.set('Cache-Control', 'no-store');
  out.headers.set('X-Content-Type-Options', 'nosniff');
  out.headers.delete('Access-Control-Allow-Origin');
  out.headers.delete('Access-Control-Allow-Credentials');
  return out;
}

export default {
  async fetch(request, env) {
    const refused = requireHttps(request);
    if (refused) return refused;
    const url = new URL(request.url);
    if (url.pathname.startsWith('/api/')) return secureResponse(await proxy(request, env));
    return secureResponse(await serveAssets(request, env));
  },
};
