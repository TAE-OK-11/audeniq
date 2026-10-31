// 다른 아티스트가 발매한 곡을 여러 곡 한 번에 등록 — 표(CSV) 한 장을 곡 입력값으로 바꾸고 줄마다 틀린 곳을 짚는다.
// 남에게 곡을 많이 써 준 작가가 한 곡씩 같은 칸을 반복해 채우지 않게 (Songtrust의 대량 등록과 같은 자리).
// 화면은 pages/WorkImport.tsx, 테스트는 workImport.test.ts
import type { ContributorRole, Creator, WorkContributor, WorkInput } from '../api/publishing';
import { uid } from './store';

export const IMPORT_COLUMNS = ['곡 제목', '발매 아티스트', '발매 제목', '발매일', 'ISRC', '레이블', '내 역할', '내 지분', '공동 작가'] as const;
export const IMPORT_SAMPLE = [
  IMPORT_COLUMNS.join(','),
  '겨울 편지,이온,겨울 편지,2025-11-20,KRA262500011,루미 엔터테인먼트,작사,30,"이온:작곡:70"',
  '봄비,하람,봄의 노래,2026-03-02,,,작사·작곡,100,',
].join('\n');

const ROLE: Record<string, ContributorRole> = { 작사: 'lyricist', 작곡: 'composer', 편곡: 'arranger', 기타: 'other' };
const roles = (s: string) => Array.from(new Set(s.split(/[·/,\s]+/).map(x => ROLE[x.trim()]).filter(Boolean)));
const ISRC = /^[A-Z]{2}-?[A-Z0-9]{3}-?\d{2}-?\d{5}$/i;

/** 따옴표·쉼표·줄바꿈을 지키는 작은 CSV 읽기 — 엑셀·구글 시트에서 내려받은 그대로 */
export function parseCsv(text: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let cell = '';
  let quoted = false;
  const src = text.replace(/^﻿/, '');
  for (let i = 0; i < src.length; i++) {
    const ch = src[i];
    if (quoted) {
      if (ch === '"' && src[i + 1] === '"') { cell += '"'; i++; } else if (ch === '"') quoted = false; else cell += ch;
    } else if (ch === '"') quoted = true;
    else if (ch === ',') { row.push(cell); cell = ''; }
    else if (ch === '\n' || ch === '\r') {
      if (ch === '\r' && src[i + 1] === '\n') i++;
      row.push(cell); cell = '';
      if (row.some(c => c.trim())) rows.push(row);
      row = [];
    } else cell += ch;
  }
  row.push(cell);
  if (row.some(c => c.trim())) rows.push(row);
  return rows;
}

export interface ImportRow { line: number; title: string; input: WorkInput | null; errors: string[]; total: number }

/** 표 → 곡 입력값. 첫 줄은 머리글(칸 이름으로 찾으니 순서가 달라도 된다). 내 줄은 작가 프로필의 이름·관리 단체·IPI로 */
export function importRows(text: string, me: Pick<Creator, 'id' | 'name' | 'society' | 'ipi'>): ImportRow[] {
  const [head, ...body] = parseCsv(text);
  if (!head) return [];
  const col = (name: string) => head.findIndex(h => h.trim() === name);
  const idx = Object.fromEntries(IMPORT_COLUMNS.map(c => [c, col(c)])) as Record<typeof IMPORT_COLUMNS[number], number>;
  return body.map((r, k) => {
    const get = (c: typeof IMPORT_COLUMNS[number]) => (idx[c] >= 0 ? (r[idx[c]] ?? '').trim() : '');
    const errors: string[] = [];
    const title = get('곡 제목');
    const artist = get('발매 아티스트');
    const date = get('발매일');
    const isrc = get('ISRC').toUpperCase();
    if (!title) errors.push('곡 제목이 비어 있어요');
    if (!artist) errors.push('발매 아티스트가 비어 있어요');
    if (date && !/^\d{4}-\d{2}-\d{2}$/.test(date)) errors.push('발매일은 2025-11-20 모양이에요');
    if (isrc && !ISRC.test(isrc)) errors.push('ISRC 모양이 아니에요');
    const myShare = Number(get('내 지분').replace('%', '')) || 0;
    const myRoles = roles(get('내 역할'));
    if (!myRoles.length) errors.push('내 역할(작사·작곡·편곡)을 적어 주세요');
    const people: WorkContributor[] = [{
      id: uid(), creatorId: me.id, name: me.name, roles: myRoles, society: me.society ?? null, ipi: me.ipi ?? null,
      share: myShare, publisherShare: 0, confirm: 'confirmed', external: false, isMe: true,
    }];
    for (const part of get('공동 작가').split(/[|;]/).map(x => x.trim()).filter(Boolean)) {
      const [name, role = '', share = '0'] = part.split(':').map(x => x.trim());
      if (!name) continue;
      people.push({ id: uid(), creatorId: null, name, roles: roles(role), society: null, ipi: null, share: Number(share.replace('%', '')) || 0, publisherShare: 0, confirm: 'pending', external: true });
    }
    const total = Math.round(people.reduce((n, c) => n + c.share, 0) * 100) / 100;
    if (total !== 100) errors.push(`지분 합계가 ${total}%예요 (저장은 되고, 신청 전에 맞춰요)`);
    const input: WorkInput | null = title && artist ? {
      title, altTitles: [], workType: 'original', language: 'ko', genre: '', description: '', visibility: 'private', iswc: null,
      hasPriorPublisher: false, priorPublisher: null, contributors: people, attachments: [], origin: 'other_release', aiUse: 'none',
      externalRelease: { artist, title: get('발매 제목') || title, releaseDate: date || null, isrc: isrc || null, label: get('레이블') || null },
    } : null;
    return { line: k + 2, title: title || '(제목 없음)', input, errors, total };
  });
}

/** 지분 합계만 틀린 줄은 등록할 수 있다 — 곡 화면에서 고친다 */
export const blocking = (r: ImportRow) => !r.input || r.errors.some(e => !e.startsWith('지분 합계'));
