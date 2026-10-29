// 실사용 성능 지표(Web Vitals) — 라이브러리 없이 브라우저의 PerformanceObserver로 잰다.
// LCP(가장 큰 내용이 그려진 때)·INP(누른 뒤 화면이 반응하기까지 가장 느렸던 때)·CLS(화면 밀림)·FCP·TTFB.
// 화면을 떠날 때(숨김) 한 번 모아서:
// - VITE_RUM_ENDPOINT가 있으면 그 주소로 sendBeacon (같은 출처 /api/… 권장 — 서버 수집 엔드포인트가 생기면 켠다)
// - localStorage 'aq.perf' = '1'이면 콘솔 표로 (점검용)
// 둘 다 없으면 아무것도 보내지 않는다.
const ENDPOINT = (import.meta.env.VITE_RUM_ENDPOINT ?? '').trim();

export interface Vitals { lcp?: number; inp?: number; cls?: number; fcp?: number; ttfb?: number }

type Entry = PerformanceEntry & { value?: number; hadRecentInput?: boolean; interactionId?: number; processingStart?: number };

function observe(type: string, cb: (list: Entry[]) => void, opts: Record<string, unknown> = {}) {
  try {
    if (!PerformanceObserver.supportedEntryTypes?.includes(type)) return;
    new PerformanceObserver(l => cb(l.getEntries() as Entry[])).observe({ type, buffered: true, ...opts } as PerformanceObserverInit);
  } catch { /* 지원하지 않는 브라우저 */ }
}

let started = false;
/** 측정을 시작한다 (buffered라 첫 화면이 그려진 뒤 불러도 앞선 값까지 받는다) */
export function startVitals(): Vitals | null {
  const debug = (() => { try { return localStorage.getItem('aq.perf') === '1'; } catch { return false; } })();
  if (started || (!ENDPOINT && !debug) || typeof PerformanceObserver === 'undefined') return null;
  started = true;
  const v: Vitals = {};
  observe('largest-contentful-paint', es => { const last = es.at(-1); if (last) v.lcp = Math.round(last.startTime); });
  observe('paint', es => { const f = es.find(e => e.name === 'first-contentful-paint'); if (f) v.fcp = Math.round(f.startTime); });
  // CLS — 1초 간격·5초 창으로 묶은 밀림 중 가장 큰 묶음 (Web Vitals 정의)
  let win = 0, winStart = 0, winLast = 0;
  observe('layout-shift', es => {
    for (const e of es) {
      if (e.hadRecentInput) continue;
      const t = e.startTime;
      if (win && t - winLast < 1000 && t - winStart < 5000) win += e.value ?? 0;
      else { win = e.value ?? 0; winStart = t; }
      winLast = t;
      v.cls = Math.max(v.cls ?? 0, Math.round(win * 1000) / 1000);
    }
  });
  // INP — 상호작용마다 가장 긴 반응 시간 중 최댓값 (상호작용이 적은 화면 기준의 근사)
  observe('event', es => {
    for (const e of es) if (e.interactionId) v.inp = Math.max(v.inp ?? 0, Math.round(e.duration));
  }, { durationThreshold: 40 });
  const nav = performance.getEntriesByType('navigation')[0] as PerformanceNavigationTiming | undefined;
  if (nav) v.ttfb = Math.round(nav.responseStart);

  let sent = false;
  const flush = (leaving = false) => {
    if (sent || (!leaving && document.visibilityState !== 'hidden')) return;
    sent = true;
    const payload = { ...v, path: location.pathname.replace(/\/[0-9a-f-]{8,}|\/r\d+|\/[A-Za-z0-9]{8}$/g, '/:id'), w: innerWidth, conn: (navigator as Navigator & { connection?: { effectiveType?: string } }).connection?.effectiveType ?? '' };
    if (debug) console.table(payload);
    if (ENDPOINT) { try { navigator.sendBeacon(ENDPOINT, new Blob([JSON.stringify(payload)], { type: 'application/json' })); } catch { /* 보내지 못해도 화면에는 영향 없음 */ } }
  };
  document.addEventListener('visibilitychange', () => flush());
  addEventListener('pagehide', () => flush(true));
  return v;
}
