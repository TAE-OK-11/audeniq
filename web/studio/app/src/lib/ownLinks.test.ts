import { describe, expect, it } from 'vitest';
import { checkLinks, linkBrand, shortUrl } from './ownLinks';

describe('아티스트 페이지 링크', () => {
  it('로고는 아는 곳만', () => {
    expect(linkBrand('https://www.youtube.com/@serin')).toBe('youtube');
    expect(linkBrand('https://m.tiktok.com/@serin')).toBe('tiktok');
    expect(linkBrand('https://instagram.com/serin')).toBeUndefined();
    expect(linkBrand('not a url')).toBeUndefined();
  });
  it('짧은 주소', () => {
    expect(shortUrl('https://www.instagram.com/serin/')).toBe('instagram.com/serin');
  });
  it('빈 줄은 빼고, https와 이름을 확인', () => {
    expect(checkLinks([{ label: ' 인스타 ', url: 'https://instagram.com/x' }, { label: '', url: ' ' }]))
      .toEqual({ links: [{ label: '인스타', url: 'https://instagram.com/x' }] });
    expect(checkLinks([{ label: '예매', url: 'http://ticket.example' }]).error?.index).toBe(0);
    expect(checkLinks([{ label: '', url: 'https://a.example' }]).error?.message).toBe('링크 이름을 적어 주세요.');
  });
});
