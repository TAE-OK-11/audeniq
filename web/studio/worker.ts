/**
 * AUDENIQ STUDIO Worker — 화면 서빙과 백엔드 전달만 한다
 * - 정적 에셋(React 빌드)은 Workers Static Assets가 서빙 (없는 화면 경로는 index.html로 SPA 폴백)
 * - /api/*, /ready 는 백엔드로 (터널 → api, shared/transport.ts proxyToBackend)
 * - AI 어시스턴트 OAuth 발견 문서(/.well-known/oauth-*)도 백엔드로 (docs/MCP.md)
 *   공지·이벤트·서버 점검(/api/notices, /api/events, /api/status, /api/content/*), 점검 중 503 차단,
 *   비상 스위치(MAINTENANCE_MODE)는 모두 백엔드가 한다 (crates/core/src/site_content.rs)
 *
 * 설정: BACKEND_URL(기본 https://api-origin.audeniq.com), EDGE_SERVICE_SECRET(secret)
 */
import { MCP_DISCOVERY_PATHS, proxyToBackend, requireHttps, secureResponse, serveAssets } from '../shared/transport.ts';
import type { WorkerRequest } from '../shared/transport.ts';
import type { ExecutionContext, TransportEnv } from '../shared/worker-types.ts';

async function proxy(request: WorkerRequest, env: TransportEnv): Promise<Response> {
  const out = await proxyToBackend(request, env, { site: 'studio' });
  out.headers.set('Access-Control-Allow-Origin', 'https://studio.audeniq.com');
  out.headers.set('Access-Control-Allow-Credentials', 'true');
  return out;
}

const studioWorker = {
  async fetch(request: WorkerRequest, env: TransportEnv, _ctx?: ExecutionContext): Promise<Response> {
    const url = new URL(request.url);
    if (url.pathname.startsWith('/api/') || url.pathname === '/ready') return proxy(request, env);
    if (MCP_DISCOVERY_PATHS.has(url.pathname) && ['GET', 'HEAD'].includes(request.method)) {
      return proxyToBackend(request, env, { site: 'studio' });
    }
    // 그 외는 정적 에셋 (없는 화면 경로는 index.html로 SPA 폴백)
    const assetRes = await serveAssets(request, env);
    if (assetRes.status === 404) {
      const isAsset = /\.[a-z0-9]+$/i.test(url.pathname);
      if (!isAsset) {
        const indexRes = await serveAssets(new Request(new URL('/', request.url), request), env);
        if (indexRes.ok) {
          // 에셋 응답의 보안 헤더(_headers의 CSP 등)는 그대로 두고 캐시만 끈다
          const headers = new Headers(indexRes.headers);
          headers.set('Cache-Control', 'no-cache');
          return new Response(indexRes.body, { status: 200, headers });
        }
      }
    }
    return assetRes;
  },
};

export default {
  async fetch(request: WorkerRequest, env: TransportEnv, ctx?: ExecutionContext): Promise<Response> {
    const refused = requireHttps(request);
    if (refused) return refused;
    return secureResponse(await studioWorker.fetch(request, env, ctx));
  },
};
