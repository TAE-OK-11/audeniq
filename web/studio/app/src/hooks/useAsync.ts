import { useCallback, useEffect, useRef, useState } from 'react';
import { errorMessage } from '../api/errors';
import { orgId } from '../api/http';
import { cacheGet, cacheHas, cacheSet } from '../lib/asyncCache';

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
 * 뒤에서 새로 받아 바꾼다. 새로 받기가 실패해도 보이던 데이터는 그대로 둔다. (lib/asyncCache)
 */
export function useAsync<T>(fn: () => Promise<T>, deps: unknown[], opts: { key?: string } = {}): AsyncState<T> {
  // 작업 공간마다 따로 — 다른 작업 공간의 데이터를 잠깐이라도 보여 주지 않게
  const key = opts.key ? `${orgId()}|${opts.key}` : '';
  const [data, setData] = useState<T | undefined>(() => (key ? cacheGet<T>(key) : undefined));
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(() => !(key && cacheHas(key)));
  const [tick, setTick] = useState(0);
  const fnRef = useRef(fn);
  fnRef.current = fn;

  useEffect(() => {
    let alive = true;
    const cached = key && cacheHas(key);
    if (cached) {
      setData(cacheGet<T>(key));
      setLoading(false);
    } else {
      setLoading(true);
    }
    setError('');
    fnRef.current()
      .then(v => { if (key) cacheSet(key, v); if (alive) setData(v); })
      .catch(e => { if (alive && !cached) setError(errorMessage(e)); })
      .finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, [...deps, key, tick]);

  const reload = useCallback(() => setTick(t => t + 1), []);
  return { data, error, loading, reload };
}
