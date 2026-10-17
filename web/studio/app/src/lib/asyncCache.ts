// 화면 데이터 캐시 — 한 번 본 화면에 다시 들어오면 마지막으로 받은 데이터를 바로 보여 주고(자리표시 없이),
// 뒤에서 새로 받아 바꾼다(stale-while-revalidate). useAsync에 key를 준 곳만 쓴다.
// 다른 계정·작업 공간의 데이터가 섞이지 않게 작업 공간이 바뀌면(로그인·로그아웃 포함) 모두 비우고,
// 무엇이든 바꾸는 요청(GET이 아닌 요청)이 성공해도 모두 비운다 — 방금 바꾼 것이 옛 모습으로 잠깐 보이지 않게.

const cache = new Map<string, unknown>();

export function cacheGet<T>(key: string): T | undefined {
  return cache.get(key) as T | undefined;
}

export function cacheHas(key: string): boolean {
  return cache.has(key);
}

export function cacheSet<T>(key: string, value: T): void {
  cache.set(key, value);
}

export function clearAsyncCache(): void {
  cache.clear();
}
