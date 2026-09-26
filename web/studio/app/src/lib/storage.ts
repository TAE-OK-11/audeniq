// localStorage 안전 래퍼 — 사파리 프라이빗 모드, 용량 초과, 손상된 JSON에도 앱이 죽지 않도록 한다.
const PREFIX = 'aq.studio.v2.';

export function readJSON<T>(key: string, fallback: T): T {
  try {
    const raw = window.localStorage.getItem(PREFIX + key);
    if (raw == null) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

/** 저장 실패(용량 초과 등) 시 앱 전체에 알리는 이벤트 이름 */
export const STORAGE_FAIL_EVENT = 'aq:storage-fail';

export function writeJSON(key: string, value: unknown): boolean {
  try {
    window.localStorage.setItem(PREFIX + key, JSON.stringify(value));
    return true;
  } catch (e) {
    // 조용히 실패하면 새로고침 후 데이터가 사라진 걸 뒤늦게 알게 되므로 사용자에게 알린다
    const quota = e instanceof DOMException && (e.name === 'QuotaExceededError' || e.code === 22);
    window.dispatchEvent(new CustomEvent(STORAGE_FAIL_EVENT, { detail: { key, quota } }));
    return false;
  }
}

export function removeKey(key: string): void {
  try {
    window.localStorage.removeItem(PREFIX + key);
  } catch {
    /* 저장소 접근 불가 — 무시 */
  }
}

/** 이 앱이 저장한 모든 키 삭제 (로그아웃·데모 초기화용) */
export function clearAll(): void {
  try {
    const keys: string[] = [];
    for (let i = 0; i < window.localStorage.length; i++) {
      const k = window.localStorage.key(i);
      if (k && k.startsWith(PREFIX)) keys.push(k);
    }
    keys.forEach(k => window.localStorage.removeItem(k));
  } catch {
    /* 무시 */
  }
}
