// 레이블 대량 작업 — 여러 발매의 메타데이터를 CSV(엑셀) 한 장으로 가져와 임시 저장 발매로 만들거나(대량 등록),
// 내보낸 CSV를 고쳐 다시 올려 아직 신청 전인 발매를 한 번에 고친다(일괄 수정).
// 한 줄 = 곡 하나. 같은 release_key(또는 release_id)의 줄이 한 발매다. 검사는 위자드와 같은 규칙을 쓰고,
// 저장할 때 서버가 다시 검사한다. 음원·커버·권리 확인·서명은 발매마다 위자드에서 마저 한다.
import type { DraftTrack, ReleaseDetail, ReleaseOptionsData, ReleasePayload } from '../api/types';
import { GENRES, KINDS, LANGUAGES } from './catalog';
import { artistIssue, isrcValid, minReleaseDate, normalizeIsrc, titleIssue, upcValid } from './dsp';
import { uid } from './store';

/** 열: [키, 한국어 머리글, 필수, 발매 단위(같은 발매의 줄은 같아야 함)] */
export const COLUMNS: { key: string; label: string; required?: boolean; release?: boolean; help: string }[] = [
  { key: 'release_id', label: '발매ID', release: true, help: '일괄 수정할 때만 (내보낸 파일에 들어 있어요)' },
  { key: 'release_key', label: '발매묶음', release: true, help: '같은 발매의 곡끼리 같은 값 (예: A1). 비우면 발매명+아티스트로 묶어요' },
  { key: 'release_title', label: '발매명', required: true, release: true, help: '앨범·싱글 제목' },
  { key: 'artist', label: '아티스트', required: true, release: true, help: '대표 아티스트명' },
  { key: 'type', label: '발매형태', release: true, help: 'single · ep · album · compilation (비우면 곡 수로 정해요)' },
  { key: 'release_date', label: '발매일', release: true, help: 'YYYY-MM-DD, 오늘부터 14일 뒤 이후' },
  { key: 'genre', label: '장르', release: true, help: '예: K-Pop, Ballad, Hip-Hop (목록에 없으면 그대로 기타 장르로)' },
  { key: 'language', label: '언어', release: true, help: 'ko · en · ja · other (비우면 ko)' },
  { key: 'label', label: '레이블', release: true, help: '레이블·기획사명' },
  { key: 'upc', label: 'UPC', release: true, help: '있을 때만 (없으면 자동 발급)' },
  { key: 'rights_holder', label: '권리자', release: true, help: '음원 마스터 권리자 (비우면 레이블 또는 아티스트)' },
  { key: 'track_no', label: '트랙번호', required: true, help: '1부터' },
  { key: 'track_title', label: '곡명', required: true, help: '피처링·버전 표기 없이' },
  { key: 'version', label: '버전', help: '예: Inst., Remix' },
  { key: 'featuring', label: '피처링', help: '피처링 아티스트 (쉼표로 구분)' },
  { key: 'isrc', label: 'ISRC', help: '있을 때만 (없으면 자동 발급)' },
  { key: 'composers', label: '작곡', help: '쉼표로 구분' },
  { key: 'lyricists', label: '작사', help: '쉼표로 구분 (연주곡이면 비움)' },
  { key: 'arrangers', label: '편곡', help: '쉼표로 구분' },
  { key: 'explicit', label: '19금', help: 'Y 또는 비움' },
];

export type Row = Record<string, string>;
export interface RowIssue { row: number; column: string; message: string }

const ALIASES: Record<string, string> = Object.fromEntries(COLUMNS.flatMap(c => [[c.key, c.key], [c.label, c.key]]));

// ---------------------------------------------------------------------------
// CSV
// ---------------------------------------------------------------------------
/** RFC 4180 (따옴표·줄바꿈·"" 이스케이프), BOM·CRLF 허용 */
export function parseCsv(text: string): string[][] {
  const src = text.replace(/^﻿/, '');
  const rows: string[][] = [];
  let row: string[] = [];
  let cell = '';
  let quoted = false;
  for (let i = 0; i < src.length; i++) {
    const c = src[i];
    if (quoted) {
      if (c === '"' && src[i + 1] === '"') { cell += '"'; i++; }
      else if (c === '"') quoted = false;
      else cell += c;
    } else if (c === '"' && cell === '') quoted = true;
    else if (c === ',') { row.push(cell); cell = ''; }
    else if (c === '\n' || c === '\r') {
      if (c === '\r' && src[i + 1] === '\n') i++;
      row.push(cell); rows.push(row); row = []; cell = '';
    } else cell += c;
  }
  if (cell !== '' || row.length) { row.push(cell); rows.push(row); }
  return rows.filter(r => r.some(v => v.trim() !== ''));
}

const cellOut = (v: string) => {
  // 엑셀 수식 실행 방지
  const s = /^[=+\-@\t\r]/.test(v) ? `'${v}` : v;
  return /[",\n\r]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
};
export function toCsv(rows: Row[]): string {
  const head = COLUMNS.map(c => c.label).join(',');
  const body = rows.map(r => COLUMNS.map(c => cellOut(r[c.key] ?? '')).join(','));
  return `﻿${[head, ...body].join('\r\n')}\r\n`;
}

/** 머리글(한국어 또는 영문 키)로 열을 찾아 줄을 만든다. 모르는 머리글은 오류. */
export function readRows(text: string): { rows: Row[]; issues: RowIssue[] } {
  const table = parseCsv(text);
  if (!table.length) return { rows: [], issues: [{ row: 0, column: '', message: '내용이 없어요.' }] };
  const head = table[0].map(h => h.trim());
  const issues: RowIssue[] = [];
  const keys = head.map(h => {
    const k = ALIASES[h];
    if (!k && h) issues.push({ row: 0, column: h, message: `알 수 없는 열이에요: ${h}` });
    return k;
  });
  for (const c of COLUMNS.filter(c => c.required)) {
    if (!keys.includes(c.key)) issues.push({ row: 0, column: c.key, message: `‘${c.label}’ 열이 없어요.` });
  }
  const rows = table.slice(1).map(cells => {
    const r: Row = {};
    keys.forEach((k, i) => { if (k) r[k] = (cells[i] ?? '').trim().replace(/^'(?=[=+\-@])/, ''); });
    return r;
  });
  if (rows.length > 2000) issues.push({ row: 0, column: '', message: '한 번에 2000줄까지 올릴 수 있어요.' });
  return { rows, issues };
}

// ---------------------------------------------------------------------------
// 검사
// ---------------------------------------------------------------------------
export const groupKey = (r: Row) => r.release_id || r.release_key || `${r.release_title}\u0000${r.artist}`;

/** 줄마다 위자드와 같은 규칙으로 검사한다. row는 머리글 다음 줄이 1. */
export function validate(rows: Row[], mode: 'create' | 'edit', today?: string): RowIssue[] {
  const out: RowIssue[] = [];
  const add = (i: number, column: string, message: string) => out.push({ row: i + 1, column, message });
  const minDate = minReleaseDate(false, today);
  const groups = new Map<string, number[]>();
  rows.forEach((r, i) => {
    for (const c of COLUMNS.filter(c => c.required)) if (!r[c.key]) add(i, c.key, `${c.label}을(를) 입력해 주세요.`);
    if (mode === 'edit' && !r.release_id) add(i, 'release_id', '일괄 수정은 내보낸 파일의 발매ID가 필요해요.');
    if (mode === 'create' && r.release_id) add(i, 'release_id', '새로 만들 때는 발매ID를 비워 주세요. 고치려면 ‘일괄 수정’을 써 주세요.');
    const t = titleIssue(r.release_title ?? '', 'release'); if (t) add(i, 'release_title', t);
    const tt = titleIssue(r.track_title ?? '', 'track'); if (tt) add(i, 'track_title', tt);
    const a = artistIssue(r.artist ?? ''); if (a) add(i, 'artist', a);
    if (r.type && !KINDS.some(k => k[0] === r.type)) add(i, 'type', 'single · ep · album · compilation 중에서 적어 주세요.');
    if (r.language && !LANGUAGES.some(l => l[0] === r.language)) add(i, 'language', 'ko · en · ja · other 중에서 적어 주세요.');
    if (r.release_date) {
      if (!/^\d{4}-\d{2}-\d{2}$/.test(r.release_date) || Number.isNaN(Date.parse(r.release_date))) add(i, 'release_date', 'YYYY-MM-DD 형식으로 적어 주세요.');
      else if (mode === 'create' && r.release_date < minDate) add(i, 'release_date', `발매일은 ${minDate} 이후로 적어 주세요 (플랫폼 납품·검수 2주).`);
    }
    if (r.upc && !upcValid(r.upc.replace(/\D/g, ''))) add(i, 'upc', 'UPC 체크 숫자가 맞지 않아요.');
    if (r.isrc && !isrcValid(r.isrc)) add(i, 'isrc', 'ISRC 형식이 맞지 않아요 (예: KRA012600001).');
    if (r.track_no && !/^\d{1,3}$/.test(r.track_no)) add(i, 'track_no', '트랙 번호는 숫자로 적어 주세요.');
    if (r.explicit && !/^(y|yes|n|no|o|x|19)$/i.test(r.explicit)) add(i, 'explicit', 'Y 또는 비워 주세요.');
    const g = groupKey(r);
    groups.set(g, [...(groups.get(g) ?? []), i]);
  });
  for (const idx of groups.values()) {
    const first = rows[idx[0]];
    for (const i of idx.slice(1)) {
      for (const c of COLUMNS.filter(c => c.release && c.key !== 'release_key' && c.key !== 'release_id')) {
        if ((rows[i][c.key] ?? '') !== (first[c.key] ?? '')) add(i, c.key, `같은 발매의 줄끼리 ${c.label} 값이 달라요 (${idx[0] + 1}번째 줄과 같게).`);
      }
    }
    const seen = new Map<string, number>();
    for (const i of idx) {
      const n = rows[i].track_no;
      if (n && seen.has(n)) add(i, 'track_no', `트랙번호 ${n}이(가) 같은 발매에 두 번 있어요.`);
      if (n) seen.set(n, i);
    }
    const isrcs = new Set<string>();
    for (const i of idx) {
      const v = rows[i].isrc ? normalizeIsrc(rows[i].isrc) : '';
      if (v && isrcs.has(v)) add(i, 'isrc', '같은 발매에 같은 ISRC가 두 번 있어요.');
      if (v) isrcs.add(v);
    }
  }
  return out;
}

// ---------------------------------------------------------------------------
// 발매 정보로 바꾸기
// ---------------------------------------------------------------------------
export interface Group { key: string; rows: { row: Row; line: number }[] }

export function groups(rows: Row[]): Group[] {
  const m = new Map<string, Group>();
  rows.forEach((row, i) => {
    const k = groupKey(row);
    if (!m.has(k)) m.set(k, { key: k, rows: [] });
    m.get(k)!.rows.push({ row, line: i + 1 });
  });
  for (const g of m.values()) g.rows.sort((a, b) => Number(a.row.track_no) - Number(b.row.track_no));
  return [...m.values()];
}

const yes = (v?: string) => /^(y|yes|o|19)$/i.test(v ?? '');

function genreFields(v: string): { genre: string; genreCustom: string } {
  if (!v) return { genre: '', genreCustom: '' };
  const hit = GENRES.find(g => g[0].toLowerCase() === v.toLowerCase() || g[1] === v);
  return hit && hit[0] && hit[0] !== '__other__' ? { genre: hit[0], genreCustom: '' } : { genre: '__other__', genreCustom: v };
}

function newTrack(r: Row): DraftTrack {
  return {
    id: uid('t'), title: r.track_title, version: r.version ?? '', isrc: r.isrc ? normalizeIsrc(r.isrc) : '',
    composers: r.composers ?? '', lyricists: r.lyricists ?? '', arrangers: r.arrangers ?? '', performers: '', producer: '',
    lyrics: '', audioName: '', audioSize: 0, explicit: yes(r.explicit), duration: '', featuring: r.featuring ?? '',
  };
}

const kind = (r: Row, n: number) => r.type || (n === 1 ? 'single' : n <= 6 ? 'ep' : 'album');

/** 새 발매(임시 저장)로 보낼 정보. 음원·커버는 비어 있다. */
export function createPayload(g: Group): ReleasePayload {
  const r = g.rows[0].row;
  const holder = r.rights_holder || r.label || r.artist;
  const year = (r.release_date || new Date().toISOString()).slice(0, 4);
  return {
    title: r.release_title, artist: r.artist, type: kind(r, g.rows.length), language: r.language || 'ko',
    ...genreFields(r.genre ?? ''), label: r.label ?? '', upc: (r.upc ?? '').replace(/\D/g, ''), notes: '',
    coverName: '', coverData: '', originalDate: '', release_date: r.release_date ?? '',
    tracks: g.rows.map(x => newTrack(x.row)), territories: ['WORLD'], platforms: [],
    ownership: holder, phonogram: holder ? `${year} ${holder}` : '', copyright: holder ? `${year} ${holder}` : '',
    rightsChecks: {}, options: {} as ReleaseOptionsData, lastStep: 0,
  };
}

/** 저장된 임시 발매를 다시 보낼 정보로 (일괄 수정의 바탕) */
export function payloadFromDetail(d: ReleaseDetail): ReleasePayload {
  const x = d.draft;
  return {
    title: d.title, artist: x?.artist ?? d.artist ?? '', type: x?.type ?? 'single', language: x?.language || 'ko',
    genre: x?.genre ?? '', genreCustom: x?.genreCustom ?? '', label: x?.label ?? '', upc: x?.upc ?? '', notes: x?.notes ?? '',
    coverName: x?.coverName ?? '', coverData: x?.coverData ?? '', coverAssetId: x?.coverAssetId, coverThumbAssetId: x?.coverThumbAssetId,
    originalDate: x?.originalDate ?? '', release_date: d.release_date ?? '', release_time: x?.release_time, preorder_start: x?.preorder_start,
    tracks: (x?.draftTracks ?? []).map(t => ({ ...t })), territories: x?.territories ?? ['WORLD'], platforms: x?.platforms ?? [],
    ownership: x?.ownership ?? '', phonogram: x?.phonogram ?? '', copyright: x?.copyright ?? '', rightsChecks: x?.rightsChecks ?? {},
    options: (x?.options ?? {}) as ReleaseOptionsData, lastStep: x?.lastStep, artistProfile: x?.artistProfile, application: x?.application,
  };
}

/** 내보낸 줄의 값으로 고친다: 발매 정보는 첫 줄, 곡은 트랙번호 순서대로 (없는 번호는 새 곡). 비운 칸은 지운다. */
export function applyEdit(p: ReleasePayload, g: Group): ReleasePayload {
  const r = g.rows[0].row;
  const tracks = [...p.tracks];
  for (const { row } of g.rows) {
    const i = Number(row.track_no) - 1;
    const base = tracks[i];
    const next: DraftTrack = base ? {
      ...base, title: row.track_title, version: row.version ?? '', isrc: row.isrc ? normalizeIsrc(row.isrc) : '',
      composers: row.composers ?? '', lyricists: row.lyricists ?? '', arrangers: row.arrangers ?? '',
      featuring: row.featuring ?? '', explicit: yes(row.explicit),
    } : newTrack(row);
    tracks[i] = next;
  }
  return {
    ...p, title: r.release_title, artist: r.artist, type: r.type || p.type, language: r.language || p.language,
    ...genreFields(r.genre ?? ''), label: r.label ?? '', upc: (r.upc ?? '').replace(/\D/g, ''),
    release_date: r.release_date ?? '', ownership: r.rights_holder || p.ownership,
    tracks: tracks.filter(Boolean),
  };
}

/** 임시 발매 → 내보낼 줄 */
export function rowsFromDetail(d: ReleaseDetail): Row[] {
  const p = payloadFromDetail(d);
  const genre = p.genre === '__other__' ? p.genreCustom : p.genre;
  const tracks = p.tracks.length ? p.tracks : [undefined];
  return tracks.map((t, i) => ({
    release_id: d.id, release_key: '', release_title: p.title, artist: p.artist, type: p.type, release_date: p.release_date,
    genre, language: p.language, label: p.label, upc: p.upc, rights_holder: p.ownership,
    track_no: t ? String(i + 1) : '', track_title: t?.title ?? '', version: t?.version ?? '', featuring: t?.featuring ?? '',
    isrc: t?.isrc ?? '', composers: t?.composers ?? '', lyricists: t?.lyricists ?? '', arrangers: t?.arrangers ?? '',
    explicit: t?.explicit ? 'Y' : '',
  }));
}

export const TEMPLATE_ROWS: Row[] = [
  { release_key: 'A1', release_title: '여름의 끝', artist: '한결', type: 'ep', release_date: '2027-01-15', genre: 'Indie Pop', language: 'ko', label: '한결 뮤직', track_no: '1', track_title: '여름의 끝', composers: '김한결', lyricists: '김한결', arrangers: '이편곡' },
  { release_key: 'A1', release_title: '여름의 끝', artist: '한결', type: 'ep', release_date: '2027-01-15', genre: 'Indie Pop', language: 'ko', label: '한결 뮤직', track_no: '2', track_title: '밤바다', version: 'Inst.', composers: '김한결', arrangers: '이편곡' },
  { release_key: 'B1', release_title: '첫눈', artist: '서린', release_date: '2027-02-01', genre: 'Ballad', label: '한결 뮤직', track_no: '1', track_title: '첫눈', composers: '서린', lyricists: '서린' },
];
