import { describe, expect, it } from 'vitest';
import type { Work, WorkContributor } from '../api/publishing';
import { addMonths, canEditInfo, editMode, evenSplit, isReleased, matchFilter, societyName, workOrigin, regStep, similarWorks, splitIssues, splitNames, splitReady, sumShares } from './publishing';

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

describe('곡 갈래 (퍼블리싱 접수 세 갈래)', () => {
  const rec = { id: 'r', releaseId: 'r1', trackId: 't1', title: '파도', releaseTitle: 'EP' };
  it('저장된 갈래를 그대로 읽는다', () => {
    expect(workOrigin(work({ origin: 'unreleased' }))).toBe('unreleased');
    expect(workOrigin(work({ origin: 'other_release', externalRelease: { artist: '이온', title: '겨울 편지' } }))).toBe('other_release');
  });
  it('갈래가 없는 예전 곡은 녹음·발매 정보로 읽는다', () => {
    expect(workOrigin(work({}))).toBe('unreleased');
    expect(workOrigin(work({ recordings: [rec] }))).toBe('own_release');
    expect(workOrigin(work({ externalRelease: { artist: '이온', title: '겨울 편지' } }))).toBe('other_release');
  });
  it('발매 전 곡에 내 발매 녹음이 붙으면 내가 발매한 곡이다', () => {
    expect(workOrigin(work({ origin: 'unreleased', recordings: [rec] }))).toBe('own_release');
  });
  it('저작권 관리는 발매된 곡만, 피칭은 발매 전 곡만', () => {
    expect(isReleased(work({ origin: 'unreleased' }))).toBe(false);
    expect(isReleased(work({ origin: 'other_release', externalRelease: { artist: '이온', title: '겨울 편지' } }))).toBe(true);
    expect(matchFilter(work({ origin: 'other_release', externalRelease: { artist: '이온', title: '겨울 편지' } }), 'unreleased')).toBe(false);
  });
});

describe('고칠 수 있는 때 (서버 update_work · put_rights와 같은 규칙)', () => {
  it('신청 전·보완·반려는 바로 고친다', () => {
    for (const r of ['none', 'preparing', 'needs_fix', 'rejected'] as const) expect(editMode(work({ registration: r }))).toBe('direct');
  });
  it('심사 중·서명 대기는 잠근다', () => {
    expect(editMode(work({ registration: 'internal_review' }))).toBe('locked');
    expect(editMode(work({ registration: 'ready' }))).toBe('locked');
  });
  it('계약 중·등록 완료는 지분을 변경 요청으로, 곡 정보는 등록이 끝난 뒤에만', () => {
    expect(editMode(work({ registration: 'ready', contractId: 'c1' }))).toBe('review');
    expect(canEditInfo(work({ registration: 'ready', contractId: 'c1' }))).toBe(false);
    expect(canEditInfo(work({ registration: 'registered', contractId: 'c1' }))).toBe(true);
  });
});

describe('계약 연장 종료일', () => {
  it('날짜 글자로 더한다 (하루 밀리지 않는다)', () => {
    expect(addMonths('2026-12-31', 12)).toBe('2027-12-31');
    expect(addMonths('2026-03-05', 6)).toBe('2026-09-05');
  });
  it('그 달에 없는 날은 그 달 마지막 날', () => {
    expect(addMonths('2026-01-31', 1)).toBe('2026-02-28');
    expect(addMonths('2027-11-30', 3)).toBe('2028-02-29');
  });
});

describe('곡 목록 필터', () => {
  it('미신청은 발매된 곡만 — 발매 전 곡은 발매 전으로', () => {
    expect(matchFilter(work({ origin: 'unreleased' }), 'none')).toBe(false);
    expect(matchFilter(work({ origin: 'unreleased' }), 'unreleased')).toBe(true);
    expect(matchFilter(work({ origin: 'other_release', externalRelease: { artist: '이온', title: '겨울 편지' } }), 'none')).toBe(true);
  });
});

describe('관리 단체 이름', () => {
  it('국내 두 단체는 정식 이름(약칭)으로, 나머지는 그대로', () => {
    expect(societyName('KOMCA')).toBe('한국음악저작권협회(KOMCA)');
    expect(societyName('함께하는음악저작인협회')).toBe('함께하는음악저작인협회(KOSCAP)');
    expect(societyName('JASRAC')).toBe('JASRAC');
    expect(societyName(null)).toBe('');
  });
});

describe('발매 신청서의 AI 답 → 곡의 AI 사용', () => {
  it('전체 AI 제작은 만듦, 작곡·작사 도움은 도움, 보컬·믹싱·커버만은 없음', async () => {
    const { releaseAiUse } = await import('../api/publishing');
    expect(releaseAiUse({ ai: true, aiUses: ['전체를 AI로 제작했어요'] })).toBe('generated');
    expect(releaseAiUse({ ai: true, aiUses: ['작사에 AI 도움을 받았어요'] })).toBe('assisted');
    expect(releaseAiUse({ ai: true, aiUses: ['믹싱·마스터링에 AI 도구를 썼어요', '커버아트를 AI로 만들었어요'] })).toBe('none');
    expect(releaseAiUse({ ai: false, aiUses: ['전체를 AI로 제작했어요'] })).toBe('none');
    expect(releaseAiUse(null)).toBe('none');
  });
});
