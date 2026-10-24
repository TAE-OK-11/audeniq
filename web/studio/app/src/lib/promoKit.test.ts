import { describe, expect, it } from 'vitest';
import { caption, defaultStage, hashtag, hashtags } from './captions';
import { channelUrl } from './channels';
import { lyricLines } from './lyricCard';
import { ddayChoices, ddayKicker, ddayLabel } from './promoImage';

describe('D-day 이미지', () => {
  it('남은 날만 고를 수 있고 글자가 맞다', () => {
    expect(ddayChoices('2026-11-15', '2026-10-24')).toEqual([7, 3, 1, 0]);
    expect(ddayChoices('2026-10-26', '2026-10-24')).toEqual([1, 0]);
    expect(ddayChoices('2026-10-20', '2026-10-24')).toEqual([]);
    expect(ddayChoices(null)).toEqual([]);
    expect(ddayLabel(7)).toBe('D-7');
    expect(ddayLabel(0)).toBe('D-DAY');
    expect(ddayKicker('2026-11-15', 3, '18:00')).toBe('11월 15일 18:00 발매');
    expect(ddayKicker('2026-11-15', 0)).toBe('오늘 발매');
  });
});

describe('가사 카드', () => {
  it('빈 줄과 바로 이어 반복되는 줄을 뺀다', () => {
    expect(lyricLines('첫 줄\n\n  둘째 줄 \n둘째 줄\r\n셋째')).toEqual(['첫 줄', '둘째 줄', '셋째']);
  });
});

describe('홍보 문구', () => {
  const i = { title: '여름의 끝', artist: '한결 & 밴드', releaseDate: '2026-11-15', releaseTime: '18:00', url: 'https://s.test/l/abcd', kind: 'EP' };
  it('해시태그는 기호를 빼고 겹치지 않게', () => {
    expect(hashtag('한결 & 밴드')).toBe('#한결밴드');
    expect(hashtag('!!')).toBe('');
    expect(hashtags(i)).toBe('#한결밴드 #여름의끝 #신곡 #NewMusic');
  });
  it('단계별 문구에 링크와 날짜', () => {
    const t = caption(i, 'teaser', '2026-10-24');
    expect(t).toContain('한결 & 밴드 EP ‘여름의 끝’ 11월 15일 18:00 발매 (D-22)');
    expect(t).toContain('🎧 https://s.test/l/abcd');
    expect(caption(i, 'release')).toContain('오늘 발매됐어요');
    expect(caption({ ...i, title: '여름 EP' }, 'after')).toContain('한결 & 밴드 ‘여름 EP’');
    expect(defaultStage('2026-11-15', '2026-10-24')).toBe('teaser');
    expect(defaultStage('2026-10-24', '2026-10-24')).toBe('release');
    expect(defaultStage(null)).toBe('after');
  });
});

describe('채널별 링크', () => {
  it('?src= 를 붙인다', () => {
    expect(channelUrl('https://s.test/l/abcd', 'instagram')).toBe('https://s.test/l/abcd?src=instagram');
    expect(channelUrl('https://s.test/l/abcd?a=1', 'qr')).toBe('https://s.test/l/abcd?a=1&src=qr');
  });
});
