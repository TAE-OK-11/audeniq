import { describe, expect, it } from 'vitest';
import type { Work, WorkContributor } from '../api/publishing';
import { evenSplit, matchFilter, regStep, similarWorks, splitIssues, splitNames, splitReady, sumShares } from './publishing';

const person = (p: Partial<WorkContributor>): WorkContributor => ({
  id: p.name ?? 'x', creatorId: null, name: '서린', roles: ['composer'], share: 100, publisherShare: 0, confirm: 'confirmed', external: false, ...p,
});
const work = (p: Partial<Work>): Work => ({
  id: 'w', title: '파도', altTitles: [], workType: 'original', language: 'ko', genre: '', description: '', visibility: 'private', iswc: null, internalCode: 'AQW-1',
  hasPriorPublisher: false, contributors: [person({})], attachments: [], recordings: [], registration: 'none', registrationLog: [], application: null,
  contractId: null, fix: null, myRole: 'owner', activity: [], rowVersion: 1, createdAt: '', updatedAt: '', ...p,
});

describe('지분 확인', () => {
  it('합계가 100이면 신청할 수 있다', () => {
    expect(splitReady([person({ name: 'a', share: 60 }), person({ name: 'b', share: 40 })])).toBe(true);
  });
  it('소수점 나누기도 100으로 본다', () => {
    expect(sumShares([person({ share: 33.33 }), person({ share: 33.33 }), person({ share: 33.34 })])).toBe(100);
  });
  it('합계가 모자라거나 넘치면 막는다 — 저장은 따로 허용', () => {
    expect(splitIssues([person({ share: 90 })])[0]).toMatchObject({ level: 'error' });
    expect(splitIssues([person({ share: 70 }), person({ name: 'b', share: 40 })])[0].text).toContain('넘을 수 없어요');
  });
  it('퍼블리셔 관리 지분은 창작 지분보다 클 수 없다', () => {
    expect(splitReady([person({ share: 100, publisherShare: 120 })])).toBe(false);
  });
  it('확인 대기는 경고, 이의 제기는 막는다', () => {
    const pending = splitIssues([person({ name: 'a', share: 50 }), person({ name: 'b', share: 50, confirm: 'pending' })]);
    expect(pending.every(i => i.level === 'warn')).toBe(true);
    expect(splitReady([person({ name: 'a', share: 50 }), person({ name: 'b', share: 50, confirm: 'disputed' })])).toBe(false);
  });
  it('역할 없는 참여자, 빈 목록은 막는다', () => {
    expect(splitReady([person({ roles: [] })])).toBe(false);
    expect(splitReady([])).toBe(false);
  });
  it('균등 나누기는 합계가 정확히 100', () => {
    for (const n of [1, 2, 3, 6, 7]) expect(Math.round(evenSplit(n).reduce((a, b) => a + b, 0) * 100) / 100).toBe(100);
  });
});

describe('작품 거르기·찾기', () => {
  it('미공개 작품은 녹음이 없는 작품', () => {
    expect(matchFilter(work({}), 'unreleased')).toBe(true);
    expect(matchFilter(work({ recordings: [{ id: 'r', releaseId: 'r1', trackId: 't1', title: '파도', releaseTitle: 'EP' }] }), 'unreleased')).toBe(false);
  });
  it('보완 필요 · 관리 중', () => {
    expect(matchFilter(work({ registration: 'needs_fix' }), 'fix')).toBe(true);
    expect(matchFilter(work({ registration: 'needs_fix' }), 'managed')).toBe(false);
    expect(matchFilter(work({ registration: 'registered' }), 'managed')).toBe(true);
  });
  it('비슷한 제목을 찾는다 (띄어쓰기·괄호 무시)', () => {
    const list = [work({ id: 'a', title: '새벽 네 시' }), work({ id: 'b', title: '한낮' })];
    expect(similarWorks('새벽네시', list).map(w => w.id)).toEqual(['a']);
    expect(similarWorks('새벽 네 시', list, 'a')).toEqual([]);
  });
  it('크레딧 글자를 이름으로 나눈다', () => {
    expect(splitNames('서린, 김하늘 / 박 & 이온')).toEqual(['서린', '김하늘', '박', '이온']);
    expect(splitNames(null)).toEqual([]);
  });
  it('등록 단계', () => {
    expect(regStep('none')).toBe(-1);
    expect(regStep('needs_fix')).toBe(1);
    expect(regStep('registered')).toBe(5);
  });
});
