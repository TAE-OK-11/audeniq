// 자동 저장 — 입력을 막지 않고, 요청은 적게. 마지막 입력 후 잠깐(debounce) 쉬거나 오래 쓰는 중이면 최대 대기
// 시간마다 저장한다. 저장은 한 번에 하나만 보내고, 그사이 바뀐 내용은 끝난 뒤 이어서 보낸다.
// 네트워크 오류는 점점 길게 기다렸다 다시 시도하고(재시도 버튼으로 바로 보낼 수도 있다), 다른 사람이 먼저
// 고친 경우(409)는 덮어쓰지 않고 '충돌'로 멈춘다. 서버 버전(base_version)은 저장마다 갱신한다.
// React와 무관한 클래스로 두어 단위 테스트가 쉽다 (docs/autosave.test.ts).

export type SaveState = 'idle' | 'dirty' | 'saving' | 'saved' | 'error' | 'conflict';

export interface SaveResult { version: number }
export interface AutosaveOptions<T> {
  /** 저장: 바뀐 내용과 기준 버전 → 새 버전 */
  save: (value: T, baseVersion: number) => Promise<SaveResult>;
  version: number;
  debounceMs?: number;
  maxWaitMs?: number;
  /** 재시도 대기 (ms) — 마지막 값을 계속 쓴다 */
  retryMs?: number[];
  isConflict?: (e: unknown) => boolean;
  /** 테스트용 타이머·시계 */
  timers?: { set: (fn: () => void, ms: number) => unknown; clear: (h: unknown) => void; now?: () => number };
}

export interface AutosaveSnapshot { state: SaveState; version: number; savedAt: number | null; error: unknown; attempt: number }

export class Autosave<T> {
  private opts: Required<Omit<AutosaveOptions<T>, 'timers'>> & { timers: NonNullable<AutosaveOptions<T>['timers']> };
  private pending: { value: T } | null = null;
  private inflight: Promise<void> | null = null;
  private timer: unknown = null;
  private firstDirtyAt = 0;
  private attempt = 0;
  private listeners = new Set<(s: AutosaveSnapshot) => void>();
  private snap: AutosaveSnapshot;
  private disposed = false;

  constructor(o: AutosaveOptions<T>) {
    this.opts = {
      debounceMs: 1200, maxWaitMs: 8000, retryMs: [2000, 4000, 8000, 16000, 30000],
      isConflict: () => false,
      timers: { set: (fn, ms) => setTimeout(fn, ms), clear: h => clearTimeout(h as ReturnType<typeof setTimeout>) },
      ...o,
    };
    this.snap = { state: 'idle', version: o.version, savedAt: null, error: null, attempt: 0 };
  }

  get snapshot(): AutosaveSnapshot { return this.snap; }
  get version(): number { return this.snap.version; }
  /** 저장 안 된 변경이 있는지 (페이지를 떠날 때 묻는다) */
  get unsaved(): boolean { return this.pending != null || this.inflight != null; }

  subscribe(fn: (s: AutosaveSnapshot) => void): () => void {
    this.listeners.add(fn);
    return () => { this.listeners.delete(fn); };
  }

  private set(p: Partial<AutosaveSnapshot>) {
    this.snap = { ...this.snap, ...p };
    for (const l of this.listeners) l(this.snap);
  }

  private schedule(ms: number) {
    if (this.timer != null) this.opts.timers.clear(this.timer);
    this.timer = this.opts.timers.set(() => { this.timer = null; void this.run(); }, ms);
  }

  /** 내용이 바뀌었다 — 저장을 예약한다 */
  change(value: T) {
    if (this.disposed || this.snap.state === 'conflict') return;
    const now = (this.opts.timers.now ?? Date.now)();
    if (!this.pending) this.firstDirtyAt = now;
    this.pending = { value };
    if (this.snap.state !== 'saving' && this.snap.state !== 'error') this.set({ state: 'dirty' });
    if (this.inflight || this.snap.state === 'error') return;
    const waited = now - this.firstDirtyAt;
    this.schedule(Math.max(0, Math.min(this.opts.debounceMs, this.opts.maxWaitMs - waited)));
  }

  /** 지금 저장 (재시도 버튼, 페이지 이동 전) */
  async flush(): Promise<void> {
    if (this.timer != null) { this.opts.timers.clear(this.timer); this.timer = null; }
    if (this.inflight) await this.inflight;
    if (this.pending && this.snap.state !== 'conflict') await this.run();
  }

  retry() {
    if (this.snap.state !== 'error') return;
    this.attempt = 0;
    void this.flush();
  }

  /** 다른 사람이 고친 최신본을 받아 다시 시작 (충돌 해결 후) */
  reset(version: number) {
    if (this.timer != null) { this.opts.timers.clear(this.timer); this.timer = null; }
    this.pending = null;
    this.attempt = 0;
    this.set({ state: 'idle', version, error: null, attempt: 0 });
  }

  /** 서버 버전이 다른 경로(상태 변경·복원)로 올라갔을 때 */
  bump(version: number) { if (version > this.snap.version) this.set({ version }); }

  dispose() {
    this.disposed = true;
    if (this.timer != null) this.opts.timers.clear(this.timer);
    this.listeners.clear();
  }

  private run(): Promise<void> {
    if (this.inflight || !this.pending || this.disposed) return this.inflight ?? Promise.resolve();
    const { value } = this.pending;
    this.pending = null;
    this.set({ state: 'saving' });
    const job = (async () => {
      try {
        const r = await this.opts.save(value, this.snap.version);
        this.attempt = 0;
        this.set({ version: r.version, savedAt: (this.opts.timers.now ?? Date.now)(), error: null, attempt: 0, state: this.pending ? 'dirty' : 'saved' });
      } catch (e) {
        // 실패한 값은 더 새로운 입력이 없으면 다시 보낼 값으로 되돌린다
        if (!this.pending) this.pending = { value };
        if (this.opts.isConflict(e)) {
          this.set({ state: 'conflict', error: e });
          return;
        }
        const wait = this.opts.retryMs[Math.min(this.attempt, this.opts.retryMs.length - 1)];
        this.attempt += 1;
        this.set({ state: 'error', error: e, attempt: this.attempt });
        if (!this.disposed) this.schedule(wait);
        return;
      } finally {
        this.inflight = null;
      }
      if (this.pending && !this.disposed) this.schedule(this.opts.debounceMs);
    })();
    this.inflight = job;
    return job;
  }
}
