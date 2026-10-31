import { describe, expect, it } from 'vitest';
import { IMPORT_SAMPLE, blocking, importRows, parseCsv } from './workImport';

const me = { id: 'cr_me', name: '서린', society: 'KOMCA', ipi: '00712345678' };

describe('곡 일괄 등록 표', () => {
  it('따옴표 안의 쉼표·줄바꿈을 지킨다', () => {
    expect(parseCsv('a,"b, c","d\n e"\n1,2,3')).toEqual([['a', 'b, c', 'd\n e'], ['1', '2', '3']]);
  });
  it('견본 표는 두 곡 모두 등록할 수 있다 — 내 줄은 작가 프로필로, 공동 작가는 확인 대기로', () => {
    const rows = importRows(IMPORT_SAMPLE, me);
    expect(rows.map(r => blocking(r))).toEqual([false, false]);
    const first = rows[0].input!;
    expect(first.origin).toBe('other_release');
    expect(first.externalRelease).toMatchObject({ artist: '이온', isrc: 'KRA262500011' });
    expect(first.contributors[0]).toMatchObject({ isMe: true, society: 'KOMCA', share: 30, roles: ['lyricist'] });
    expect(first.contributors[1]).toMatchObject({ name: '이온', roles: ['composer'], share: 70, confirm: 'pending' });
    expect(rows[1].input!.contributors[0].roles).toEqual(['lyricist', 'composer']);
  });
  it('필수 칸이 비면 막고, 지분 합계만 틀리면 등록은 된다', () => {
    const rows = importRows('곡 제목,발매 아티스트,내 역할,내 지분\n,하람,작사,100\n노을,하람,작곡,60', me);
    expect(blocking(rows[0])).toBe(true);
    expect(blocking(rows[1])).toBe(false);
    expect(rows[1].errors[0]).toContain('60%');
  });
});
