// 보도자료 — 서버 crates/core/src/artist_press.rs. 보도용 소개(한 줄 소개·짧은 소개·장르·활동 지역·소속·데뷔·주요 활동·인용)와
// 아티스트가 쓰는 보도자료(초안은 나만, 공개하면 보도자료 페이지에). 발매를 고르면 발매 정보로 초안을 채워 준다.
import { orgPath, req } from '../api/http';
import { MOCK } from './mode';

export interface PressQuote { quote: string; source: string }
export interface PressProfile {
  tagline: string; bio_short: string; genre: string; based_in: string; label: string; debut: string;
  highlights: string[]; quotes: PressQuote[];
}
export interface PressRelease {
  id: string; release_id: string | null; release_title: string | null;
  title: string; lead: string; body: string; published: boolean; published_at: string | null;
  created_at: string; updated_at: string;
}
export interface PublicPressRelease {
  id: string; title: string; lead: string; body: string; published_at: string | null;
  release: { slug: string; title: string | null; cover: boolean } | null;
}
export interface PressDraft { release_id: string | null; title: string; lead: string; body: string; published: boolean }

export const EMPTY_PROFILE: PressProfile = { tagline: '', bio_short: '', genre: '', based_in: '', label: '', debut: '', highlights: [], quotes: [] };
export const LIMITS = { tagline: 120, bio_short: 400, genre: 60, based_in: 60, label: 80, highlight: 140, highlights: 8, quote: 300, source: 80, quotes: 6, title: 120, lead: 400, body: 8000 };

/** 초안에 넣는 안내 문장 — 공개하기 전에 남아 있으면 알려 준다 */
export const GUIDE_MARK = '(여기에';

// ---------------------------------------------------------------------------
// 초안 채우기
// ---------------------------------------------------------------------------
/** 이름 끝 글자에 받침이 있는지 — 한글이 아니면 받침 없는 것으로 (영문 이름은 대개 '가·를'로 읽힌다) */
function batchim(name: string): boolean {
  // 따옴표·괄호·기호는 건너뛰고 마지막 글자로 ('겨울 산책' → 책)
  const t = name.replace(/[^0-9A-Za-z\uAC00-\uD7A3]+$/u, '');
  const c = t.slice(-1).charCodeAt(0);
  if (c >= 0xac00 && c <= 0xd7a3) return (c - 0xac00) % 28 !== 0;
  if (/[0-9]$/.test(t)) return /[013678]$/.test(t); // 영·일·삼·육·칠·팔
  return /[lmnr]$/i.test(t);
}
export const josa = (name: string, withB: string, withoutB: string) => `${name}${batchim(name) ? withB : withoutB}`;

const KIND: Record<string, string> = { SINGLE: '싱글', EP: 'EP', ALBUM: '정규 앨범' };

export function koreanDate(d: string | null): string {
  if (!d) return '발매일 미정';
  const [y, m, day] = d.split('-').map(Number);
  return `${y}년 ${m}월 ${day}일`;
}

/** 발매 정보로 채운 보도자료 초안 — 지어내지 않고, 아티스트가 써야 할 곳은 안내 문장으로 남긴다 */
export function draftFor(input: {
  artist: string; title: string; type?: string | null; releaseDate: string | null; released: boolean;
  profile: PressProfile; link?: string; contact?: string;
}): Pick<PressDraft, 'title' | 'lead' | 'body'> {
  const kind = KIND[input.type ?? ''] ?? '새 음원';
  const when = koreanDate(input.releaseDate);
  const work = `${kind} '${input.title}'`;
  const verb = input.released ? '발매했다' : '발매한다';
  const lead = `${input.profile.tagline ? `${input.profile.tagline} ` : ''}${josa(input.artist, '이', '가')} ${input.releaseDate ? `${when} ` : ''}${josa(work, '을', '를')} ${verb}.`;
  const facts = [
    `- 아티스트: ${input.artist}`,
    `- 제목: ${input.title} (${kind})`,
    `- 발매일: ${when}`,
    input.profile.genre && `- 장르: ${input.profile.genre}`,
    input.profile.label && `- 소속: ${input.profile.label}`,
    input.link && `- 듣기: ${input.link}`,
  ].filter(Boolean).join('\n');
  // 첫 문단은 따로 두고 본문은 그다음부터 (전문 복사 때 두 번 나오지 않게)
  const body = [
    `${GUIDE_MARK} 이번 발매를 한두 문단으로 소개해 주세요 — 어떤 이야기를 담았는지, 어떻게 만들었는지, 함께한 사람이 있다면 누구인지.)`,
    input.profile.bio_short || `${GUIDE_MARK} 아티스트를 두세 문장으로 소개해 주세요.)`,
    `발매 정보\n${facts}`,
    input.contact ? `문의\n${input.contact}` : '',
  ].filter(Boolean).join('\n\n');
  return { title: `${input.artist}, ${work} ${input.released ? '발매' : '발매 예정'}`.slice(0, LIMITS.title), lead: lead.slice(0, LIMITS.lead), body: body.slice(0, LIMITS.body) };
}

/** 복사용 전문 — 제목·첫 문단·본문 */
export const fullText = (r: { title: string; lead: string; body: string }) => [r.title, r.lead, r.body].filter(Boolean).join('\n\n');

// ---------------------------------------------------------------------------
// 체험 모드
// ---------------------------------------------------------------------------
const mock: { profile: PressProfile; releases: PressRelease[] } = {
  profile: {
    tagline: '새벽의 도시를 노래하는 싱어송라이터',
    bio_short: '서울에서 활동하는 싱어송라이터. 도시의 풍경과 하루의 감정을 담담한 목소리로 기록한다.',
    genre: '인디 팝', based_in: '서울', label: '자체 레이블', debut: '2024',
    highlights: ['EP <여름의 끝> 멜론 인디 차트 3위', '2026 서울 인디 뮤직 페스티벌 출연'],
    quotes: [{ quote: '올해 가장 조용히, 오래 남는 목소리.', source: '음악 웹진 ○○' }],
  },
  releases: [{
    id: 'mock-pr-1', release_id: 'mock-1', release_title: '여름의 끝', title: "서린, EP '여름의 끝' 발매",
    lead: "새벽의 도시를 노래하는 싱어송라이터 서린이 2026년 8월 1일 EP '여름의 끝'을 발매했다.",
    body: "EP '여름의 끝'은 지나간 계절에 대한 여섯 곡의 기록이다.\n\n발매 정보\n- 아티스트: 서린\n- 제목: 여름의 끝 (EP)\n- 발매일: 2026년 8월 1일",
    published: true, published_at: '2026-08-01T09:00:00+09:00', created_at: '2026-07-28T10:00:00+09:00', updated_at: '2026-08-01T09:00:00+09:00',
  }],
};

export const MOCK_PUBLIC_PRESS = () => ({
  profile: mock.profile,
  releases: mock.releases.filter(r => r.published).map(r => ({
    id: r.id, title: r.title, lead: r.lead, body: r.body, published_at: r.published_at,
    release: r.release_id ? { slug: 'mock0001', title: r.release_title, cover: false } : null,
  })) as PublicPressRelease[],
});

// ---------------------------------------------------------------------------
// 서버
// ---------------------------------------------------------------------------
const base = (artistId: string) => orgPath(`/artists/${encodeURIComponent(artistId)}/link/press`);

export async function fetchPress(artistId: string): Promise<{ profile: PressProfile; releases: PressRelease[] }> {
  if (MOCK) { await new Promise(r => setTimeout(r, 200)); return { profile: { ...mock.profile }, releases: [...mock.releases] }; }
  return req(base(artistId), { quiet401: true });
}

export async function saveProfile(artistId: string, p: PressProfile): Promise<PressProfile> {
  if (MOCK) { mock.profile = { ...p, highlights: p.highlights.map(h => h.trim()).filter(Boolean), quotes: p.quotes.filter(q => q.quote.trim() || q.source.trim()) }; return mock.profile; }
  return req(base(artistId), { method: 'PUT', body: p });
}

export async function saveRelease(artistId: string, id: string | null, d: PressDraft, releaseTitle: string | null): Promise<PressRelease> {
  if (MOCK) {
    const now = new Date().toISOString();
    const prev = mock.releases.find(r => r.id === id);
    const item: PressRelease = {
      id: id ?? `mock-pr-${Date.now()}`, ...d, release_title: releaseTitle,
      published_at: d.published ? prev?.published_at ?? now : null, created_at: prev?.created_at ?? now, updated_at: now,
    };
    mock.releases = [item, ...mock.releases.filter(r => r.id !== item.id)];
    return item;
  }
  return id
    ? req(`${base(artistId)}/releases/${encodeURIComponent(id)}`, { method: 'PUT', body: d })
    : req(`${base(artistId)}/releases`, { method: 'POST', body: d });
}

export async function deleteRelease(artistId: string, id: string): Promise<void> {
  if (MOCK) { mock.releases = mock.releases.filter(r => r.id !== id); return; }
  await req(`${base(artistId)}/releases/${encodeURIComponent(id)}`, { method: 'DELETE' });
}
