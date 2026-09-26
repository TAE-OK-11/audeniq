// 목 API — 실제 서버 없이도 전체 흐름을 쓸 수 있도록 브라우저 저장소(localStorage)에 영속화한다.
import type { User, Org, Release, ReleaseDetail, ReleasePayload, Track } from './client';
import { ApiError } from './errors';
import { createStore, uid } from '../lib/store';
import { readJSON, removeKey, writeJSON } from '../lib/storage';
import { stampNow, todayStr } from '../lib/date';

const delay = (ms: number) => new Promise(r => setTimeout(r, ms));

const mockOrgs: Org[] = [{ id: 'org_1', name: '내 작업 공간' }];

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
    id: 'r3', title: '데모 트랙', status: 'needs', release_date: null, created_at: '2026-09-24', updated_at: '2026-09-24 11:20', track_count: 1, artist: '서린',
    tracks: [{ id: 't7', title: '데모 트랙', duration_ms: 172000, isrc: null }],
  },
  {
    id: 'r4', title: '미발매 작업물', status: 'draft', release_date: null, created_at: '2026-09-25', updated_at: '2026-09-25 18:02', track_count: 0, artist: '서린', tracks: [],
  },
];

const db = createStore<ReleaseDetail[]>(() => structuredClone(SEED), {
  persist: 'mock.releases',
  revive: (raw, fallback) => (Array.isArray(raw) ? raw as ReleaseDetail[] : fallback),
});

const summary = (d: ReleaseDetail): Release => ({
  id: d.id, title: d.title, status: d.status, release_date: d.release_date,
  created_at: d.created_at, updated_at: d.updated_at, track_count: d.tracks.length,
  artist: d.artist, coverData: d.draft?.coverData || undefined,
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
    await delay(450);
    if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) throw new ApiError('이메일 형식을 확인해 주세요.', 400);
    if (!password) throw new ApiError('비밀번호를 입력해 주세요.', 400);
    const s: Session = { email };
    writeJSON(SESSION_KEY, s);
    return sessionUser(s);
  },
  signup: async (email: string, password: string): Promise<User> => {
    await delay(550);
    if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) throw new ApiError('이메일 형식을 확인해 주세요.', 400);
    if (password.length < 8) throw new ApiError('비밀번호는 8자 이상이어야 해요.', 400);
    const s: Session = { email };
    writeJSON(SESSION_KEY, s);
    return sessionUser(s);
  },
  logout: async (): Promise<void> => {
    await delay(150);
    removeKey(SESSION_KEY);
  },
  me: async (): Promise<User> => {
    await delay(120);
    const s = readJSON<Session | null>(SESSION_KEY, null);
    if (!s?.email) throw new ApiError('로그인이 필요해요.', 401);
    return sessionUser(s);
  },
  listOrgs: async (): Promise<Org[]> => {
    await delay(80);
    return mockOrgs;
  },
  listReleases: async (): Promise<Release[]> => {
    await delay(320);
    return db.get()
      .map(summary)
      .sort((a, b) => String(b.updated_at || b.created_at).localeCompare(String(a.updated_at || a.created_at)));
  },
  getRelease: async (id: string): Promise<ReleaseDetail> => {
    await delay(220);
    const d = db.get().find(r => r.id === id);
    if (!d) throw new ApiError('발매를 찾을 수 없어요. 삭제됐거나 주소가 잘못됐을 수 있어요.', 404);
    return structuredClone(d);
  },
  saveDraft: async (id: string | null, data: ReleasePayload): Promise<Release> => {
    await delay(160);
    const existing = id ? db.get().find(r => r.id === id) : undefined;
    if (id && !existing) throw new ApiError('임시 저장을 찾을 수 없어요.', 404);
    // 이미 접수된 발매는 임시 저장으로 상태를 되돌리지 않는다
    const status = existing && existing.status !== 'draft' ? existing.status : 'draft';
    const next = apply(existing, data, status, existing ? null : '임시 저장 시작');
    upsert(next);
    return summary(next);
  },
  submitRelease: async (id: string | null, data: ReleasePayload): Promise<Release> => {
    await delay(600);
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
    await delay(260);
    db.set(list => list.filter(r => r.id !== id));
  },
  /** 데모 데이터 초기화 */
  resetData: () => db.reset(),
};
