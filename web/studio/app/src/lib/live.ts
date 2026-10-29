// 실시간 연결 — 서버가 바뀐 것만 알려 주는 SSE(`/api/orgs/{org}/live`, `/api/staff/live`).
// 이벤트에는 고객 내용이 없고, 받으면 기존 API로 해당 목록만 다시 받는다.
// EventSource가 없거나 연결이 계속 실패하면(프록시·네트워크) 조용히 짧은 주기 새로 고침으로 바꿨다가 다시 시도한다.
import { API_BASE } from '../api/http';

type Handler = (data: Record<string, unknown>) => void;

export interface LiveOptions {
  /** 이벤트 이름 → 처리 */
  on: Record<string, Handler>;
  /** 연결이 (다시) 열렸을 때와 대체 주기마다 — 놓친 변경을 따라잡는다 */
  resync: () => void;
  /** 실시간 연결이 안 될 때 새로 고침 주기 */
  fallbackMs?: number;
}

const MAX_FAILS = 3;
const RETRY_LIVE_MS = 120_000;
/** 서버의 20초 keep-alive는 SSE 주석이라 EventSource가 화면에 알려 주지 않는다 — 대신 연결이 끊겼을 법한 순간에 직접 다시 붙는다:
 *  인터넷 복귀, 오래(1분) 다른 탭·앱에 있다 돌아옴, 기기가 잠들었다 깸(타이머가 크게 밀림). 다시 붙으면 'ready'가 놓친 변경을 따라잡는다 */
const AWAY_MS = 60_000;
const TICK_MS = 15_000;

/** 연결을 닫는 함수 — connected()로 지금 실시간 연결이 살아 있는지 알 수 있다 */
export type LiveHandle = (() => void) & { connected: () => boolean };

export function openLive(path: string, opts: LiveOptions): LiveHandle {
  const fallbackMs = opts.fallbackMs ?? 15_000;
  let es: EventSource | null = null;
  let fails = 0;
  let stopped = false;
  let poll = 0;
  let retry = 0;

  const startFallback = () => {
    if (poll || stopped) return;
    poll = window.setInterval(() => { if (document.visibilityState === 'visible') opts.resync(); }, fallbackMs);
    retry = window.setTimeout(() => { stopFallback(); connect(); }, RETRY_LIVE_MS);
  };
  const stopFallback = () => {
    window.clearInterval(poll); poll = 0;
    window.clearTimeout(retry); retry = 0;
  };

  const connect = () => {
    if (stopped) return;
    if (typeof EventSource === 'undefined') { startFallback(); return; }
    es = new EventSource(`${API_BASE}${path}`, { withCredentials: true });
    es.addEventListener('ready', () => { fails = 0; opts.resync(); });
    for (const [name, fn] of Object.entries(opts.on)) {
      es.addEventListener(name, e => {
        try { fn(JSON.parse((e as MessageEvent).data || '{}')); } catch { fn({}); }
      });
    }
    // 세션이 끝났거나 권한이 사라졌다 — 다시 붙지 않는다 (다음 요청에서 로그인 화면으로 간다)
    es.addEventListener('end', () => { es?.close(); es = null; });
    es.onerror = () => {
      fails += 1;
      if (fails >= MAX_FAILS) {
        es?.close(); es = null;
        startFallback();
      }
      // 그 전에는 EventSource가 스스로 다시 붙는다 (서버가 10분마다 연결을 새로 하게 한다)
    };
  };

  const reconnect = () => {
    if (stopped || poll) return; // 대체 주기 새로 고침 중이면 그쪽이 다시 시도한다
    es?.close(); es = null;
    fails = 0;
    connect();
  };
  let hiddenAt = 0;
  const onVisible = () => {
    if (document.visibilityState === 'hidden') { hiddenAt = Date.now(); return; }
    if (hiddenAt && Date.now() - hiddenAt > AWAY_MS) reconnect();
    hiddenAt = 0;
  };
  let lastTick = Date.now();
  const tick = window.setInterval(() => {
    const now = Date.now();
    if (now - lastTick > TICK_MS * 3) reconnect(); // 잠들었다 깸
    lastTick = now;
  }, TICK_MS);
  window.addEventListener('online', reconnect);
  document.addEventListener('visibilitychange', onVisible);

  connect();
  const stop = () => {
    stopped = true;
    es?.close(); es = null;
    stopFallback();
    window.clearInterval(tick);
    window.removeEventListener('online', reconnect);
    document.removeEventListener('visibilitychange', onVisible);
  };
  return Object.assign(stop, { connected: () => es?.readyState === 1 /* EventSource.OPEN */ });
}
