// Mock data for design testing - no real API calls
import type { User, Org, Release, ReleaseDetail } from './client';

const delay = (ms: number) => new Promise(r => setTimeout(r, ms));

const mockUser: User = { id: 'u_test', email: 'test@audeniq.kr' };
const mockOrgs: Org[] = [
  { id: 'org_1', name: '테스트 레이블' },
];

const mockReleases: Release[] = [
  { id: 'r1', title: '첫 번째 싱글', status: 'LIVE', release_date: '2026-10-01', created_at: '2026-09-20', track_count: 2, artist: '서린' },
  { id: 'r2', title: '여름 EP', status: 'STAGE1_PASSED', release_date: '2026-11-15', created_at: '2026-09-22', track_count: 4, artist: '서린' },
  { id: 'r3', title: '데모 트랙', status: 'STAGE1_CORRECTION', release_date: null, created_at: '2026-09-24', track_count: 1, artist: '서린' },
  { id: 'r4', title: '미발매 작업물', status: 'DRAFT', release_date: null, created_at: '2026-09-25', track_count: 0, artist: '서린' },
];

const mockDetails: Record<string, ReleaseDetail> = {
  r1: {
    ...mockReleases[0],
    tracks: [
      { id: 't1', title: '첫 번째 싱글', duration_ms: 214000, isrc: 'KRA262600001' },
      { id: 't2', title: '첫 번째 싱글 (Inst.)', duration_ms: 214000, isrc: 'KRA262600002' },
    ],
  },
  r2: {
    ...mockReleases[1],
    tracks: [
      { id: 't3', title: '파도', duration_ms: 198000, isrc: 'KRA262600003' },
      { id: 't4', title: '한낮', duration_ms: 224000, isrc: null },
      { id: 't5', title: '노을', duration_ms: 186000, isrc: null },
      { id: 't6', title: '밤바다', duration_ms: 241000, isrc: null },
    ],
  },
  r3: {
    ...mockReleases[2],
    tracks: [
      { id: 't7', title: '데모 트랙', duration_ms: 172000, isrc: null },
    ],
  },
  r4: { ...mockReleases[3], tracks: [] },
};

export const mockApi = {
  login: async (): Promise<User> => {
    await delay(400);
    return mockUser;
  },
  logout: async (): Promise<void> => { await delay(200); },
  me: async (): Promise<User> => {
    await delay(200);
    const saved = localStorage.getItem('mock_session');
    if (!saved) throw new Error('no session');
    return mockUser;
  },
  setSession: () => localStorage.setItem('mock_session', '1'),
  clearSession: () => localStorage.removeItem('mock_session'),
  listOrgs: async (): Promise<Org[]> => {
    await delay(200);
    return mockOrgs;
  },
  listReleases: async (): Promise<Release[]> => {
    await delay(400);
    return mockReleases;
  },
  getRelease: async (id: string): Promise<ReleaseDetail> => {
    await delay(300);
    const d = mockDetails[id];
    if (!d) throw new Error('발매를 찾을 수 없음');
    return d;
  },
  createRelease: async (data: { title: string; release_date: string }): Promise<Release> => {
    await delay(500);
    const r: Release = {
      id: 'r' + Date.now(),
      title: data.title,
      status: 'DRAFT',
      release_date: data.release_date || null,
      created_at: new Date().toISOString().slice(0, 10),
      track_count: 0,
    };
    mockReleases.unshift(r);
    mockDetails[r.id] = { ...r, tracks: [] };
    return r;
  },
};
