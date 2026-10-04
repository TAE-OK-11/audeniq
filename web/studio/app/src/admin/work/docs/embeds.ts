// 문서 속 AUDENIQ 블록(발매·QC·DSP·문의·카드…)의 실시간 값. 블록마다 요청하지 않고, 같은 순간에 그려지는
// 블록들의 키를 모아 `/embeds` 한 번으로 불러온다(N+1 없음). 받은 값은 잠깐(ttl) 기억하고, 새로고침은 키 단위.
import { useEffect, useState, useSyncExternalStore } from 'react';
import type { EmbedData } from '../types';

type Fetch = (keys: string[]) => Promise<Record<string, EmbedData>>;
interface Entry { data?: EmbedData; error?: string; at: number; loading: boolean }

const MAX_BATCH = 60;

export class EmbedStore {
  private entries = new Map<string, Entry>();
  private queue = new Set<string>();
  private timer: ReturnType<typeof setTimeout> | null = null;
  private listeners = new Set<() => void>();
  private version = 0;

  constructor(private fetcher: Fetch, private ttlMs = 60_000, private delayMs = 16) {}

  subscribe = (fn: () => void) => { this.listeners.add(fn); return () => { this.listeners.delete(fn); }; };
  getVersion = () => this.version;
  get(key: string): Entry | undefined { return this.entries.get(key); }

  private emit() { this.version += 1; for (const l of this.listeners) l(); }

  /** 필요한 키를 알린다 — 없거나 오래됐으면 다음 묶음에 넣는다 */
  want(key: string, force = false) {
    const e = this.entries.get(key);
    if (!force && e && (e.loading || Date.now() - e.at < this.ttlMs)) return;
    this.entries.set(key, { ...e, at: e?.at ?? 0, loading: true });
    this.queue.add(key);
    if (!this.timer) this.timer = setTimeout(() => void this.flush(), this.delayMs);
  }

  async flush() {
    this.timer = null;
    const keys = [...this.queue];
    this.queue.clear();
    for (let i = 0; i < keys.length; i += MAX_BATCH) {
      const batch = keys.slice(i, i + MAX_BATCH);
      try {
        const items = await this.fetcher(batch);
        const now = Date.now();
        for (const k of batch) this.entries.set(k, { data: items[k] ?? { kind: '', id: k, view: 'summary', missing: true }, at: now, loading: false });
      } catch (err) {
        const msg = err instanceof Error ? err.message : '불러오지 못했어요.';
        for (const k of batch) this.entries.set(k, { ...this.entries.get(k), error: msg, at: 0, loading: false });
      }
      this.emit();
    }
  }
}

/** 블록 하나의 값 — 같은 화면의 블록들은 한 요청으로 묶인다 */
export function useEmbed(store: EmbedStore, key: string | null): { data?: EmbedData; error?: string; loading: boolean; refresh: () => void } {
  useSyncExternalStore(store.subscribe, store.getVersion);
  const [, setTick] = useState(0);
  useEffect(() => { if (key) store.want(key); }, [store, key]);
  const e = key ? store.get(key) : undefined;
  return {
    data: e?.data, error: e?.error, loading: !e || e.loading,
    refresh: () => { if (key) { store.want(key, true); setTick(t => t + 1); } },
  };
}
