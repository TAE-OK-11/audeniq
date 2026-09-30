// 스튜디오 신청서 — 아티스트가 위자드에서 입력하고 서명한 내용(제출 리비전의 release.draft)을
// 심사 화면에서 그대로 보여 주기 위한 타입·표기·원본 확인.
// 스튜디오 web/studio/app/src/lib/application.ts · api/types.ts와 같은 규칙을 쓴다. 한쪽을 바꾸면 같이 맞춰 주세요.

export interface StudioDraftTrack {
  id?: string; serverId?: string; title?: string; version?: string; isrc?: string; featuring?: string;
  composers?: string; lyricists?: string; arrangers?: string; performers?: string; producer?: string;
  lyrics?: string; audioName?: string; audioSpec?: string; duration?: string;
  explicit?: boolean; instrumental?: boolean;
}

export interface StudioOptions {
  express?: boolean; expressAck?: boolean; expressReason?: string;
  minor?: boolean; guardian?: string; guardianRelation?: string; guardianContact?: string;
  guardian2?: string; guardian2Relation?: string; guardian2Contact?: string;
  guardianConsentDone?: boolean; familyCertName?: string; familyCertMethod?: string;
  cover?: boolean; coverTracks?: { trackId: string; originalTitle: string; originalArtist: string; originalWriters: string }[];
  coverRightsAck?: boolean; coverLicenseFile?: string;
  sample?: boolean; sampleLicenseFile?: string;
  featured?: boolean; featuredConsentFile?: string;
  ai?: boolean; aiTool?: string; aiUses?: string[]; aiTools?: string[]; aiUseOther?: string; aiToolOther?: string;
  shared?: boolean; sharedContractFile?: string;
  rerelease?: boolean; previousTitle?: string; previousId?: string;
}

export interface StudioApplication {
  no: string; form: string; submittedAt: string; signerName: string; signerRole: string;
  signature: string; hash: string; agreements: string[];
}

export interface StudioDraft {
  artist?: string; type?: string; language?: string; genre?: string; genreCustom?: string;
  label?: string; upc?: string; notes_lines?: string[]; notes?: string;
  coverName?: string; coverData?: string; originalDate?: string; release_date?: string;
  territories?: string[]; platforms?: string[];
  ownership?: string; phonogram?: string; copyright?: string;
  rightsChecks?: Record<string, boolean>;
  options?: StudioOptions | null;
  draftTracks?: StudioDraftTrack[];
  artistProfile?: { isNew: boolean; spotify?: string; apple?: string; melon?: string };
  application?: StudioApplication;
}

// ---------------------------------------------------------------------------
// 표기
// ---------------------------------------------------------------------------
const KINDS: Record<string, string> = { single: '싱글', ep: 'EP', album: '정규 앨범', compilation: '컴필레이션' };
const LANGUAGES: Record<string, string> = { ko: '한국어', en: '영어', ja: '일본어', other: '기타' };
export const kindLabel = (v?: string) => (v ? KINDS[v] ?? v : '');
export const languageLabel = (v?: string) => (v ? LANGUAGES[v] ?? v : '');
const GENRES: Record<string, string> = {
  Pop: '팝', 'Indie Pop': '인디 팝', Rock: '록', 'Indie Rock': '인디 록', Alternative: '얼터너티브', 'Hip-Hop': '힙합',
  'R&B / Soul': 'R&B / 소울', Electronic: '일렉트로닉', Dance: '댄스', Jazz: '재즈', Classical: '클래식', Folk: '포크',
  Acoustic: '어쿠스틱', Ballad: '발라드', Metal: '메탈', Punk: '펑크', Blues: '블루스', Reggae: '레게', Latin: '라틴',
  World: '월드', 'New Age': '뉴에이지', 'OST / Soundtrack': 'OST / 사운드트랙', Children: '어린이 음악',
  Religious: '종교음악', Ambient: '앰비언트', Instrumental: '연주곡', 'Spoken Word': '낭독 / 스포큰 워드',
};
/** 장르 — ‘기타 · 직접 입력’이면 아티스트가 적은 이름 */
export const genreLabel = (d: StudioDraft) =>
  d.genre === '__other__' ? (d.genreCustom?.trim() || '기타') : (d.genre ? GENRES[d.genre] ?? d.genre : '');

export const PROFILE_LINKS: { key: 'spotify' | 'apple' | 'melon'; label: string }[] = [
  { key: 'spotify', label: 'Spotify' }, { key: 'apple', label: 'Apple Music' }, { key: 'melon', label: '멜론' },
];

/** 부가서비스 (위자드 ‘부가서비스’) */
export const SERVICE_OPTIONS: [keyof StudioOptions, string][] = [['express', '신속 발매 요청']];
/** 해당 항목 (위자드 ‘해당하는 항목’) */
export const RIGHTS_OPTIONS: [keyof StudioOptions, string][] = [
  ['minor', '미성년 아티스트·권리자'], ['cover', '커버곡'], ['sample', '샘플링·타인 음원 사용'],
  ['featured', '피처링·공동 실연'], ['ai', 'AI 생성·보조 제작'], ['shared', '공동 권리자·레이블 계약'],
  ['rerelease', '기존 발매 이전·재발매'],
];
export const OPTION_LABELS: Record<string, string> = Object.fromEntries([...SERVICE_OPTIONS, ...RIGHTS_OPTIONS]);

/** 권리 확인 (위자드 마지막 단계 체크) */
export const RIGHTS_CHECKS: [string, string][] = [
  ['rightsMaster', '음원 마스터를 배급할 권한이 있어요.'],
  ['rightsComposition', '작사·작곡·편곡 등 저작물 이용에 필요한 허락을 확보했어요.'],
  ['rightsArtwork', '커버아트와 사용한 이미지·폰트에 필요한 이용 권한이 있어요.'],
  ['rightsConsent', '입력한 정보가 정확하며 필요한 권리 증빙을 요청받으면 제출할 수 있어요.'],
  ['rightsSamples', '샘플링·피처링 관련 제3자 권리 허락을 확보했어요.'],
  ['rightsAi', 'AI 생성·보조 제작물의 플랫폼 수용 기준을 확인했어요.'],
  ['rightsShared', '공동 권리자와의 배급 위임 범위를 확인했어요.'],
  ['rightsRerelease', '기존 발매와의 중복 송출 여부를 확인했어요.'],
];
/** 조건부 확인은 해당 항목을 골랐을 때만 필요 */
export function rightsChecksFor(o: StudioOptions | null | undefined): [string, string][] {
  const need: Record<string, boolean> = {
    rightsSamples: !!(o?.sample || o?.featured), rightsAi: !!o?.ai, rightsShared: !!o?.shared, rightsRerelease: !!o?.rerelease,
  };
  return RIGHTS_CHECKS.filter(([k]) => !(k in need) || need[k]);
}

export const AGREEMENTS: { id: string; text: string }[] = [
  { id: 'truth', text: '신청 내용이 사실이며, 음원·가사·커버아트·크레딧을 배급할 적법한 권리를 가지고 있음을 확인합니다.' },
  { id: 'terms', text: 'AUDENIQ 디지털 음원 배급 약관과 정산·수정·테이크다운 조건에 동의합니다.' },
  { id: 'privacy', text: '배급·정산을 위한 개인정보 수집·이용 및 음원 플랫폼 제공에 동의합니다.' },
  { id: 'esign', text: '이 전자서명이 자필 서명과 같은 효력을 가진다는 데 동의합니다.' },
];

/** 예전에 AQ-로 발급된 번호·서식도 AUD-로 표기 */
export const displayCode = (v: string) => v.replace(/^AQ-/, 'AUD-');
export const hashLabel = (h: string) => h.slice(0, 32).toUpperCase().match(/.{1,4}/g)?.join(' ') ?? '';
export const draftNotes = (d: StudioDraft) => (d.notes_lines?.join('\n') ?? d.notes ?? '').trim();

// ---------------------------------------------------------------------------
// 원본 확인 — 스튜디오가 서명할 때 계산한 SHA-256을 제출 내용으로 다시 계산해 비교
// ---------------------------------------------------------------------------
// eslint-disable-next-line no-control-regex
const FORBIDDEN = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f‪-‮⁦-⁩​⁠-⁤﻿­᠎]/g;
const c = (v: unknown) => (typeof v === 'string' ? v.replace(FORBIDDEN, '').replace(/[\r\n\t]+/g, ' ').trim() : '');
const OPTION_KEYS = ['express', 'minor', 'cover', 'sample', 'featured', 'ai', 'shared', 'rerelease'] as const;

function snapshot(d: StudioDraft, title: string) {
  const o = (d.options ?? {}) as Record<string, unknown>;
  const ap = d.artistProfile;
  return {
    title: c(title), artist: c(d.artist), type: c(d.type), language: c(d.language), genre: c(d.genre),
    label: c(d.label), upc: c(d.upc), releaseDate: c(d.release_date ?? ''), originalDate: c(d.originalDate),
    territories: [...(d.territories ?? [])], platforms: [...(d.platforms ?? [])],
    ownership: c(d.ownership), phonogram: c(d.phonogram), copyright: c(d.copyright),
    options: OPTION_KEYS.filter(k => o[k] === true),
    artistProfile: ap && !ap.isNew
      ? { isNew: false, spotify: c(ap.spotify), apple: c(ap.apple), melon: c(ap.melon) }
      : { isNew: true, spotify: '', apple: '', melon: '' },
    tracks: (d.draftTracks ?? []).filter(t => c(t.title)).map(t => ({
      title: c(t.title), version: c(t.version), featuring: c(t.featuring), isrc: c(t.isrc),
      composers: c(t.composers), lyricists: t.instrumental ? '' : c(t.lyricists), arrangers: c(t.arrangers),
      performers: c(t.performers), producer: c(t.producer), explicit: !!t.explicit, instrumental: !!t.instrumental,
      duration: c(t.duration), audioName: c(t.audioName), audioSpec: c(t.audioSpec),
    })),
  };
}

async function sha256Hex(text: string): Promise<string> {
  const buf = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(buf)].map(b => b.toString(16).padStart(2, '0')).join('');
}

export type Integrity = 'checking' | 'ok' | 'changed' | 'unsigned';

/**
 * 제출 리비전의 내용 + 서명 기록으로 해시를 다시 계산한다.
 * 서버가 접수 때 기록한 해시(recorded)와 같으면 서명한 내용 그대로 심사에 올라온 것.
 */
/** 신청 내용 + 서명 기록의 SHA-256 (스튜디오 createApplication과 같은 입력) */
export async function applicationHash(d: StudioDraft, title: string, a: Omit<StudioApplication, 'hash'>): Promise<string> {
  return sha256Hex(JSON.stringify({
    form: a.form, no: a.no, submittedAt: a.submittedAt, signerName: c(a.signerName), signerRole: a.signerRole,
    agreements: a.agreements, signature: a.signature, release: snapshot(d, title),
  }));
}

export async function verifyDraft(d: StudioDraft, title: string, recorded?: string | null): Promise<Integrity> {
  const a = d.application;
  if (!a?.hash) return 'unsigned';
  if (!globalThis.crypto?.subtle) return 'checking';
  const h = await applicationHash(d, title, a);
  return h === a.hash && (!recorded || recorded === a.hash) ? 'ok' : 'changed';
}

/**
 * 스튜디오 입력 기록(draft)이 없는 접수(스튜디오 밖에서 접수·예전 접수)도 신청서를 볼 수 있게
 * 서버 신청 정보로 같은 모양을 채운다.
 */
export function draftFromSheet(sheet: {
  release: { release_type: string };
  application: {
    artist?: string; genre?: string; release_date?: string; original_date?: string; label?: string;
    p_line?: string; c_line?: string; platforms: string[]; options?: object | null;
    tracks?: { id: string; title: string; version: string; isrc: string | null; parental_advisory: boolean }[] | null;
  };
}): StudioDraft {
  const a = sheet.application;
  const strip = (v?: string) => (v ?? '').replace(/^\s*[℗©]\s*/, '');
  return {
    artist: a.artist,
    type: ({ SINGLE: 'single', EP: 'ep', ALBUM: 'album' } as Record<string, string>)[sheet.release.release_type],
    genre: a.genre,
    label: a.label,
    release_date: a.release_date,
    originalDate: a.original_date,
    platforms: a.platforms,
    phonogram: strip(a.p_line),
    copyright: strip(a.c_line),
    options: (a.options ?? null) as StudioOptions | null,
    draftTracks: (a.tracks ?? []).map(t => ({
      id: t.id, serverId: t.id, title: t.title, version: t.version, isrc: t.isrc ?? '', explicit: t.parental_advisory,
    })),
  };
}
