// 아티스트 페이지 관리 — 서버 crates/core/src/artist_studio.rs. 소개 페이지·스마트링크·소식·응원의 반응(기간별 방문·이동·채널·발매)과
// 프로필 사진, 플랫폼에 사진 바꾸기 요청. 체험 모드는 같은 모양의 예시 숫자를 만든다 (날짜마다 같은 값).
import { API_BASE, orgPath, req } from '../api/http';
import { MOCK } from './mode';

export type Period = 7 | 30 | 90;
export const PERIODS: Period[] = [7, 30, 90];

export interface DayPoint { day: string; page_views: number; link_views: number; clicks: number }
export interface Totals { page_views: number; link_views: number; clicks: number }
export interface ReleaseInsight {
  release_id: string; title: string; type?: string; release_date: string | null; released: boolean; cover: boolean;
  link: { slug: string; enabled: boolean; visible: boolean } | null;
  views: number; clicks: number; alerts: number; alerts_new: number; presaves: number; presaves_new: number;
}
export interface PhotoRequest {
  id: string; platform: string; status: string; resolution: string; created_at: string; updated_at: string;
  /** 지금 사진으로 한 요청인지 (사진을 바꾸면 false) */
  current: boolean;
}
export interface Insights {
  artist_id: string; name: string; days: Period;
  page: { slug: string; enabled: boolean; cheers_enabled: boolean } | null;
  photo: boolean; photo_platforms: string[]; photo_requests: PhotoRequest[];
  totals: Totals; previous: Totals; series: DayPoint[];
  platforms: Record<string, number>; sources: Record<string, number>;
  releases: ReleaseInsight[];
  community: { posts: number; cheers: number; cheers_new: number; hearts: number };
}

/** 사진을 AUDENIQ이 플랫폼에 보내는 곳 — 서버 PHOTO_PLATFORMS와 같은 순서 */
export const SENDABLE = ['melon', 'genie', 'bugs', 'flo', 'amazon'];

/** 아티스트 본인 계정에서만 사진을 바꿀 수 있는 곳 — 바로 가는 도구 주소 */
export const SELF_SERVE: { key: string; tool: string; url: string }[] = [
  { key: 'spotify', tool: 'Spotify for Artists', url: 'https://artists.spotify.com/' },
  { key: 'apple', tool: 'Apple Music for Artists', url: 'https://artists.apple.com/' },
  { key: 'youtube', tool: 'YouTube Studio', url: 'https://studio.youtube.com/' },
  { key: 'deezer', tool: 'Deezer for Creators', url: 'https://creators.deezer.com/' },
];

/** 바뀐 비율(%) — 이전 기간이 0이면 비교하지 않는다 */
export function change(now: number, before: number): number | null {
  if (!before) return null;
  return Math.round(((now - before) / before) * 100);
}

/** 이동률 — 스마트링크 방문 중 플랫폼으로 넘어간 비율(%) */
export function clickRate(t: Totals): number | null {
  return t.link_views ? Math.round((t.clicks / t.link_views) * 100) : null;
}

export const photoUrl = (artistId: string, version: string | number) =>
  `${API_BASE}${orgPath(`/artists/${encodeURIComponent(artistId)}/photo`)}?v=${encodeURIComponent(String(version))}`;

// ---------------------------------------------------------------------------
// 체험 모드
// ---------------------------------------------------------------------------
const mockState: { photo: string | null; requests: PhotoRequest[] } = { photo: null, requests: [] };

/** 날짜로 정해지는 0~1 값 — 다시 열어도 같은 그래프 */
function seeded(n: number): number {
  const x = Math.sin(n * 12.9898) * 43758.5453;
  return x - Math.floor(x);
}

function mockInsights(artistId: string, name: string, days: Period, page: Insights['page']): Insights {
  const today = new Date();
  const series: DayPoint[] = [];
  const make = (offset: number) => {
    const d = new Date(today);
    d.setDate(d.getDate() - offset);
    const n = Math.floor(d.getTime() / 86400000);
    // 발매 직후(12일 전)에 오르고 주말에 조금 더
    const boost = Math.max(0, 1 - Math.abs(offset - 12) / 9) * 60;
    const weekend = d.getDay() === 0 || d.getDay() === 6 ? 1.3 : 1;
    const link = Math.round((14 + seeded(n) * 22 + boost) * weekend);
    return {
      day: d.toISOString().slice(0, 10),
      page_views: Math.round((5 + seeded(n + 7) * 9 + boost / 3) * weekend),
      link_views: link,
      clicks: Math.round(link * (0.38 + seeded(n + 3) * 0.2)),
    };
  };
  for (let i = days - 1; i >= 0; i--) series.push(make(i));
  const prev: DayPoint[] = [];
  for (let i = days * 2 - 1; i >= days; i--) prev.push(make(i));
  const sum = (list: DayPoint[], k: keyof Totals) => list.reduce((n, d) => n + d[k], 0);
  const totals = { page_views: sum(series, 'page_views'), link_views: sum(series, 'link_views'), clicks: sum(series, 'clicks') };
  const share = (n: number, parts: [string, number][]) => Object.fromEntries(parts.map(([k, w]) => [k, Math.round(n * w)]));
  return {
    artist_id: artistId, name, days, page, photo: !!mockState.photo,
    photo_platforms: SENDABLE, photo_requests: mockState.requests,
    totals,
    previous: { page_views: sum(prev, 'page_views'), link_views: sum(prev, 'link_views'), clicks: sum(prev, 'clicks') },
    series,
    platforms: share(totals.clicks, [['melon', 0.34], ['spotify', 0.22], ['youtube', 0.16], ['apple', 0.1], ['genie', 0.08], ['flo', 0.06], ['bugs', 0.04]]),
    sources: share(totals.link_views, [['instagram', 0.41], ['youtube', 0.17], ['kakao', 0.12], ['x', 0.07], ['qr', 0.05]]),
    releases: [
      { release_id: 'mock-soon', title: '겨울 산책', type: 'SINGLE', release_date: '2099-01-01', released: false, cover: false,
        link: { slug: 'soon0001', enabled: true, visible: true },
        views: Math.round(totals.link_views * 0.38), clicks: 0, alerts: 64, alerts_new: 21, presaves: 37, presaves_new: 12 },
      { release_id: 'mock-1', title: '여름의 끝', type: 'EP', release_date: '2026-08-01', released: true, cover: false,
        link: { slug: 'mock0001', enabled: true, visible: true },
        views: Math.round(totals.link_views * 0.47), clicks: Math.round(totals.clicks * 0.8), alerts: 0, alerts_new: 0, presaves: 0, presaves_new: 0 },
      { release_id: 'mock-2', title: '첫 번째 싱글', type: 'SINGLE', release_date: '2026-03-14', released: true, cover: false,
        link: { slug: 'mock0002', enabled: true, visible: true },
        views: Math.round(totals.link_views * 0.15), clicks: Math.round(totals.clicks * 0.2), alerts: 0, alerts_new: 0, presaves: 0, presaves_new: 0 },
      { release_id: 'mock-3', title: '데모 트랙', type: 'SINGLE', release_date: '2026-12-05', released: false, cover: false,
        link: null, views: 0, clicks: 0, alerts: 0, alerts_new: 0, presaves: 0, presaves_new: 0 },
    ],
    community: { posts: 3, cheers: 18, cheers_new: days === 7 ? 4 : 11, hearts: 6 },
  };
}

// ---------------------------------------------------------------------------
// 서버
// ---------------------------------------------------------------------------
export async function fetchInsights(artistId: string, name: string, days: Period, page: Insights['page']): Promise<Insights> {
  if (MOCK) { await new Promise(r => setTimeout(r, 250)); return mockInsights(artistId, name, days, page); }
  return req<Insights>(orgPath(`/artists/${encodeURIComponent(artistId)}/insights?days=${days}`), { quiet401: true });
}

export async function setPhoto(artistId: string, assetId: string, preview: string): Promise<void> {
  if (MOCK) { mockState.photo = preview; mockState.requests = mockState.requests.map(r => ({ ...r, current: false })); return; }
  // 막 올린 사진은 안전 검사가 끝나야 쓸 수 있다 — 끝날 때까지 잠깐씩 기다렸다 다시 (최대 30초쯤)
  for (let i = 0; ; i++) {
    try {
      await req(orgPath(`/artists/${encodeURIComponent(artistId)}/photo`), { method: 'PUT', body: { asset_id: assetId } });
      return;
    } catch (e) {
      if (i >= 11 || (e as { code?: string })?.code !== 'ARTIST_PHOTO_NOT_READY') throw e;
      await new Promise(r => setTimeout(r, 2500));
    }
  }
}

export async function removePhoto(artistId: string): Promise<void> {
  if (MOCK) { mockState.photo = null; return; }
  await req(orgPath(`/artists/${encodeURIComponent(artistId)}/photo`), { method: 'DELETE' });
}

/** 체험 모드에서 고른 사진 (서버 주소 대신) */
export const mockPhoto = () => mockState.photo;

export async function sendPhoto(artistId: string, platforms: string[]): Promise<{ created: { id: string; platform: string }[]; skipped: string[] }> {
  if (MOCK) {
    const open = (p: string) => mockState.requests.some(r => r.platform === p && ['SUBMITTED', 'IN_PROGRESS', 'SENT_TO_PLATFORM'].includes(r.status));
    const now = new Date().toISOString();
    const created = platforms.filter(p => !open(p)).map(p => ({ id: `mock-${p}-${Date.now()}`, platform: p }));
    mockState.requests = [
      ...mockState.requests.filter(r => !created.some(c => c.platform === r.platform)),
      ...created.map(c => ({ ...c, status: 'SUBMITTED', resolution: '', created_at: now, updated_at: now, current: true })),
    ];
    return { created, skipped: platforms.filter(open) };
  }
  return req(orgPath(`/artists/${encodeURIComponent(artistId)}/photo/send`), { method: 'POST', body: { platforms } });
}
