/**
 * AUDENIQ STUDIO Worker
 * - 정적 에셋은 Workers Static Assets가 서빙
 * - /api/notices, /api/events는 D1(CONTENT_DB)에서 서빙
 */

const KST_OFFSET = 9 * 60 * 60 * 1000;

function kstToday() {
  const now = new Date(Date.now() + KST_OFFSET);
  return now.toISOString().slice(0, 10);
}

function eventStatus(startsOn, endsOn) {
  const today = kstToday();
  if (today < startsOn) return 'upcoming';
  if (endsOn && today > endsOn) return 'ended';
  return 'ongoing';
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);

    // --- /api/notices ---
    if (url.pathname === '/api/notices' && request.method === 'GET') {
      try {
        const { results } = await env.CONTENT_DB.prepare(
          `SELECT id, title, body, pinned, published_at
           FROM notices
           WHERE deleted_at IS NULL AND published_at <= strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
           ORDER BY pinned DESC, published_at DESC`
        ).all();
        return Response.json({ items: results ?? [] }, {
          headers: { 'Cache-Control': 'public, max-age=60' },
        });
      } catch (e) {
        return Response.json({ items: [], error: 'D1_ERROR' }, { status: 500 });
      }
    }

    // --- /api/events ---
    if (url.pathname === '/api/events' && request.method === 'GET') {
      try {
        const { results } = await env.CONTENT_DB.prepare(
          `SELECT id, title, summary, body, place, starts_on, ends_on, link_url, published_at
           FROM events
           WHERE deleted_at IS NULL AND published_at <= strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
           ORDER BY starts_on DESC`
        ).all();
        const items = (results ?? []).map((ev) => ({
          ...ev,
          status: eventStatus(ev.starts_on, ev.ends_on),
        }));
        return Response.json({ items }, {
          headers: { 'Cache-Control': 'public, max-age=60' },
        });
      } catch (e) {
        return Response.json({ items: [], error: 'D1_ERROR' }, { status: 500 });
      }
    }

    // --- 그 외는 정적 에셋으로 폴백 (SPA: 없는 경로는 index.html) ---
    const assetRes = await env.ASSETS.fetch(request);
    if (assetRes.status === 404) {
      // 에셋·API가 아닌 경로 + 확장자 없음 → SPA 폴백
      const isAsset = /\.[a-z0-9]+$/i.test(url.pathname);
      if (!isAsset && !url.pathname.startsWith('/api/')) {
        const indexReq = new Request(new URL('/', request.url), request);
        const indexRes = await env.ASSETS.fetch(indexReq);
        if (indexRes.ok) {
          return new Response(indexRes.body, {
            status: 200,
            headers: {
              'Content-Type': 'text/html;charset=utf-8',
              'Cache-Control': 'no-cache',
            },
          });
        }
      }
    }
    return assetRes;
  },
};
