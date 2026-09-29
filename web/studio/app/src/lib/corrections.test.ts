import { describe, expect, it } from 'vitest';
import { correctionWhere, fixPath, resolveCorrection, splitCorrections, WIZ_STEP } from './corrections';

describe('보완 요청 → 신청서 위치', () => {
  it('커버·크레딧·발매일 요청을 해당 단계와 입력칸으로 연결한다', () => {
    expect(resolveCorrection({ code: 'IMAGE_TOO_SMALL', message: '' })).toMatchObject({ step: WIZ_STEP.cover, field: 'coverFile' });
    expect(resolveCorrection({ code: 'S2_RELEASE_DATE_FAR_PAST', message: '' })).toMatchObject({ step: WIZ_STEP.distribution, field: 'f-releaseDate' });
    // 트랙 요청은 신청서의 트랙 순번으로 입력칸 id를 만든다
    expect(resolveCorrection({ code: 'S2_META_CREDITS', message: 'x', trackId: 'b' }, ['a', 'b'])).toMatchObject({ step: WIZ_STEP.tracks, field: 'tr-1-composers', message: 'x' });
  });

  it('모르는 코드는 최종 확인 단계로 보내고 기본 안내를 쓴다', () => {
    const r = resolveCorrection({ code: 'SOMETHING_NEW', message: '' });
    expect(r.step).toBe(WIZ_STEP.review);
    expect(r.field).toBeUndefined();
    expect(r.message).toBeTruthy();
  });

  it('보완 주소와 위치 표기', () => {
    expect(fixPath('r 1', { code: 'S2_META_CREDITS', message: '', trackId: 't7' })).toBe('/upload?edit=r+1&fix=S2_META_CREDITS&track=t7');
    expect(fixPath('r3')).toBe('/upload?edit=r3&fix=1');
    expect(correctionWhere(resolveCorrection({ code: 'IMAGE_NOT_SQUARE', message: '' }))).toBe('커버아트');
    expect(correctionWhere(resolveCorrection({ code: 'S2_META_CREDITS', message: '' }))).toBe('트랙 등록 · 크레딧');
  });
});

describe('splitCorrections', () => {
  it('담당자 전체 의견은 고칠 항목에서 빼고 따로 모은다', () => {
    const { items, note } = splitCorrections([
      { code: 'IMAGE_TOO_SMALL', message: '커버' },
      { code: 'REVIEW_NOTE', message: '  두 항목만 고쳐 주세요. ' },
      { code: 'S2_META_CREDITS', message: '작곡' },
    ]);
    expect(items.map(c => c.code)).toEqual(['IMAGE_TOO_SMALL', 'S2_META_CREDITS']);
    expect(note).toBe('두 항목만 고쳐 주세요.');
    expect(splitCorrections(undefined)).toEqual({ items: [], note: '' });
  });
});
