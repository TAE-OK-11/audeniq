import { describe, expect, it } from 'vitest';
import type { Royalty, Work, WorkContributor } from '../api/publishing';
import {
  albumReadiness, confirmationBoard, filterFromParams, filterRoyalties, filterToParams, reuseContributors, royaltyFilterLabel, splitDiff, workStage,
} from './publishingPrep';

const person = (p: Partial<WorkContributor>): WorkContributor => ({
  id: p.name ?? 'x', creatorId: null, name: 'x', roles: ['composer'], society: 'KOMCA', ipi: '00700000000', share: 100, publisherShare: 0,
  confirm: 'confirmed', external: false, ...p,
});
const work = (p: Partial<Work>): Work => ({
  id: 'w', title: '곡', altTitles: [], workType: 'original', language: 'ko', genre: '', description: '', visibility: 'private', iswc: null,
  internalCode: 'AQW-1', hasPriorPublisher: false, contributors: [person({ name: '서린', isMe: true })], attachments: [{ id: 'f', kind: 'demo', name: '데모', at: '' }], recordings: [],
  registration: 'none', registrationLog: [], application: null, contractId: null, fix: null, myRole: 'owner', activity: [], rowVersion: 1,
  createdAt: '', updatedAt: '', ...p,
});

describe('신청 전 준비 점검', () => {
  const ready = work({ id: 'a', title: '가' });
  const off = work({ id: 'b', title: '나', contributors: [person({ name: '서린', isMe: true, share: 60 })] });
  const waiting = work({ id: 'c', title: '다', contributors: [person({ name: '서린', isMe: true, share: 50 }), person({ name: '김하늘', share: 50, confirm: 'pending', ipi: null })] });
  const managed = work({ id: 'd', title: '라', registration: 'registered', contractId: 'c1' });

  it('곡마다 신청 가능·확인 필요·관리 중을 나눈다', () => {
    expect(workStage(ready)).toBe('ready');
    expect(workStage(off)).toBe('blocked');
    expect(workStage(waiting)).toBe('ready');
    expect(workStage(managed)).toBe('managed');
    expect(workStage(work({ myRole: 'contributor' }))).toBe('readonly');
    // 보완 요청은 '확인 필요'가 아니라 '보완 요청' — 앨범 머리 카드와 같은 말
    expect(workStage(work({ registration: 'needs_fix', application: { id: 'a', number: 'n', status: 'needs_fix' }, fix: { reason: 'IPI', fields: [], requestedAt: '' } }))).toBe('fix');
  });

  it('앨범 한 줄 요약과 항목별 할 일', () => {
    const r = albumReadiness([ready, off, waiting, managed], 1);
    expect(r.summary).toBe('신청 가능 2곡 · 지분 확인 필요 1곡 · 정리 전 1곡');
    const g = Object.fromEntries(r.groups.map(x => [x.key, x]));
    expect(g.split.works.map(w => w.id)).toEqual(['b']);
    expect(g.confirm.state).toBe('wait');
    expect(g.ipi.works.map(w => w.id)).toEqual(['c']);
    expect(g.tracks.state).toBe('todo');
  });

  it('창작 증빙이 없으면 신청할 수 없고, 가사·창작일·합의서는 권하기만 한다', async () => {
    const { workIssues } = await import('./publishingPrep');
    const bare = work({ id: 'e', attachments: [] });
    expect(workStage(bare)).toBe('blocked');
    expect(workIssues(bare).find(i => i.field === 'evidence')?.blocking).toBe(true);
    expect(albumReadiness([bare], 0).groups.find(g => g.key === 'evidence')?.works.map(w => w.id)).toEqual(['e']);
    const soft = workIssues(waiting).filter(i => i.field === 'evidence');
    expect(soft.every(i => !i.blocking)).toBe(true);
    expect(soft.map(i => i.text.slice(0, 6))).toEqual(['가사가 없어', '창작을 마친', '공동 작가가']);
    const done = work({ id: 'f', createdOn: '2026-03-02', attachments: [{ id: 'l', kind: 'lyrics', name: '가사', at: '' }] });
    expect(workIssues(done).filter(i => i.field === 'evidence')).toEqual([]);
  });
});

describe('참여자 확인 현황', () => {
  it('사람마다 묶고 가장 급한 상태를 고른다', () => {
    const a = work({ id: 'a', contributors: [person({ name: '김하늘', share: 40, confirm: 'pending' }), person({ name: '서린', isMe: true, share: 60 })] });
    const b = work({ id: 'b', contributors: [person({ name: '김하늘', share: 30, confirm: 'disputed' }), person({ name: '서린', isMe: true, share: 70 })] });
    const board = confirmationBoard([a, b]);
    expect(board.people[0]).toMatchObject({ name: '김하늘', state: 'disputed' });
    expect(board.people[0].entries).toHaveLength(2);
    expect(board.counts).toEqual({ confirmed: 1, pending: 0, disputed: 1 });
  });
});

describe('변경 전후', () => {
  it('바뀐 칸만 "60% → 50%"로', () => {
    const before = [person({ id: '1', name: '서린', share: 60 }), person({ id: '2', name: '김하늘', share: 40 })];
    const after = [person({ id: '1', name: '서린', share: 50 }), person({ id: '3', name: '박도윤', share: 50, roles: ['lyricist'] })];
    expect(splitDiff(before, after)).toEqual([
      { key: '1', name: '서린', kind: 'changed', lines: ['창작 지분 60% → 50%'] },
      { key: '3', name: '박도윤', kind: 'added', lines: ['새 참여자 · 작사 · 창작 지분 50%'] },
      { key: '2', name: '김하늘', kind: 'removed', lines: ['빠짐 · 창작 지분 40%'] },
    ]);
  });
});

describe('참여자 재사용', () => {
  it('역할·단체·IPI는 가져오고 지분은 비운다', () => {
    const from = [person({ id: 'a', name: '서린', isMe: true, share: 60, publisherShare: 60 }), person({ id: 'b', name: '이온', share: 40, ipi: '00755511122' })];
    const out = reuseContributors(from, [person({ id: 'k', name: '서린', isMe: true })]);
    expect(out.map(c => [c.name, c.share, c.ipi, c.confirm])).toEqual([['서린', 100, '00700000000', 'confirmed'], ['이온', 0, '00755511122', 'pending']]);
    expect(out[1].id).not.toBe('b');
  });
});

describe('저작권료 기간·조건', () => {
  const row = (id: string, period: string, source = 'KOMCA', country = 'KR', workId = 'w1'): Royalty => ({
    id, workId, workTitle: '곡', kind: 'digital', source, period, country, currency: 'KRW', collected: 0, share: 100, fee: 0, net: 100, status: 'confirmed',
  });
  const rows = [row('a', '2026-10'), row('b', '2026-04'), row('c', '2025-09', 'JASRAC', 'JP'), row('d', '2026-08', 'KOMCA', 'KR', 'w2')];
  const now = new Date('2026-10-30T12:00:00+09:00');

  it('최근 6개월·1년·직접 선택', () => {
    expect(filterRoyalties(rows, { period: '6m' }, now).map(r => r.id)).toEqual(['a', 'd']);
    expect(filterRoyalties(rows, { period: '12m' }, now).map(r => r.id)).toEqual(['a', 'b', 'd']);
    expect(filterRoyalties(rows, { period: 'custom', from: '2025-01', to: '2026-05' }, now).map(r => r.id)).toEqual(['b', 'c']);
  });
  it('곡·징수 단체·국가를 함께', () => {
    expect(filterRoyalties(rows, { period: 'all', source: 'JASRAC', country: 'JP' }, now).map(r => r.id)).toEqual(['c']);
    expect(filterRoyalties(rows, { period: 'all', work: 'w2' }, now).map(r => r.id)).toEqual(['d']);
    expect(royaltyFilterLabel({ period: '6m', source: 'KOMCA' }, { work: '파도' }, now)).toBe('최근 6개월 · 파도 · KOMCA');
  });
  it('주소와 오간다 (기본값은 주소에 남기지 않는다)', () => {
    const f = { period: 'custom' as const, from: '2026-01', to: '2026-06', source: 'KOMCA' };
    const p = new URLSearchParams(Object.entries(filterToParams(f)).filter(([, v]) => v !== null) as [string, string][]);
    expect(filterFromParams(p)).toMatchObject(f);
    expect(filterToParams({ period: '12m' }).period).toBeNull();
  });
});

describe('발매 크레딧 ↔ 곡 참여자', () => {
  it('크레딧에만 있는 이름과 참여자에만 있는 이름을 짚는다 (띄어쓰기·대소문자 무시)', async () => {
    const { creditGaps } = await import('./publishingPrep');
    const w = work({ contributors: [person({ name: '서린', isMe: true, roles: ['composer'] }), person({ name: '김 하늘', roles: ['lyricist'] }), person({ name: '박도윤', roles: ['arranger'] })] });
    expect(creditGaps(w, { composers: '서린, 이온', lyricists: '김하늘' })).toEqual({ missing: ['이온'], extra: ['박도윤'] });
    expect(creditGaps(w, { composers: '서린', lyricists: '없는사람', instrumental: true, arrangers: '박도윤' })).toEqual({ missing: [], extra: ['김 하늘'] });
    expect(creditGaps(w, {})).toEqual({ missing: [], extra: [] });
  });
});
