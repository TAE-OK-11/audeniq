// API client for AUDENIQ backend
const API_BASE = '';

export interface Release {
  id: string;
  title: string;
  status: string;
  release_date: string | null;
  created_at: string;
  track_count: number;
}

export interface ReleaseDetail extends Release {
  tracks: Track[];
}

export interface Track {
  id: string;
  title: string;
  duration_ms: number | null;
  isrc: string | null;
}

async function req<T>(path: string, opts: RequestInit = {}): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    ...opts,
    headers: { 'Content-Type': 'application/json', ...opts.headers },
    credentials: 'include',
  });
  if (!res.ok) {
    const body = await res.text();
    throw new Error(`API ${res.status}: ${body.slice(0, 200)}`);
  }
  return res.json();
}

export const api = {
  // TODO: wire to real endpoints once auth is set up
  listReleases: (): Promise<Release[]> =>
    req('/api/releases'),
  getRelease: (id: string): Promise<ReleaseDetail> =>
    req(`/api/releases/${id}`),
  createRelease: (data: { title: string; release_date: string }): Promise<Release> =>
    req('/api/releases', { method: 'POST', body: JSON.stringify(data) }),
};
