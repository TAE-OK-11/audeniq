// API client for AUDENIQ backend
// VITE_MOCK=false 로 빌드하면 실제 API를 호출하고, 기본값(true)은 브라우저 저장소 기반 목 데이터로 동작한다.
import { mockApi } from './mock';
import { ApiError } from './errors';

export { ApiError };

export const MOCK = import.meta.env.VITE_MOCK !== 'false';
const API_BASE = (import.meta.env.VITE_API_BASE ?? '').replace(/\/$/, '');

export interface User {
  id: string;
  email: string;
}

export interface Org {
  id: string;
  name: string;
}

export interface Release {
  id: string;
  title: string;
  status: string;
  release_date: string | null;
  created_at: string;
  updated_at?: string;
  track_count: number;
  artist?: string;
  coverData?: string;
}

export interface ReleaseDetail extends Release {
  tracks: Track[];
  draft?: ReleaseDraft;
}

export interface DraftTrack {
  id: string; title: string; version: string; isrc: string;
  composers: string; lyricists: string; arrangers: string; performers: string;
  producer: string; lyrics: string; audioName: string; audioSize: number;
  explicit: boolean; duration: string;
}

export interface CoverTrackData {
  trackId: string; originalTitle: string; originalArtist: string; originalWriters: string;
}

export interface ReleaseOptionsData {
  express: boolean; expressAck: boolean; expressReason: string;
  minor: boolean;
  guardian: string; guardianRelation: string; guardianContact: string;
  guardian2: string; guardian2Relation: string; guardian2Contact: string;
  guardianConsentDone: boolean; familyCertName: string; familyCertMethod: string;
  cover: boolean; coverTracks: CoverTrackData[]; coverRightsAck: boolean; coverLicenseFile: string;
  sample: boolean; sampleLicenseFile: string;
  featured: boolean; featuredConsentFile: string;
  ai: boolean; aiTool: string;
  shared: boolean; sharedContractFile: string;
  rerelease: boolean; previousTitle: string; previousId: string;
}

export interface ReleaseDraft {
  artist?: string;
  type: string;
  language?: string;
  genre: string;
  genreCustom?: string;
  label: string;
  upc: string;
  notes: string;
  coverName: string;
  coverData?: string;
  originalDate?: string;
  territories: string[];
  platforms: string[];
  ownership: string;
  phonogram: string;
  copyright: string;
  rightsChecks: Record<string, boolean>;
  options?: ReleaseOptionsData;
  draftTracks?: DraftTrack[];
  /** 위자드에서 마지막으로 머문 단계 (이어서 작성용) */
  lastStep?: number;
  history: { text: string; time: string }[];
}

export interface Track {
  id: string;
  title: string;
  duration_ms: number | null;
  isrc: string | null;
  version?: string | null;
  composers?: string | null;
  lyricists?: string | null;
  audioName?: string | null;
  explicit?: boolean;
}

/** 위자드 → 서버로 보내는 전체 발매 정보 */
export interface ReleasePayload {
  title: string;
  artist: string;
  type: string;
  language: string;
  genre: string;
  genreCustom: string;
  label: string;
  upc: string;
  notes: string;
  coverName: string;
  coverData: string;
  originalDate: string;
  release_date: string;
  tracks: DraftTrack[];
  territories: string[];
  platforms: string[];
  ownership: string;
  phonogram: string;
  copyright: string;
  rightsChecks: Record<string, boolean>;
  options: ReleaseOptionsData;
  lastStep?: number;
}


let csrfToken = '';
let currentOrgId = '';

/** AuthProvider가 선택한 조직을 알려준다 (조직 범위 API 경로에 사용) */
export function setCurrentOrg(id: string) {
  currentOrgId = id;
}

function orgPath(path: string): string {
  if (!currentOrgId) throw new ApiError('작업 공간을 찾을 수 없어요. 다시 로그인해 주세요.', 400);
  return `/api/orgs/${encodeURIComponent(currentOrgId)}${path}`;
}

async function req<T>(path: string, opts: RequestInit = {}, timeoutMs = 20000): Promise<T> {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(opts.headers as Record<string, string>),
  };
  if (csrfToken && opts.method && opts.method !== 'GET') {
    headers['X-CSRF-Token'] = csrfToken;
  }
  const ctrl = new AbortController();
  const timer = window.setTimeout(() => ctrl.abort(), timeoutMs);
  let res: Response;
  try {
    res = await fetch(`${API_BASE}${path}`, {
      ...opts,
      headers,
      credentials: 'include',
      signal: opts.signal ?? ctrl.signal,
    });
  } catch (e) {
    const aborted = e instanceof DOMException && e.name === 'AbortError';
    throw new ApiError(aborted ? '서버 응답이 늦어요. 잠시 후 다시 시도해 주세요.' : '네트워크 연결을 확인해 주세요.');
  } finally {
    window.clearTimeout(timer);
  }
  if (res.status === 401) {
    // 인증 화면에서의 401은 리다이렉트하지 않음 (무한 새로고침 루프 방지)
    const onAuthPage = /#\/(login|signup|find-account)/.test(window.location.hash || '');
    if (!onAuthPage) window.location.assign(`${import.meta.env.BASE_URL}#/login`);
    throw new ApiError('로그인이 필요해요.', 401);
  }
  if (!res.ok) {
    let message = `요청을 처리하지 못했어요. (${res.status})`;
    try {
      const body = await res.json() as { message?: string; error?: string };
      if (body?.message || body?.error) message = String(body.message || body.error);
    } catch { /* 본문 없음 */ }
    throw new ApiError(message, res.status);
  }
  const text = await res.text();
  return text ? JSON.parse(text) as T : ({} as T);
}

async function fetchCsrf() {
  const csrf = await req<{ token: string }>('/api/auth/csrf', { method: 'POST' });
  csrfToken = csrf.token;
}

export const api = {
  // Auth
  login: async (email: string, password: string): Promise<User> => {
    if (MOCK) return mockApi.login(email, password);
    const u = await req<User>('/api/auth/login', { method: 'POST', body: JSON.stringify({ email, password }) });
    await fetchCsrf();
    return u;
  },
  signup: async (email: string, password: string): Promise<User> => {
    if (MOCK) return mockApi.signup(email, password);
    const u = await req<User>('/api/auth/register', { method: 'POST', body: JSON.stringify({ email, password }) });
    await fetchCsrf();
    return u;
  },
  logout: async (): Promise<void> => {
    if (MOCK) return mockApi.logout();
    await req('/api/auth/logout', { method: 'POST' });
    csrfToken = '';
  },
  me: async (): Promise<User> => {
    if (MOCK) return mockApi.me();
    const u = await req<User>('/api/me');
    if (!csrfToken) await fetchCsrf().catch(() => {});
    return u;
  },

  // Orgs
  listOrgs: (): Promise<Org[]> => {
    if (MOCK) return mockApi.listOrgs();
    return req('/api/orgs');
  },

  // Releases
  listReleases: (): Promise<Release[]> => {
    if (MOCK) return mockApi.listReleases();
    return req(orgPath('/releases'));
  },
  getRelease: (id: string): Promise<ReleaseDetail> => {
    if (MOCK) return mockApi.getRelease(id);
    return req(orgPath(`/releases/${encodeURIComponent(id)}`));
  },
  /** 임시 저장 — id가 없으면 새 draft를 만들고, 있으면 같은 draft를 갱신 */
  saveDraft: (id: string | null, data: ReleasePayload): Promise<Release> => {
    if (MOCK) return mockApi.saveDraft(id, data);
    return id
      ? req(orgPath(`/releases/${encodeURIComponent(id)}`), { method: 'PUT', body: JSON.stringify({ ...data, status: 'draft' }) })
      : req(orgPath('/releases'), { method: 'POST', body: JSON.stringify({ ...data, status: 'draft' }) });
  },
  /** 발매 신청 접수 (새 발매 또는 기존 draft/발매 수정) */
  submitRelease: async (id: string | null, data: ReleasePayload): Promise<Release> => {
    if (MOCK) return mockApi.submitRelease(id, data);
    const saved = await api.saveDraft(id, data);
    await req(orgPath(`/releases/${encodeURIComponent(saved.id)}/submit`), { method: 'POST' });
    return saved;
  },
  deleteRelease: (id: string): Promise<void> => {
    if (MOCK) return mockApi.deleteRelease(id);
    return req(orgPath(`/releases/${encodeURIComponent(id)}`), { method: 'DELETE' });
  },
};
