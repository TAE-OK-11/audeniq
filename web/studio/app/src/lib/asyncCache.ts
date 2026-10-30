// 화면 데이터 캐시 — 한 번 본 화면에 다시 들어오면 마지막으로 받은 데이터를 바로 보여 주고(자리표시 없이),
// 뒤에서 새로 받아 바꾼다(stale-while-revalidate). useAsync에 key를 준 곳만 쓴다.
// 다른 계정·작업 공간의 데이터가 섞이지 않게 작업 공간이 바뀌면(로그인·로그아웃 포함) 모두 비우고,
// 무엇이든 바꾸는 요청(GET이 아닌 요청)이 성공해도 모두 비운다 — 방금 바꾼 것이 옛 모습으로 잠깐 보이지 않게.

const cache = new Map<string, unknown>();
/** 같은 key를 쓰는 다른 화면 조각들 — 한 곳이 새로 받으면 모두 같은 데이터를 보이게 */
const listeners = new Map<string, Set<() => void>>();
/** 비운 횟수 — 비운 뒤 다시 받아야 하는 곳(왼쪽 메뉴의 개수)이 이것을 보고 새로 받는다 */
let clears = 0;
const clearListeners = new Set<() => void>();
/** 받은 시각 — 방금 받은 데이터는 다시 받지 않는다 */
const stamp = new Map<string, number>();
/** 이만큼 안에 받은 데이터는 새것으로 본다 — 머리글(바로 뜨는 코드)이 받은 목록을 뒤늦게 뜬 화면이 곧바로 또 받지 않게 */
export const FRESH_MS = 1500;

export function cacheGet<T>(key: string): T | undefined {
  return cache.get(key) as T | undefined;
}

export function cacheHas(key: string): boolean {
  return cache.has(key);
}

export function cacheSet<T>(key: string, value: T): void {
  cache.set(key, value);
  stamp.set(key, Date.now());
  listeners.get(key)?.forEach(fn => fn());
}

/** key의 데이터가 새로 들어오면 알려 준다. 돌려준 함수로 그만 듣는다 */
export function cacheSubscribe(key: string, fn: () => void): () => void {
  let set = listeners.get(key);
  if (!set) listeners.set(key, set = new Set());
  set.add(fn);
  return () => { set.delete(fn); if (!set.size) listeners.delete(key); };
}

export function cacheClears(): number {
  return clears;
}

export function onCacheClear(fn: () => void): () => void {
  clearListeners.add(fn);
  return () => { clearListeners.delete(fn); };
}

/** 받은 지 얼마나 됐는지(ms) — 없으면 Infinity */
export function cacheAge(key: string): number {
  const t = stamp.get(key);
  return t === undefined ? Infinity : Date.now() - t;
}

/** 방금(FRESH_MS 안에) 받은 데이터인지 */
export function cacheFresh(key: string): boolean {
  const t = stamp.get(key);
  return t !== undefined && Date.now() - t < FRESH_MS;
}

export function clearAsyncCache(): void {
  cache.clear();
  stamp.clear();
  clears += 1;
  clearListeners.forEach(fn => fn());
}
