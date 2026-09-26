// 실제 백엔드 어댑터 — 메인 서버(Rust API) 계약을 화면 모델로 변환한다.
// 경로: 브라우저 → Cloudflare Worker(엣지) → Workers VPC / Cloudflare Tunnel → 메인 서버
//
// 백엔드 규칙 요약 (docs/API.md)
// - 로그인 응답에 csrf_token, 이후 모든 변경 요청에 X-CSRF-Token
// - 발매 = { name, release_type: SINGLE|EP|ALBUM, profile(JSON) }, row_version 낙관적 잠금
// - profile 문자열은 한 줄만 허용(키가 lyrics인 값만 여러 줄), 요청 본문 최대 64KB
// - 트랙은 아티스트 ID·자산 ID로 연결, 파일은 서명 URL로 R2에 직접 업로드
// - 접수 = 동의(consent) 생성 → submit(동의 ID, 자기 선언)
import type {
  Correction,
  DraftTrack, Org, PreflightIssue, Release, ReleaseDetail, ReleaseDraft, ReleasePayload,
  SaveResult, Track, UploadKind, UploadResult, User,
} from './types';
import { ApiError } from './errors';
import { bootstrapCsrf, hasCsrf, listAll, orgPath, putToGrant, req, setCsrf, type UploadGrant } from './http';
import { stampNow } from '../lib/date';
import { normalizeIsrc } from '../lib/dsp';

// ---------------------------------------------------------------------------
// 서버 모델
// ---------------------------------------------------------------------------
interface ServerRelease {
  id: string;
  title: string;
  release_type: 'SINGLE' | 'EP' | 'ALBUM';
  status: string;
  draft: Record<string, unknown> | null;
  row_version: number;
  created_at?: string;
  tracks?: ServerTrack[];
}
interface ServerTrack {
  id: string;
  title: string;
  disc_number: number;
  track_number: number;
  artist_id: string;
  asset_id: string | null;
  isrc: string | null;
  version?: string | null;
  lyrics?: string | null;
  parental_advisory?: boolean | null;
  credits?: ServerCredit[];
}
interface ServerCredit { party_id: string; role: string }

// ---------------------------------------------------------------------------
// 상태·유형 매핑
// ---------------------------------------------------------------------------
/** 서버 처리 단계(application_pipeline_status) → 화면 상태 칩 */
export function uiStatus(server: string): string {
  if (server === 'DRAFT' || server === 'WITHDRAWN' || server === 'SUPERSEDED') return 'draft';
  if (/CORRECTION$/.test(server) || server === 'ON_HOLD_RIGHTS') return 'needs';
  if (server === 'READY_FOR_DELIVERY') return 'scheduled';
  return 'review';
}
const EDITABLE = new Set(['DRAFT', 'STAGE1_CORRECTION', 'STAGE2_CORRECTION', 'STAGE3_CORRECTION']);

/** 서버는 UPC-A(12자리)만 받는다: 0으로 시작하는 EAN-13은 같은 번호라 앞 0을 뗀다. 비우면 3단계에서 발급 */
export function serverUpc(v: string): string | null {
  const d = v.trim();
  if (/^\d{12}$/.test(d)) return d;
  if (/^0\d{12}$/.test(d)) return d.slice(1);
  return null;
}
const toReleaseType = (t: string): ServerRelease['release_type'] => (t === 'ep' ? 'EP' : t === 'album' || t === 'compilation' ? 'ALBUM' : 'SINGLE');
const fromReleaseType = (t: string) => (t === 'EP' ? 'ep' : t === 'ALBUM' ? 'album' : 'single');

// ---------------------------------------------------------------------------
// profile 직렬화 — 서버 텍스트 정책에 맞게 정리
// ---------------------------------------------------------------------------
// eslint-disable-next-line no-control-regex
const FORBIDDEN = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f‪-‮⁦-⁩​⁠-⁤﻿­᠎]/g;

/** 문자열 정리: 금지 문자 제거, lyrics가 아니면 줄바꿈·탭을 공백으로 */
export function cleanText(s: string, multiline = false): string {
  const t = s.replace(FORBIDDEN, '');
  return multiline ? t.replace(/\r\n?/g, '\n').replace(/\t/g, ' ') : t.replace(/[\r\n\t]+/g, ' ').trim();
}

export function sanitizeProfile(v: unknown, key = ''): unknown {
  if (typeof v === 'string') return cleanText(v, key === 'lyrics');
  if (Array.isArray(v)) return v.map(x => sanitizeProfile(x, key));
  if (v && typeof v === 'object') {
    const out: Record<string, unknown> = {};
    for (const [k, x] of Object.entries(v)) {
      if (x === undefined) continue;
      out[cleanText(k)] = sanitizeProfile(x, k);
    }
    return out;
  }
  return v;
}

const PROFILE_LIMIT = 48 * 1024; // 요청 본문 64KB 한도 안에서 여유를 둔다

function buildProfile(data: ReleasePayload, prev: Record<string, unknown> | null, history: ReleaseDraft['history']) {
  const profile = {
    app: 'studio-react',
    schema: 1,
    artist: data.artist,
    type: data.type,
    language: data.language,
    genre: data.genre,
    genreCustom: data.genreCustom,
    label: data.label,
    upc: data.upc,
    // 앨범 소개는 여러 줄이라 줄 단위 배열로 보관 (서버는 lyrics 외 줄바꿈을 받지 않음)
    notes_lines: data.notes.split(/\r?\n/),
    coverName: data.coverName,
    coverData: data.coverData,
    coverAssetId: data.coverAssetId ?? (prev?.coverAssetId as string | undefined),
    originalDate: data.originalDate,
    release_date: data.release_date,
    territories: data.territories,
    platforms: data.platforms,
    ownership: data.ownership,
    phonogram: data.phonogram,
    copyright: data.copyright,
    // 서버 1차 검사·DDEX는 p_line/c_line을 읽는다 (화면 입력은 기호 없이 '연도 권리자명')
    p_line: data.phonogram,
    c_line: data.copyright,
    rightsChecks: data.rightsChecks,
    options: data.options,
    draftTracks: data.tracks,
    lastStep: data.lastStep,
    artistProfile: data.artistProfile,
    // 서명한 신청서는 한 번 기록되면 이후 임시 저장에서도 유지
    application: data.application ?? (prev?.application as ReleasePayload['application']),
    history,
    saved_at: stampNow(),
  };
  let clean = sanitizeProfile(profile) as Record<string, unknown>;
  // 너무 크면 가사 → 커버 썸네일 순으로 덜어낸다
  if (JSON.stringify(clean).length > PROFILE_LIMIT) {
    clean = { ...clean, draftTracks: (clean.draftTracks as DraftTrack[]).map(t => ({ ...t, lyrics: '' })) };
  }
  if (JSON.stringify(clean).length > PROFILE_LIMIT) clean = { ...clean, coverData: '' };
  if (JSON.stringify(clean).length > PROFILE_LIMIT) {
    throw new ApiError('저장할 정보가 너무 커요. 앨범 소개 길이를 줄여 주세요.', 413, 'PROFILE_TOO_LARGE');
  }
  return clean;
}

function readDraft(r: ServerRelease): ReleaseDraft {
  const p = (r.draft ?? {}) as Record<string, unknown>;
  const s = (k: string) => (typeof p[k] === 'string' ? p[k] as string : '');
  const a = <T,>(k: string) => (Array.isArray(p[k]) ? p[k] as T[] : []);
  return {
    artist: s('artist'),
    type: s('type') || fromReleaseType(r.release_type),
    language: s('language') || 'ko',
    genre: s('genre'),
    genreCustom: s('genreCustom'),
    label: s('label'),
    upc: s('upc'),
    notes: a<string>('notes_lines').join('\n') || s('notes'),
    coverName: s('coverName'),
    coverData: s('coverData'),
    coverAssetId: s('coverAssetId') || undefined,
    originalDate: s('originalDate'),
    territories: a<string>('territories'),
    platforms: a<string>('platforms'),
    ownership: s('ownership'),
    phonogram: s('phonogram'),
    copyright: s('copyright'),
    rightsChecks: (p.rightsChecks && typeof p.rightsChecks === 'object' ? p.rightsChecks : {}) as Record<string, boolean>,
    options: p.options as ReleaseDraft['options'],
    draftTracks: a<DraftTrack>('draftTracks'),
    lastStep: typeof p.lastStep === 'number' ? p.lastStep : undefined,
    artistProfile: p.artistProfile && typeof p.artistProfile === 'object' ? p.artistProfile as ReleaseDraft['artistProfile'] : undefined,
    application: p.application && typeof p.application === 'object' ? p.application as ReleaseDraft['application'] : undefined,
    history: a<{ text: string; time: string }>('history'),
  };
}

function durationMs(mmss?: string): number | null {
  const m = /^(\d+):(\d{2})$/.exec(mmss || '');
  return m ? (+m[1] * 60 + +m[2]) * 1000 : null;
}

function toSummary(r: ServerRelease): Release {
  const d = readDraft(r);
  return {
    id: r.id,
    title: r.title,
    status: uiStatus(r.status),
    release_date: (typeof r.draft?.release_date === 'string' && r.draft.release_date) || null,
    created_at: (r.created_at ?? '').slice(0, 10),
    updated_at: typeof r.draft?.saved_at === 'string' ? r.draft.saved_at : r.created_at,
    track_count: r.tracks ? r.tracks.length : (d.draftTracks?.filter(t => t.title.trim()).length ?? 0),
    artist: d.artist || undefined,
    coverData: d.coverData || undefined,
  };
}

function toDetail(r: ServerRelease): ReleaseDetail {
  const d = readDraft(r);
  const byServer = new Map((d.draftTracks ?? []).filter(t => t.serverId).map(t => [t.serverId!, t]));
  const tracks: Track[] = (r.tracks ?? []).map(t => {
    const local = byServer.get(t.id);
    return {
      id: t.id,
      title: t.title,
      duration_ms: durationMs(local?.duration),
      isrc: t.isrc,
      version: t.version ?? local?.version ?? null,
      composers: local?.composers ?? null,
      lyricists: local?.lyricists ?? null,
      audioName: local?.audioName || (t.asset_id ? '업로드한 음원' : null),
      explicit: !!t.parental_advisory,
      assetId: t.asset_id,
    };
  });
  return { ...toSummary(r), track_count: tracks.length, tracks, draft: d };
}

// ---------------------------------------------------------------------------
// 계정 상태
// ---------------------------------------------------------------------------
const ACCOUNT_KEY = 'aq.studio.v2.account';
let partyId = '';

function rememberEmail(email: string) {
  try { localStorage.setItem(ACCOUNT_KEY, JSON.stringify({ email })); } catch { /* 저장소 없음 */ }
}
function recallEmail(): string {
  try { return (JSON.parse(localStorage.getItem(ACCOUNT_KEY) || '{}') as { email?: string }).email ?? ''; } catch { return ''; }
}

// ---------------------------------------------------------------------------
// 아티스트 — 트랙은 아티스트 ID가 필요하므로 이름으로 찾거나 만든다
// ---------------------------------------------------------------------------
const artistCache = new Map<string, string>();
async function ensureArtist(name: string): Promise<string> {
  const n = cleanText(name) || '아티스트';
  const hit = artistCache.get(n);
  if (hit) return hit;
  const list = await listAll<{ id: string; name: string }>(orgPath('/artists'));
  for (const a of list) artistCache.set(a.name, a.id);
  const found = artistCache.get(n);
  if (found) return found;
  const created = await req<{ id: string }>(orgPath('/artists'), { method: 'POST', body: { name: n, profile: { app: 'studio-react' } } });
  artistCache.set(n, created.id);
  return created.id;
}

// ---------------------------------------------------------------------------
// 크레딧 — 작곡·작사 등 입력한 이름을 서버 파티로 만들고 트랙 크레딧으로 교체
// ---------------------------------------------------------------------------
const partyCache = new Map<string, string>();
async function ensureParty(name: string): Promise<string> {
  const path = orgPath('/parties');
  const key = `${path}\n${name}`;
  const hit = partyCache.get(key);
  if (hit) return hit;
  // 같은 이름이면 서버가 기존 파티를 돌려준다
  const r = await req<{ party_id: string }>(path, { method: 'POST', body: { display_name: name } });
  partyCache.set(key, r.party_id);
  return r.party_id;
}

// 역할명은 DDEX 표기 그대로 저장 (서버 1차 검사는 Composer·Lyricist를 작가 크레딧으로 본다)
const CREDIT_FIELDS: [keyof DraftTrack, string][] = [
  ['composers', 'Composer'], ['lyricists', 'Lyricist'], ['arrangers', 'Arranger'], ['producer', 'Producer'],
];

/** '홍길동, 김철수' → 이름 목록 (쉼표로 구분, 중복 제거) */
export function creditNames(v: unknown): string[] {
  if (typeof v !== 'string') return [];
  return [...new Set(v.split(/[,，、]/).map(s => cleanText(s)).filter(Boolean))];
}

async function wantedCredits(t: DraftTrack): Promise<ServerCredit[]> {
  const out: ServerCredit[] = [];
  for (const [field, role] of CREDIT_FIELDS) {
    if (field === 'lyricists' && t.instrumental) continue;
    for (const name of creditNames(t[field])) out.push({ party_id: await ensureParty(name), role });
  }
  return out;
}

const creditKey = (cs: ServerCredit[]) => cs.map(c => `${c.party_id}|${c.role}`).sort().join(',');

// ---------------------------------------------------------------------------
// 트랙 동기화 — 제목이 있는 트랙만 서버에 반영 (작성 중인 빈 트랙은 profile에만)
// ---------------------------------------------------------------------------
async function syncTracks(release: ServerRelease, tracks: DraftTrack[], artistId: string): Promise<{ rowVersion: number; tracks: DraftTrack[] }> {
  const base = orgPath(`/releases/${release.id}/tracks`);
  let rv = release.row_version;
  const server = new Map((release.tracks ?? []).map(t => [t.id, t]));
  const wanted = tracks.filter(t => t.title.trim());
  const keepIds = new Set(wanted.map(t => t.serverId).filter(Boolean));

  // 1) 지운 트랙 보관 처리 (자리 번호를 먼저 비운다)
  for (const t of release.tracks ?? []) {
    if (!keepIds.has(t.id)) {
      const r = await req<{ row_version: number }>(`${base}/${t.id}`, { method: 'DELETE', body: { row_version: rv } });
      rv = r.row_version;
    }
  }
  // 2) 순서대로 수정·추가
  const out: DraftTrack[] = [];
  let no = 0;
  for (const t of tracks) {
    if (!t.title.trim()) { out.push(t); continue; }
    no += 1;
    const body = {
      title: cleanText(t.title),
      disc_number: 1,
      track_number: no,
      artist_id: artistId,
      asset_id: t.assetId || null,
      row_version: rv,
      lyrics: t.lyrics ? cleanText(t.lyrics, true) : null,
      parental_advisory: t.explicit,
      version: cleanText(t.version) || null,
      // 없으면 3단계에서 발급
      isrc: normalizeIsrc(t.isrc ?? '') || null,
    };
    const existing = t.serverId ? server.get(t.serverId) : undefined;
    let trackId: string;
    if (existing) {
      const same = existing.title === body.title && existing.track_number === no && existing.asset_id === body.asset_id
        && (existing.version ?? null) === body.version && !!existing.parental_advisory === body.parental_advisory
        && (existing.lyrics ?? null) === body.lyrics && existing.artist_id === artistId
        && (existing.isrc ?? null) === body.isrc;
      if (!same) {
        const r = await req<{ row_version: number }>(`${base}/${existing.id}`, { method: 'PUT', body });
        rv = r.row_version;
      }
      trackId = existing.id;
      out.push(t);
    } else {
      const r = await req<{ id: string; row_version: number }>(base, { method: 'POST', body });
      rv = r.row_version;
      trackId = r.id;
      out.push({ ...t, serverId: r.id });
    }
    const credits = await wantedCredits(t);
    if (creditKey(credits) !== creditKey(existing?.credits ?? [])) {
      const r = await req<{ row_version: number }>(`${base}/${trackId}/credits`, { method: 'PUT', body: { row_version: rv, credits } });
      rv = r.row_version;
    }
  }
  return { rowVersion: rv, tracks: out };
}

function detailPath(id: string) { return orgPath(`/releases/${encodeURIComponent(id)}`); }

interface ServerCheck { check_code: string; status: string; severity?: string; detail?: string | null }

/** 보완 필요 발매의 검사 결과 중 사용자가 고쳐야 하는 항목 (접수 이력의 check_results) */
async function fetchCorrections(id: string): Promise<Correction[]> {
  try {
    const r = await req<{ checks?: ServerCheck[] }>(`${detailPath(id)}/submission`, { quiet401: true });
    const seen = new Set<string>();
    const out: Correction[] = [];
    for (const c of r.checks ?? []) {
      if (c.severity !== 'CORRECTION' && c.status !== 'CORRECTION_REQUIRED') continue;
      // 트랙별 검사는 detail에 'track=<id>'가 있다 → 그 트랙 입력칸으로 안내
      const trackId = /track=([0-9a-f-]{36})/.exec(c.detail ?? '')?.[1];
      const key = `${c.check_code}:${trackId ?? ''}`;
      if (seen.has(key)) continue;
      seen.add(key);
      // detail은 내부 기록용이라 화면에는 코드별 안내 문구를 쓴다
      out.push(trackId ? { code: c.check_code, message: '', trackId } : { code: c.check_code, message: '' });
    }
    return out;
  } catch {
    return [];
  }
}

async function withCorrections<T extends Release>(r: T): Promise<T> {
  if (r.status !== 'needs') return r;
  const corrections = await fetchCorrections(r.id);
  return corrections.length ? { ...r, corrections } : r;
}

async function fetchRelease(id: string): Promise<ServerRelease> {
  return req<ServerRelease>(detailPath(id));
}

/** 3단계가 읽는 발매 칸: UPC(없으면 발급)와 커버 이미지 */
function releaseRefs(data: ReleasePayload, prev: Record<string, unknown> | null) {
  const cover = data.coverAssetId ?? (typeof prev?.coverAssetId === 'string' ? prev.coverAssetId : undefined);
  return { upc: serverUpc(data.upc), artwork_asset_id: cover || null };
}

/** 발매 저장 공통 — 새로 만들거나, 편집 가능한 상태면 정보와 트랙을 갱신 */
async function persist(id: string | null, data: ReleasePayload, historyText: string | null): Promise<ServerRelease & { trackServerIds: Record<string, string> }> {
  const name = cleanText(data.title) || '제목 없는 발매';
  let rel: ServerRelease;
  if (!id) {
    const history = historyText ? [{ text: historyText, time: stampNow() }] : [];
    const created = await req<{ id: string; row_version: number }>(orgPath('/releases'), {
      method: 'POST',
      body: { name, release_type: toReleaseType(data.type), profile: buildProfile(data, null, history), ...releaseRefs(data, null) },
    });
    rel = await fetchRelease(created.id);
  } else {
    rel = await fetchRelease(id);
    if (!EDITABLE.has(rel.status)) {
      throw new ApiError('이미 접수된 발매는 여기서 수정할 수 없어요. 수정이 필요하면 문의로 요청해 주세요.', 409, 'NOT_EDITABLE');
    }
  }

  // 트랙 → 서버 트랙 ID를 받은 뒤 profile에 함께 저장
  const artistId = await ensureArtist(data.artist);
  const synced = await syncTracks(rel, data.tracks, artistId);
  const prevHistory = readDraft(rel).history;
  const history = historyText && id ? [...prevHistory, { text: historyText, time: stampNow() }] : prevHistory;
  const profile = buildProfile({ ...data, tracks: synced.tracks }, rel.draft, history);
  const r = await req<{ row_version: number }>(detailPath(rel.id), {
    method: 'PUT',
    body: { name, release_type: toReleaseType(data.type), profile, row_version: synced.rowVersion, ...releaseRefs(data, rel.draft) },
  });
  const trackServerIds: Record<string, string> = {};
  for (const t of synced.tracks) if (t.serverId) trackServerIds[t.id] = t.serverId;
  return { ...rel, title: name, draft: profile, row_version: r.row_version, trackServerIds };
}

// ---------------------------------------------------------------------------
// 사전 점검 문구
// ---------------------------------------------------------------------------
const PREFLIGHT_TEXT: Record<string, string> = {
  TRACK_REQUIRED: '트랙을 1곡 이상 등록해 주세요.',
  AUDIO_REQUIRED: '음원 파일이 없는 트랙이 있어요.',
  AUDIO_NOT_REGISTERED: '음원 업로드가 끝나지 않은 트랙이 있어요.',
  AUDIO_QC_PENDING_OR_BLOCKED: '음원 품질 검사가 끝나지 않았거나 통과하지 못한 트랙이 있어요.',
  NOT_DRAFT: '이미 접수된 발매예요.',
};
const BLOCKING = new Set(['TRACK_REQUIRED', 'AUDIO_REQUIRED', 'AUDIO_NOT_REGISTERED', 'NOT_DRAFT']);

// ---------------------------------------------------------------------------
// 공개 어댑터
// ---------------------------------------------------------------------------
export const remoteApi = {
  async login(email: string, password: string): Promise<User> {
    const r = await req<{ user_id: string; csrf_token: string }>('/api/auth/login', {
      method: 'POST', body: { email: email.trim(), password }, quiet401: true,
    });
    setCsrf(r.csrf_token);
    rememberEmail(email.trim());
    const me = await req<{ user_id: string; party_id: string }>('/api/me', { quiet401: true });
    partyId = me.party_id;
    return { id: r.user_id, email: email.trim() };
  },
  async signup(email: string, password: string): Promise<User> {
    await req('/api/auth/register', { method: 'POST', body: { email: email.trim(), password }, quiet401: true });
    return remoteApi.login(email, password);
  },
  async logout(): Promise<void> {
    try { await req('/api/auth/logout', { method: 'POST', quiet401: true }); } finally { setCsrf(''); partyId = ''; artistCache.clear(); partyCache.clear(); }
  },
  async me(): Promise<User> {
    const me = await req<{ user_id: string; party_id: string }>('/api/me', { quiet401: true });
    partyId = me.party_id;
    if (!hasCsrf()) await bootstrapCsrf();
    return { id: me.user_id, email: recallEmail() };
  },
  async listOrgs(): Promise<Org[]> {
    const r = await req<{ items: { id: string; name: string; role: string }[] }>('/api/orgs');
    // 발매 권한이 있는(OWNER/EDITOR) 작업 공간을 앞으로
    return r.items
      .slice()
      .sort((a, b) => Number(b.role !== 'VIEWER') - Number(a.role !== 'VIEWER'))
      .map(o => ({ id: o.id, name: o.name }));
  },
  async listReleases(): Promise<Release[]> {
    const items = await listAll<ServerRelease>(orgPath('/releases'));
    const list = await Promise.all(items.map(toSummary).map(withCorrections));
    return list.sort((a, b) => String(b.updated_at || '').localeCompare(String(a.updated_at || '')));
  },
  async getRelease(id: string): Promise<ReleaseDetail> {
    return withCorrections(toDetail(await fetchRelease(id)));
  },
  async saveDraft(id: string | null, data: ReleasePayload): Promise<SaveResult> {
    const rel = await persist(id, data, id ? null : '임시 저장 시작');
    return { ...toSummary(rel), trackServerIds: rel.trackServerIds };
  },
  /** 접수 전 서버 점검 — 막히는 항목을 한국어로 돌려준다 */
  async preflight(id: string): Promise<PreflightIssue[]> {
    const [r, rel] = await Promise.all([
      req<{ issues: { code: string; resource_id: string }[] }>(orgPath(`/releases/${id}/preflight`)),
      fetchRelease(id),
    ]);
    const titles = new Map((rel.tracks ?? []).map(t => [t.id, t.title]));
    return r.issues.map(i => ({ code: i.code, message: PREFLIGHT_TEXT[i.code] ?? i.code, trackTitle: titles.get(i.resource_id) }));
  },
  async submitRelease(id: string | null, data: ReleasePayload): Promise<Release> {
    const o = data.options;
    if (o.minor) throw new ApiError('미성년 아티스트 발매는 법정대리인 확인 절차가 서버에 준비되면 접수할 수 있어요. 지금은 임시 저장해 두고 문의로 알려 주세요.', 422, 'MINORITY_REVIEW_REQUIRED');
    const rel = await persist(id, data, null);
    // 1) 사전 점검
    const issues = (await remoteApi.preflight(rel.id)).filter(i => BLOCKING.has(i.code));
    if (issues.length) {
      const detail = issues.map(i => (i.trackTitle ? `‘${i.trackTitle}’ ${i.message}` : i.message)).join(' ');
      throw new ApiError(detail, 422, 'PREFLIGHT_FAILED');
    }
    // 2) 동의 — 본인(파티)이 권리자로서 현재 발매 내용 범위에 동의
    if (!partyId) await remoteApi.me();
    const consent = await req<{ consent_id: string }>(orgPath(`/releases/${rel.id}/consents`), {
      method: 'POST', body: { parties: [{ party_id: partyId, role: 'RIGHTS_HOLDER' }], minority_declared: false, valid_days: 365 },
    });
    // 3) 접수 — 같은 내용(row_version)으로 다시 눌러도 한 번만 처리되도록 멱등 키 사용
    const rc = data.rightsChecks;
    await req(orgPath(`/releases/${rel.id}/submit`), {
      method: 'POST',
      body: {
        consent_id: consent.consent_id,
        minority_declared: false,
        idempotency_key: `studio:${rel.id}:${rel.row_version}`,
        declarations: {
          rights_confirmed: !!(rc.rightsMaster && rc.rightsComposition && rc.rightsArtwork && rc.rightsConsent),
          adult_confirmed: !o.minor,
          is_cover: !!o.cover,
          is_remix: false,
          contains_samples: !!o.sample,
          ai_involved: !!o.ai,
          explicit_content: data.tracks.some(t => t.explicit),
        },
      },
    });
    // 4) 서명한 신청서를 서버에도 기록 — 신청서 번호·문서 확인 코드가 남고 배급 계약서 검토가 시작된다
    const app = data.application;
    if (app) {
      await req(orgPath(`/releases/${rel.id}/application`), {
        method: 'POST',
        body: {
          application_no: app.no, form: app.form, content_hash: app.hash, signer_name: app.signerName,
          signer_role: app.signerRole, agreements: app.agreements, signature: app.signature, submitted_at: app.submittedAt,
        },
      });
    }
    const fresh = await fetchRelease(rel.id);
    return { ...toSummary(fresh), updated_at: stampNow() };
  },
  async deleteRelease(id: string): Promise<void> {
    const rel = await fetchRelease(id);
    if (rel.status !== 'DRAFT') throw new ApiError('작성 중인 발매만 삭제할 수 있어요.', 409, 'NOT_DRAFT');
    await req(detailPath(id), { method: 'DELETE', body: { row_version: rel.row_version } });
  },
  /** 음원·커버를 R2에 직접 올리고 서버에 등록 */
  async uploadFile(file: File, kind: UploadKind, onProgress?: (r: number) => void, signal?: AbortSignal): Promise<UploadResult> {
    const contentType = uploadContentType(file, kind);
    if (!contentType) {
      throw new ApiError(kind === 'AUDIO' ? '음원은 WAV 또는 FLAC 파일만 올릴 수 있어요.' : kind === 'DOCUMENT' ? '서류는 PDF, JPG, PNG 파일만 올릴 수 있어요.' : '커버는 JPG 또는 PNG 파일만 올릴 수 있어요.', 400, 'UPLOAD_TYPE_UNSUPPORTED');
    }
    const issued = await req<{ upload_session_id: string; asset_id: string; expected_key: string; grant: UploadGrant }>(orgPath('/uploads'), {
      method: 'POST', body: { kind, size_bytes: file.size, content_type: contentType },
    });
    try {
      await putToGrant(issued.grant, file, onProgress, signal);
    } catch (e) {
      // 올리다 실패하면 업로드 세션을 취소해 둔다 (자산으로 등록되지 않도록)
      void req(orgPath(`/uploads/${issued.upload_session_id}/cancel`), { method: 'POST' }).catch(() => {});
      throw e;
    }
    const done = await req<{ asset_id: string; sha256?: string; detected_container?: string }>(orgPath(`/uploads/${issued.upload_session_id}/complete`), {
      method: 'POST', body: { asset_id: issued.asset_id, expected_key: issued.expected_key }, timeoutMs: 120000,
    });
    return { assetId: done.asset_id, sha256: done.sha256, container: done.detected_container };
  },
};

/** 서버가 받는 형식으로 content-type을 정한다 (브라우저가 비워 두는 경우 확장자로 판단) */
export function uploadContentType(file: File, kind: UploadKind): string {
  const name = file.name.toLowerCase();
  const t = file.type.toLowerCase();
  if (kind === 'AUDIO') {
    if (t === 'audio/wav' || t === 'audio/x-wav' || t === 'audio/wave' || name.endsWith('.wav')) return 'audio/wav';
    if (t === 'audio/flac' || t === 'audio/x-flac' || name.endsWith('.flac')) return 'audio/flac';
    return '';
  }
  if (kind === 'DOCUMENT' && (t === 'application/pdf' || name.endsWith('.pdf'))) return 'application/pdf';
  if (t === 'image/jpeg' || /\.jpe?g$/.test(name)) return 'image/jpeg';
  if (t === 'image/png' || name.endsWith('.png')) return 'image/png';
  return '';
}

