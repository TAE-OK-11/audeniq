import { useCallback, useEffect, useRef, useState } from 'react';
import { errorMessage } from '../api/errors';

export interface AsyncState<T> {
  data: T | undefined;
  error: string;
  loading: boolean;
  reload: () => void;
}

/**
 * 비동기 로더 훅 — 언마운트/재요청 시 이전 응답을 무시해 경쟁 상태를 막는다.
 * deps가 바뀌거나 reload()를 부르면 다시 불러온다.
 */
export function useAsync<T>(fn: () => Promise<T>, deps: unknown[]): AsyncState<T> {
  const [data, setData] = useState<T>();
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);
  const [tick, setTick] = useState(0);
  const fnRef = useRef(fn);
  fnRef.current = fn;

  useEffect(() => {
    let alive = true;
    setLoading(true);
    setError('');
    fnRef.current()
      .then(v => { if (alive) setData(v); })
      .catch(e => { if (alive) setError(errorMessage(e)); })
      .finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, [...deps, tick]);

  const reload = useCallback(() => setTick(t => t + 1), []);
  return { data, error, loading, reload };
}
