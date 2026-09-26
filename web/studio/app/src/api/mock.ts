// 목 API — 실제 서버 없이도 전체 흐름을 쓸 수 있도록 브라우저 저장소(localStorage)에 영속화한다.
import type { Correction, User, Org, Release, ReleaseDetail, ReleasePayload, Track } from './types';
import { ApiError } from './errors';
import { createStore, uid } from '../lib/store';
import { readJSON, removeKey, writeJSON } from '../lib/storage';
import { stampNow, todayStr } from '../lib/date';

// 네트워크 흉내 지연 — 읽기는 거의 즉시, 인증·접수만 짧게 (버튼 진행 상태가 보이도록)
const delay = (ms: number) => new Promise(r => setTimeout(r, ms));

const mockOrgs: Org[] = [{ id: 'org_1', name: '내 작업 공간' }];

// 데모: 담당자 보완 요청 (코드 → 신청서 위치는 lib/corrections.ts)
const SEED_CORRECTIONS: Record<string, Correction[]> = {
  r3: [
    { code: 'IMAGE_TOO_SMALL', message: '커버아트 해상도가 1000×1000이에요. 3000×3000 이상 정사각형 이미지로 다시 올려 주세요.' },
    { code: 'S2_META_CREDITS', message: '‘데모 트랙’의 작곡가 정보가 비어 있어요. 실제 작곡가 이름을 입력해 주세요.', trackId: 't7' },
  ],
};

const SEED: ReleaseDetail[] = [
  {
    id: 'r1', title: '첫 번째 싱글', status: 'live', release_date: '2026-10-01', created_at: '2026-09-20', updated_at: '2026-09-21 10:05', track_count: 2, artist: '서린',
    tracks: [
      { id: 't1', title: '첫 번째 싱글', duration_ms: 214000, isrc: 'KRA262600001', composers: '서린', lyricists: '서린', audioName: 'first_single_master.wav' },
      { id: 't2', title: '첫 번째 싱글 (Inst.)', duration_ms: 214000, isrc: 'KRA262600002', composers: '서린', audioName: 'first_single_inst.wav' },
    ],
    draft: {
      artist: '서린', type: 'single', language: 'ko', genre: 'Indie Pop', label: 'AUDENIQ', upc: '8800000000011', notes: '하루의 끝에서 시작되는 첫 번째 이야기.',
      coverName: '', territories: ['WORLD'], platforms: ['spotify', 'apple', 'youtube', 'melon'], ownership: '서린', phonogram: '2026 서린', copyright: '2026 서린',
      rightsChecks: { rightsMaster: true, rightsComposition: true, rightsArtwork: true, rightsConsent: true },
      history: [
        { text: '발매 신청 접수 완료 · AUDENIQ 검토 시작', time: '2026-09-20 14:32' },
        { text: '검토 완료 · 발매 확정', time: '2026-09-21 10:05' },
      ],
    },
  },
  {
    id: 'r2', title: '여름 EP', status: 'review', release_date: '2026-11-15', created_at: '2026-09-22', updated_at: '2026-09-22 09:10', track_count: 4, artist: '서린',
    tracks: [
      { id: 't3', title: '파도', duration_ms: 198000, isrc: 'KRA262600003', composers: '서린' },
      { id: 't4', title: '한낮', duration_ms: 224000, isrc: null, composers: '서린' },
      { id: 't5', title: '노을', duration_ms: 186000, isrc: null, composers: '서린' },
      { id: 't6', title: '밤바다', duration_ms: 241000, isrc: null, composers: '서린' },
    ],
    draft: {
      artist: '서린', type: 'ep', language: 'ko', genre: 'Ballad', label: 'AUDENIQ', upc: '', notes: '',
      coverName: '', territories: ['WORLD'], platforms: ['spotify', 'apple', 'melon', 'genie', 'flo'], ownership: '서린', phonogram: '2026 서린', copyright: '2026 서린',
      rightsChecks: {}, history: [{ text: '발매 신청 접수 완료 · AUDENIQ 검토 시작', time: '2026-09-22 09:10' }],
    },
  },
  {
    id: 'r3', title: '데모 트랙', status: 'needs', release_date: '2026-12-05', created_at: '2026-09-24', updated_at: '2026-09-24 11:20', track_count: 1, artist: '서린',
    tracks: [{ id: 't7', title: '데모 트랙', duration_ms: 172000, isrc: null, audioName: 'demo_track_master.wav' }],
    draft: {
      artist: '서린', type: 'single', language: 'ko', genre: 'Indie Pop', label: '', upc: '', notes: '',
      coverName: 'demo_cover.jpg', territories: ['WORLD'], platforms: ['spotify', 'apple', 'youtube', 'melon'],
      ownership: '서린', phonogram: '2026 서린', copyright: '2026 서린',
      rightsChecks: { rightsMaster: true, rightsComposition: true, rightsArtwork: true, rightsConsent: true },
      history: [
        { text: '발매 신청 접수 완료 · AUDENIQ 검토 시작', time: '2026-09-23 10:02' },
        { text: '검토 결과 · 보완 요청 2건', time: '2026-09-23 11:20' },
      ],
    },
    corrections: SEED_CORRECTIONS.r3,
  },
  {
    id: 'r4', title: '미발매 작업물', status: 'draft', release_date: null, created_at: '2026-09-25', updated_at: '2026-09-25 18:02', track_count: 0, artist: '서린', tracks: [],
  },
];

const STATUSES = new Set(['draft', 'ready', 'needs', 'review', 'scheduled', 'live']);
const str = (v: unknown, d = '') => (typeof v === 'string' ? v : d);
const arr = <T,>(v: unknown): T[] => (Array.isArray(v) ? v as T[] : []);

/** 저장된 발매를 현재 형식으로 보정 — 구버전·손상 데이터 때문에 화면이 깨지지 않도록 */
export function normalizeRelease(raw: unknown): ReleaseDetail | null {
  if (!raw || typeof raw !== 'object') return null;
  const r = raw as Partial<ReleaseDetail>;
  if (typeof r.id !== 'string' || !r.id) return null;
  const tracks = arr<Track>(r.tracks).filter(t => t && typeof t === 'object' && typeof t.id === 'string')
    .map(t => ({ ...t, title: str(t.title), duration_ms: typeof t.duration_ms === 'number' ? t.duration_ms : null, isrc: t.isrc ?? null }));
  const d = r.draft && typeof r.draft === 'object' ? r.draft : undefined;
  const status = STATUSES.has(str(r.status)) ? str(r.status) : 'draft';
  // 보완 요청은 '보완 필요' 상태에서만 유지 (예전 저장 데이터의 데모 발매에는 기본 요청을 채운다)
  const corrections = status !== 'needs' ? undefined
    : Array.isArray(r.corrections) ? r.corrections.filter(c => c && typeof c.code === 'string')
    : SEED_CORRECTIONS[r.id];
  return {
    ...r,
    id: r.id,
    title: str(r.title, '제목 없는 발매'),
    status,
    corrections,
    release_date: typeof r.release_date === 'string' && r.release_date ? r.release_date : null,
    created_at: str(r.created_at, todayStr()),
    track_count: tracks.length,
    tracks,
    draft: d && {
      ...d,
      type: str(d.type, 'single'), genre: str(d.genre), label: str(d.label), upc: str(d.upc), notes: str(d.notes),
      coverName: str(d.coverName), territories: arr<string>(d.territories), platforms: arr<string>(d.platforms),
      ownership: str(d.ownership), phonogram: str(d.phonogram), copyright: str(d.copyright),
      rightsChecks: d.rightsChecks && typeof d.rightsChecks === 'object' ? d.rightsChecks : {},
      draftTracks: d.draftTracks ? arr(d.draftTracks) : undefined,
      history: arr<{ text: string; time: string }>(d.history).filter(h => h && typeof h.text === 'string'),
    },
  };
}

const db = createStore<ReleaseDetail[]>(() => structuredClone(SEED), {
  persist: 'mock.releases',
  revive: (raw, fallback) => (Array.isArray(raw)
    ? raw.map(normalizeRelease).filter((r): r is ReleaseDetail => !!r)
    : fallback),
});

const summary = (d: ReleaseDetail): Release => ({
  id: d.id, title: d.title, status: d.status, release_date: d.release_date,
  created_at: d.created_at, updated_at: d.updated_at, track_count: d.tracks.length,
  artist: d.artist, coverData: d.draft?.coverData || undefined,
  corrections: d.corrections?.length ? d.corrections : undefined,
});

function durationMs(mmss: string): number | null {
  const m = /^(\d+):(\d{2})$/.exec(mmss || '');
  return m ? (+m[1] * 60 + +m[2]) * 1000 : null;
}

function toTracks(data: ReleasePayload): Track[] {
  return data.tracks.map(t => ({
    id: t.id,
    title: t.title,
    duration_ms: durationMs(t.duration),
    isrc: t.isrc || null,
    version: t.version || null,
    composers: t.composers || null,
    lyricists: t.lyricists || null,
    audioName: t.audioName || null,
    explicit: t.explicit,
  }));
}

function apply(existing: ReleaseDetail | undefined, data: ReleasePayload, status: string, historyText: string | null): ReleaseDetail {
  const now = stampNow();
  const prevHistory = existing?.draft?.history ?? [];
  return {
    id: existing?.id ?? uid('r'),
    title: data.title || '제목 없는 발매',
    artist: data.artist || undefined,
    status,
    // 보완 필요 상태로 임시 저장할 때는 요청 항목을 그대로 두고, 다시 접수하면 비운다
    corrections: status === 'needs' ? existing?.corrections : undefined,
    release_date: data.release_date || null,
    created_at: existing?.created_at ?? todayStr(),
    updated_at: now,
    track_count: data.tracks.length,
    tracks: toTracks(data),
    draft: {
      artist: data.artist, type: data.type, language: data.language, genre: data.genre,
      genreCustom: data.genreCustom, label: data.label, upc: data.upc, notes: data.notes,
      coverName: data.coverName, coverData: data.coverData, originalDate: data.originalDate,
      territories: data.territories, platforms: data.platforms, ownership: data.ownership,
      phonogram: data.phonogram, copyright: data.copyright, rightsChecks: data.rightsChecks,
      options: data.options, draftTracks: data.tracks.map(t => ({ ...t })), lastStep: data.lastStep,
      artistProfile: data.artistProfile,
      application: data.application ?? existing?.draft?.application,
      history: historyText ? [...prevHistory, { text: historyText, time: now }] : prevHistory,
    },
  };
}

function upsert(next: ReleaseDetail) {
  db.set(list => {
    const i = list.findIndex(r => r.id === next.id);
    if (i < 0) return [next, ...list];
    const copy = list.slice();
    copy[i] = next;
    return copy;
  });
}

interface Session { email: string }
const SESSION_KEY = 'mock.session';

function sessionUser(s: Session): User {
  return { id: 'u_' + s.email.replace(/[^a-z0-9]/gi, '').slice(0, 16), email: s.email };
}

export const mockApi = {
  login: async (email: string, password: string): Promise<User> => {
    await delay(250);
    if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) throw new ApiError('이메일 형식을 확인해 주세요.', 400);
    if (!password) throw new ApiError('비밀번호를 입력해 주세요.', 400);
    const s: Session = { email };
    writeJSON(SESSION_KEY, s);
    return sessionUser(s);
  },
  signup: async (email: string, password: string): Promise<User> => {
    await delay(300);
    if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) throw new ApiError('이메일 형식을 확인해 주세요.', 400);
    if (password.length < 8) throw new ApiError('비밀번호는 8자 이상이어야 해요.', 400);
    const s: Session = { email };
    writeJSON(SESSION_KEY, s);
    return sessionUser(s);
  },
  logout: async (): Promise<void> => {
    await delay(0);
    removeKey(SESSION_KEY);
  },
  me: async (): Promise<User> => {
    await delay(0);
    const s = readJSON<Session | null>(SESSION_KEY, null);
    if (!s?.email) throw new ApiError('로그인이 필요해요.', 401);
    return sessionUser(s);
  },
  listOrgs: async (): Promise<Org[]> => {
    await delay(0);
    return mockOrgs;
  },
  listReleases: async (): Promise<Release[]> => {
    await delay(60);
    return db.get()
      .map(summary)
      .sort((a, b) => String(b.updated_at || b.created_at).localeCompare(String(a.updated_at || a.created_at)));
  },
  getRelease: async (id: string): Promise<ReleaseDetail> => {
    await delay(40);
    const d = db.get().find(r => r.id === id);
    if (!d) throw new ApiError('발매를 찾을 수 없어요. 삭제됐거나 주소가 잘못됐을 수 있어요.', 404);
    return structuredClone(d);
  },
  saveDraft: async (id: string | null, data: ReleasePayload): Promise<Release> => {
    await delay(0);
    const existing = id ? db.get().find(r => r.id === id) : undefined;
    if (id && !existing) throw new ApiError('임시 저장을 찾을 수 없어요.', 404);
    // 이미 접수된 발매는 임시 저장으로 상태를 되돌리지 않는다
    const status = existing && existing.status !== 'draft' ? existing.status : 'draft';
    const next = apply(existing, data, status, existing ? null : '임시 저장 시작');
    upsert(next);
    return summary(next);
  },
  submitRelease: async (id: string | null, data: ReleasePayload): Promise<Release> => {
    await delay(250);
    const existing = id ? db.get().find(r => r.id === id) : undefined;
    if (id && !existing) throw new ApiError('발매를 찾을 수 없어요.', 404);
    const prev = existing?.status ?? 'draft';
    const firstSubmit = ['draft', 'ready', 'needs'].includes(prev);
    const status = firstSubmit ? 'review' : prev;
    const text = prev === 'draft' ? '발매 신청 접수 완료 · AUDENIQ 검토 시작'
      : prev === 'needs' ? '보완 내용 제출 · 재검토 시작'
      : prev === 'live' ? '발매 정보 수정 요청 접수'
      : '발매 정보 수정 접수';
    const next = apply(existing, data, status, text);
    upsert(next);
    return summary(next);
  },
  deleteRelease: async (id: string): Promise<void> => {
    await delay(80);
    db.set(list => list.filter(r => r.id !== id));
  },
  /** 데모 데이터 초기화 */
  resetData: () => db.reset(),
};
