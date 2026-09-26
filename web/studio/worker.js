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

    // --- 그 외는 정적 에셋으로 폴백 ---
    return env.ASSETS.fetch(request);
  },
};
