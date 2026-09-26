// 작은 외부 스토어 — useSyncExternalStore 기반, 선택적으로 localStorage에 영속화한다.
// 여러 탭에서 동시에 열려 있어도 storage 이벤트로 동기화된다.
import { useSyncExternalStore } from 'react';
import { readJSON, writeJSON } from './storage';

export interface Store<T> {
  get: () => T;
  set: (next: T | ((prev: T) => T)) => void;
  subscribe: (listener: () => void) => () => void;
  use: () => T;
  /** 초기값으로 되돌림 (영속 데이터도 덮어씀) */
  reset: () => void;
}

interface Options<T> {
  /** 영속화 키. 없으면 메모리에만 보관 */
  persist?: string;
  /** 저장 직전 변환 (예: File 같은 직렬화 불가 값 제거) */
  serialize?: (value: T) => unknown;
  /** 불러온 값 검증·보정. 형식이 맞지 않으면 fallback 반환 */
  revive?: (raw: unknown, fallback: T) => T;
}

export function createStore<T>(initial: T | (() => T), opts: Options<T> = {}): Store<T> {
  const makeInitial = () => (typeof initial === 'function' ? (initial as () => T)() : initial);
  const load = (): T => {
    const fallback = makeInitial();
    if (!opts.persist || typeof window === 'undefined') return fallback;
    const raw = readJSON<unknown>(opts.persist, undefined);
    if (raw === undefined) return fallback;
    try {
      return opts.revive ? opts.revive(raw, fallback) : (raw as T);
    } catch {
      return fallback;
    }
  };

  let state = load();
  const listeners = new Set<() => void>();
  const emit = () => listeners.forEach(l => l());

  const save = () => {
    if (!opts.persist) return;
    writeJSON(opts.persist, opts.serialize ? opts.serialize(state) : state);
  };

  if (opts.persist && typeof window !== 'undefined') {
    window.addEventListener('storage', e => {
      if (e.key && e.key.endsWith('.' + opts.persist)) {
        state = load();
        emit();
      }
    });
  }

  const store: Store<T> = {
    get: () => state,
    set: next => {
      const value = typeof next === 'function' ? (next as (prev: T) => T)(state) : next;
      if (Object.is(value, state)) return;
      state = value;
      save();
      emit();
    },
    subscribe: l => {
      listeners.add(l);
      return () => { listeners.delete(l); };
    },
    use: () => useSyncExternalStore(store.subscribe, store.get, store.get),
    reset: () => {
      state = makeInitial();
      save();
      emit();
    },
  };
  return store;
}

/** 간단한 고유 ID */
export function uid(prefix = ''): string {
  const rand = typeof crypto !== 'undefined' && 'randomUUID' in crypto
    ? crypto.randomUUID().replace(/-/g, '').slice(0, 12)
    : Math.random().toString(36).slice(2, 14);
  return prefix + rand;
}
