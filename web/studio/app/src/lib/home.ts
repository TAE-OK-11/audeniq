// 홈 화면 계산 — 다가오는 발매, 발매 단계, 마감까지 남은 날 (pages/Dashboard.tsx).
import type { Release } from '../api/types';
import { todayStr } from './date';

/** 접수 → 검토 → 배급 승인 → 발매. 보완 필요는 검토 단계에 머문 것으로 본다 */
export const RELEASE_STEPS = ['접수', '검토', '배급 승인', '발매'] as const;
const STEP: Record<string, number> = { ready: 0, review: 1, needs: 1, scheduled: 2, live: 3 };

export function releaseStep(status: string): number {
  return STEP[status] ?? -1;
}

/** 'YYYY-MM-DD' 두 날짜 사이의 날 수 (b − a) */
export function daysBetween(a: string, b: string): number {
  const d = (s: string) => Date.UTC(+s.slice(0, 4), +s.slice(5, 7) - 1, +s.slice(8, 10));
  return Math.round((d(b) - d(a)) / 86400000);
}

/** D-17 · 오늘 발매 · 발매 3일째 */
export function dDayLabel(date: string, today = todayStr()): string {
  const n = daysBetween(today, date);
  if (n > 0) return `D-${n}`;
  if (n === 0) return '오늘 발매';
  return `발매 ${1 - n}일째`;
}

/** 발매 뒤 이만큼은 홈에 계속 둔다 (플랫폼에 다 올라왔는지 보는 기간) */
const LIVE_DAYS = 14;

/**
 * 홈에 크게 보여 줄 발매 하나 — 접수 뒤 아직 나오지 않은 것 중 발매일이 가장 가까운 것,
 * 없으면 최근 2주 안에 나온 것. 작성 중·종료·거절·취소는 보지 않는다.
 */
export function upcomingRelease(releases: Release[], today = todayStr()): Release | undefined {
  const pending = releases
    .filter(r => ['ready', 'review', 'needs', 'scheduled'].includes(r.status))
    .sort((a, b) => (a.release_date ?? '9999').localeCompare(b.release_date ?? '9999'));
  if (pending.length) return pending[0];
  return releases
    .filter(r => r.status === 'live' && r.release_date && daysBetween(r.release_date, today) <= LIVE_DAYS)
    .sort((a, b) => (b.release_date ?? '').localeCompare(a.release_date ?? ''))[0];
}

/** 피칭을 낼 수 있는 마지막 날까지 남은 날 — 발매일 lead일 전이 마감 */
export function pitchDaysLeft(daysUntilRelease: number, leadDays: number): number {
  return daysUntilRelease - leadDays;
}

// ---------------------------------------------------------------------------
// 홈 추가 카드 — 보여 줄 것이 없으면 카드를 그리지 않는다 (undefined)
// ---------------------------------------------------------------------------

/** 홍보 성과에 쓸 발매 — 접수 뒤 발매만, 최근(발매일이 늦은) 순으로 몇 개 */
export function promoCandidates(releases: Release[], n = 3): Release[] {
  return releases
    .filter(r => !['draft', 'cancelled', 'rejected', 'closed'].includes(r.status))
    .sort((a, b) => (b.release_date ?? '').localeCompare(a.release_date ?? ''))
    .slice(0, n);
}

export interface SmartLinkStats { views: number; clicks: Record<string, number>; days: number }
export interface PromoPick { releaseId: string; title: string; slug: string; views: number; clicks: number; days: number; topPlatform?: string }

/** 공개 중인 스마트링크 가운데 반응(조회+클릭)이 가장 큰 것 — 링크가 없거나 꺼져 있으면 고르지 않는다 */
export function pickPromo(list: { release: Release; link: { slug: string; enabled: boolean } | null; stats: SmartLinkStats }[]): PromoPick | undefined {
  const picks = list
    .filter(x => x.link?.enabled)
    .map(x => {
      const entries = Object.entries(x.stats.clicks ?? {}).filter(([, n]) => n > 0).sort((a, b) => b[1] - a[1]);
      return {
        releaseId: x.release.id, title: x.release.title, slug: x.link!.slug, views: x.stats.views ?? 0,
        clicks: entries.reduce((n, [, c]) => n + c, 0), days: x.stats.days ?? 30, topPlatform: entries[0]?.[0],
      };
    });
  return picks.sort((a, b) => (b.views + b.clicks) - (a.views + a.clicks))[0];
}

export interface NewsItem { kind: 'event' | 'notice'; id: string; title: string; meta: string; to: string }

/** 진행 중인 이벤트 하나 + 중요(고정) 공지 하나 — 둘 다 없으면 빈 목록 */
export function pickNews(
  notices: { id: string; title: string; pinned: boolean; published_at: string }[],
  events: { id: string; title: string; status: string; starts_on: string; ends_on: string | null }[],
): NewsItem[] {
  const out: NewsItem[] = [];
  const ev = events.filter(e => e.status === 'ongoing').sort((a, b) => (a.ends_on ?? '9999').localeCompare(b.ends_on ?? '9999'))[0];
  if (ev) out.push({ kind: 'event', id: ev.id, title: ev.title, meta: ev.ends_on ? `${ev.ends_on.replace(/-/g, '.')}까지` : '진행 중', to: '/events' });
  const no = notices.filter(n => n.pinned).sort((a, b) => b.published_at.localeCompare(a.published_at))[0];
  if (no) out.push({ kind: 'notice', id: no.id, title: no.title, meta: no.published_at.slice(0, 10).replace(/-/g, '.'), to: `/notices/${encodeURIComponent(no.id)}` });
  return out;
}
