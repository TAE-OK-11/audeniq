import { describe, expect, it } from 'vitest';
import type { Release } from '../api/types';
import { dDayLabel, daysBetween, pickNews, pickPromo, pitchDaysLeft, promoCandidates, releaseStep, upcomingRelease } from './home';

const rel = (id: string, status: string, release_date: string | null): Release =>
  ({ id, title: id, status, release_date, created_at: '2026-10-01', track_count: 1 });

describe('홈 화면 계산', () => {
  it('날 수와 D-day', () => {
    expect(daysBetween('2026-10-24', '2026-11-09')).toBe(16);
    expect(daysBetween('2026-12-31', '2027-01-01')).toBe(1);
    expect(dDayLabel('2026-11-09', '2026-10-24')).toBe('D-16');
    expect(dDayLabel('2026-10-24', '2026-10-24')).toBe('오늘 발매');
    expect(dDayLabel('2026-10-22', '2026-10-24')).toBe('발매 3일째');
  });

  it('단계: 보완 필요는 검토, 작성 중은 단계 없음', () => {
    expect(releaseStep('ready')).toBe(0);
    expect(releaseStep('needs')).toBe(1);
    expect(releaseStep('scheduled')).toBe(2);
    expect(releaseStep('live')).toBe(3);
    expect(releaseStep('draft')).toBe(-1);
  });

  it('다가오는 발매: 접수 뒤 가장 가까운 발매일, 없으면 최근 2주 안에 나온 것', () => {
    const today = '2026-10-24';
    expect(upcomingRelease([
      rel('draft', 'draft', '2026-10-30'),
      rel('far', 'review', '2026-11-16'),
      rel('near', 'scheduled', '2026-11-09'),
      rel('nodate', 'ready', null),
    ], today)?.id).toBe('near');
    expect(upcomingRelease([rel('old', 'live', '2026-09-01'), rel('new', 'live', '2026-10-20')], today)?.id).toBe('new');
    expect(upcomingRelease([rel('old', 'live', '2026-09-01'), rel('d', 'draft', null)], today)).toBeUndefined();
  });

  it('피칭 마감은 발매일 lead일 전', () => {
    expect(pitchDaysLeft(16, 7)).toBe(9);
    expect(pitchDaysLeft(7, 7)).toBe(0);
  });

  it('홍보 성과: 공개 중인 링크 중 반응이 큰 것, 없으면 카드 없음', () => {
    const a = rel('a', 'live', '2026-10-01');
    const b = rel('b', 'live', '2026-10-10');
    expect(pickPromo([{ release: a, link: null, stats: { views: 0, clicks: {}, days: 30 } }])).toBeUndefined();
    expect(pickPromo([{ release: a, link: { slug: 'x', enabled: false }, stats: { views: 99, clicks: {}, days: 30 } }])).toBeUndefined();
    const p = pickPromo([
      { release: a, link: { slug: 'a1', enabled: true }, stats: { views: 10, clicks: { melon: 1 }, days: 30 } },
      { release: b, link: { slug: 'b1', enabled: true }, stats: { views: 40, clicks: { spotify: 5, melon: 2 }, days: 30 } },
    ]);
    expect(p).toMatchObject({ releaseId: 'b', slug: 'b1', views: 40, clicks: 7, topPlatform: 'spotify' });
    expect(promoCandidates([rel('d', 'draft', '2026-12-01'), a, b]).map(r => r.id)).toEqual(['b', 'a']);
  });

  it('소식: 진행 중 이벤트와 고정 공지만, 없으면 빈 목록', () => {
    expect(pickNews([{ id: 'n', title: '일반', pinned: false, published_at: '2026-10-01' }], [{ id: 'e', title: '끝남', status: 'ended', starts_on: '2026-09-01', ends_on: '2026-09-30' }])).toEqual([]);
    const news = pickNews(
      [{ id: 'n1', title: '정식 서비스', pinned: true, published_at: '2026-09-26T00:00:00Z' }],
      [{ id: 'e1', title: '런칭 프로모션', status: 'ongoing', starts_on: '2026-10-01', ends_on: '2026-10-31' }],
    );
    expect(news.map(n => [n.kind, n.title, n.meta])).toEqual([['event', '런칭 프로모션', '2026.10.31까지'], ['notice', '정식 서비스', '2026.09.26']]);
  });
});
