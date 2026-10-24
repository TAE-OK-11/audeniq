// 홈 추가 카드 데이터 — 홍보 성과(스마트링크), 플랫폼 반영, AUDENIQ 소식.
// 보여 줄 것이 없으면 그 값은 비어 있고 카드도 그리지 않는다 (링크가 없으면 홍보 성과 없음, 아직 보낸 곳이 없으면 반영 현황 없음).
// 실패하면 그 카드만 빠진다.
import { useEffect, useState } from 'react';
import type { Release } from '../api/types';
import { api } from '../api/client';
import { orgPath, req } from '../api/http';
import { fetchEvents, fetchNotices, loadContent } from '../api/content';
import { MOCK } from '../lib/mode';
import { deliverySummary, type DeliverySummary } from '../lib/delivery';
import { pickNews, pickPromo, promoCandidates, type NewsItem, type PromoPick, type SmartLinkStats } from '../lib/home';

export interface HomeCards {
  promo?: PromoPick;
  delivery?: DeliverySummary & { releaseId: string; title: string };
  news: NewsItem[];
}

interface SmartLinkState { link: { slug: string; enabled: boolean } | null; stats: SmartLinkStats }

/** 반영 현황은 배급을 승인한 뒤(전송 준비)부터 */
const DELIVERING = ['scheduled', 'live'];

export function useHomeCards(releases: Release[], loading: boolean, upcoming: Release | undefined): HomeCards {
  const [cards, setCards] = useState<HomeCards>({ news: [] });
  const key = releases.map(r => `${r.id}:${r.status}:${r.release_date ?? ''}`).join('|');

  useEffect(() => {
    if (loading) return;
    let alive = true;

    const promo = MOCK ? Promise.resolve(undefined) : Promise.all(promoCandidates(releases).map(r =>
      req<SmartLinkState>(orgPath(`/releases/${encodeURIComponent(r.id)}/smart-link`), { quiet401: true, quietServer: true })
        .then(s => ({ release: r, link: s.link, stats: s.stats }))
        .catch(() => undefined),
    )).then(list => pickPromo(list.filter((x): x is NonNullable<typeof x> => !!x)));

    // '다가오는 발매' 카드의 발매와 같은 발매 — 두 카드가 서로 다른 발매를 말하지 않게
    const target = upcoming && DELIVERING.includes(upcoming.status) ? upcoming : undefined;
    const delivery = target
      ? api.getDelivery(target.id)
        .then(items => {
          const sum = deliverySummary(items);
          return sum.total ? { ...sum, releaseId: target.id, title: target.title } : undefined;
        })
        .catch(() => undefined)
      : Promise.resolve(undefined);

    const news = Promise.all([
      loadContent(fetchNotices, n => n, []).catch(() => []),
      loadContent(fetchEvents, e => e, []).catch(() => []),
    ]).then(([n, e]) => pickNews(n, e));

    void Promise.all([promo, delivery, news]).then(([p, d, n]) => {
      if (alive) setCards({ promo: p, delivery: d, news: n });
    });
    return () => { alive = false; };
    // releases는 key로 본다
  }, [key, loading, upcoming?.id, upcoming?.status]); // eslint-disable-line react-hooks/exhaustive-deps

  return cards;
}
