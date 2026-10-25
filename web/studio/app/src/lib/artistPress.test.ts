import { describe, expect, it } from 'vitest';
import { EMPTY_PROFILE, GUIDE_MARK, draftFor, fullText, josa } from './artistPress';

describe('보도자료 초안', () => {
  it('조사는 마지막 글자의 받침을 따른다 (따옴표·괄호는 건너뛴다)', () => {
    expect(josa("싱글 '겨울 산책'", '을', '를')).toBe("싱글 '겨울 산책'을");
    expect(josa("EP '여름의 끝'", '을', '를')).toBe("EP '여름의 끝'을");
    expect(josa('서린', '이', '가')).toBe('서린이');
    expect(josa('한결', '이', '가')).toBe('한결이');
    expect(josa('보라', '이', '가')).toBe('보라가');
    expect(josa("싱글 '1999'", '을', '를')).toBe("싱글 '1999'를");
  });

  it('발매 정보로 채우고, 아티스트가 쓸 곳은 안내 문장으로 남긴다', () => {
    const d = draftFor({
      artist: '서린', title: '겨울 산책', type: 'SINGLE', releaseDate: '2099-01-01', released: false,
      profile: { ...EMPTY_PROFILE, genre: '인디 팝' }, link: 'https://audeniq.com/link/release/abc', contact: 'a@b.test',
    });
    expect(d.title).toBe("서린, 싱글 '겨울 산책' 발매 예정");
    expect(d.lead).toBe("서린이 2099년 1월 1일 싱글 '겨울 산책'을 발매한다.");
    expect(d.body).toContain(GUIDE_MARK);
    expect(d.body).toContain('- 장르: 인디 팝');
    expect(d.body).toContain('- 듣기: https://audeniq.com/link/release/abc');
    expect(d.body).not.toContain(d.lead);
    expect(fullText(d).split(d.lead).length).toBe(2);
  });
});
