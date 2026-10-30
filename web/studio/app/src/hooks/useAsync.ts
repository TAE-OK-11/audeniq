import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { errorMessage } from '../api/errors';
import { orgId } from '../api/http';
import { cacheClears, cacheFresh, cacheGet, cacheHas, cacheSet, cacheSubscribe, onCacheClear } from '../lib/asyncCache';

export interface AsyncState<T> {
  data: T | undefined;
  error: string;
  loading: boolean;
  reload: () => void;
}

/**
 * 비동기 로더 훅 — 언마운트/재요청 시 이전 응답을 무시해 경쟁 상태를 막는다.
 * deps가 바뀌거나 reload()를 부르면 다시 불러온다.
 * key를 주면 받은 데이터를 기억해 두었다가, 같은 key로 다시 들어오면 그 데이터를 바로 보여 주고(loading 없이)
 * 뒤에서 새로 받아 바꾼다. 새로 받기가 실패해도 보이던 데이터는 그대로 둔다. 방금 받은 데이터면 다시 받지 않는다
 * (reload()는 늘 새로 받는다). (lib/asyncCache)
 */
// key마다 마지막으로 시작한 요청 번호 — 먼저 보낸 요청이 늦게 끝나도 캐시(와 같은 key를 보는 화면)를 옛 데이터로 덮지 않게
const latest = new Map<string, number>();
let seq = 0;

export function useAsync<T>(fn: () => Promise<T>, deps: unknown[], opts: { key?: string } = {}): AsyncState<T> {
  // 작업 공간마다 따로 — 다른 작업 공간의 데이터를 잠깐이라도 보여 주지 않게
  const key = opts.key ? `${orgId()}|${opts.key}` : '';
  const [data, setData] = useState<T | undefined>(() => (key ? cacheGet<T>(key) : undefined));
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(() => !(key && cacheHas(key)));
  const [tick, setTick] = useState(0);
  const lastTick = useRef(tick);
  const fnRef = useRef(fn);
  fnRef.current = fn;

  useEffect(() => {
    let alive = true;
    const cached = key && cacheHas(key);
    const forced = tick !== lastTick.current;
    lastTick.current = tick;
    if (cached) {
      setData(cacheGet<T>(key));
      setLoading(false);
      if (!forced && cacheFresh(key)) return () => { alive = false; };
    } else {
      setLoading(true);
    }
    setError('');
    const mine = ++seq;
    if (key) latest.set(key, mine);
    fnRef.current()
      .then(v => {
        // 더 새 요청이 있으면 캐시는 그 응답에 맡긴다 (이 화면이 다시 부른 경우라면 alive가 이미 false)
        if (key && latest.get(key) === mine) cacheSet(key, v);
        if (alive) setData(v);
      })
      .catch(e => { if (alive && !cached) setError(errorMessage(e)); })
      .finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, [...deps, key, tick]);

  // 같은 key를 쓰는 다른 곳이 새로 받으면 이곳도 그 데이터로 (예: 발매 목록 화면이 받은 목록 → 왼쪽 메뉴 개수)
  useEffect(() => {
    if (!key) return;
    return cacheSubscribe(key, () => {
      setData(cacheGet<T>(key));
      setLoading(false);
      setError('');
    });
  }, [key]);

  const reload = useCallback(() => setTick(t => t + 1), []);
  return { data, error, loading, reload };
}

/** 화면 데이터 캐시를 비운 횟수 — 무언가 바뀐 뒤(GET이 아닌 요청 성공·작업 공간 전환) 다시 받을 곳의 deps에 넣는다 */
export function useCacheClears(): number {
  return useSyncExternalStore(onCacheClear, cacheClears, cacheClears);
}
