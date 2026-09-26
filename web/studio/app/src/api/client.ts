// API client for AUDENIQ backend
import { mockApi } from './mock';

// Design test mode: no real API calls
const MOCK = true;
const API_BASE = '';

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
  track_count: number;
  artist?: string;
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
  sample?: boolean;
}

let csrfToken = '';

async function req<T>(path: string, opts: RequestInit = {}): Promise<T> {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(opts.headers as Record<string, string>),
  };
  if (csrfToken && opts.method && opts.method !== 'GET') {
    headers['X-CSRF-Token'] = csrfToken;
  }
  const res = await fetch(`${API_BASE}${path}`, {
    ...opts,
    headers,
    credentials: 'include',
  });
  if (res.status === 401) {
    window.location.href = '/connected/#/login';
    throw new Error('로그인이 필요합니다');
  }
  if (!res.ok) {
    const body = await res.text();
    throw new Error(`API ${res.status}: ${body.slice(0, 200)}`);
  }
  const text = await res.text();
  return text ? JSON.parse(text) : ({} as T);
}

export const api = {
  // Auth
  login: async (email: string, password: string): Promise<User> => {
    if (MOCK) { const u = await mockApi.login(); mockApi.setSession(); return u; }
    // Get CSRF token first
    const csrf = await req<{ token: string }>('/api/auth/csrf', { method: 'POST' });
    csrfToken = csrf.token;
    return req<User>('/api/auth/login', {
      method: 'POST',
      body: JSON.stringify({ email, password }),
    });
  },
  logout: (): Promise<void> => {
    if (MOCK) { mockApi.clearSession(); return mockApi.logout(); }
    return req('/api/auth/logout', { method: 'POST' });
  },
  me: (): Promise<User> => {
    if (MOCK) return mockApi.me();
    return req('/api/me');
  },

  // Orgs
  listOrgs: (): Promise<Org[]> => {
    if (MOCK) return mockApi.listOrgs();
    return req('/api/orgs');
  },

  // Releases
  listReleases: (orgId: string): Promise<Release[]> => {
    if (MOCK) return mockApi.listReleases();
    return req(`/api/orgs/${orgId}/releases`);
  },
  getRelease: (orgId: string, id: string): Promise<ReleaseDetail> => {
    if (MOCK) return mockApi.getRelease(id);
    return req(`/api/orgs/${orgId}/releases/${id}`);
  },
  createRelease: (orgId: string, data: { title: string; release_date: string }): Promise<Release> => {
    if (MOCK) return mockApi.createRelease(data);
    return req(`/api/orgs/${orgId}/releases`, {
      method: 'POST',
      body: JSON.stringify({ ...data, draft: {} }),
    });
  },
};
