/**
 * AUDENIQ STUDIO Worker — 프록시만 한다 (화면 파일도 백엔드가 서빙: crates/core/src/studio_static.rs)
 * 모든 요청을 백엔드로 넘기면서 헤더만 다듬는다 (shared/transport.ts backendHeaders):
 * - 브라우저가 보낸 x-audeniq-* 헤더는 버린다 (필요한 헤더만 골라 보낸다)
 * - 실제 사용자 IP(Cloudflare CF-Connecting-IP)를 x-audeniq-client-ip로 넣는다
 * - 서비스 식별 헤더 x-audeniq-service(비밀)·x-audeniq-site(studio)를 넣는다
 * SPA 폴백·리다이렉트·캐시·보안 헤더(_headers)는 백엔드가 한다. 그래서 같은 일을 AWS CloudFront
 * Function(viewer-request, 헤더만)과 origin custom header로 옮길 수 있다 (deploy/STUDIO_SERVING.md).
 *
 * 설정: BACKEND_URL(기본 https://api-origin.audeniq.com), EDGE_SERVICE_SECRET(secret)
 */
import { proxyToBackend, requireHttps, secureResponse } from '../shared/transport.ts';
import type { WorkerRequest } from '../shared/transport.ts';
import type { ExecutionContext, TransportEnv } from '../shared/worker-types.ts';

/** 정적 에셋 바인딩 없이 백엔드 주소·비밀만 */
export type StudioEnv = Pick<TransportEnv, 'BACKEND_URL' | 'EDGE_SERVICE_SECRET'>;

export default {
  async fetch(request: WorkerRequest, env: StudioEnv, _ctx?: ExecutionContext): Promise<Response> {
    const refused = requireHttps(request);
    if (refused) return refused;
    const out = await proxyToBackend(request, env, { site: 'studio' });
    if (new URL(request.url).pathname.startsWith('/api/')) {
      out.headers.set('Access-Control-Allow-Origin', 'https://studio.audeniq.com');
      out.headers.set('Access-Control-Allow-Credentials', 'true');
    }
    return secureResponse(out);
  },
};
