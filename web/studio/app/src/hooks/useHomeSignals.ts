// 홈 '확인할 작업'에 더하는 신호 — 피칭 마감, 공동작업자 분배 수락 대기, AI 앱 승인 요청.
// 같은 피칭 요청으로 진행 중인 피칭(홈 '플레이리스트 피칭' 카드)도 함께 읽는다.
// 각 화면(PitchCard·SplitsCard·AiConnections)이 쓰는 같은 API를 홈에서 몇 건만 가볍게 읽는다.
// 실패하면 그 신호만 빠진다 (홈은 다른 할 일을 그대로 보여 준다).
import { useEffect, useState } from 'react';
import type { Release } from '../api/types';
import { orgId, orgPath, req } from '../api/http';
import { aiActionRequests, aiPermissionRequests, aiSubmissions } from '../api/aiConnections';
import { MOCK } from '../lib/mode';
import { cacheAge, cacheGet, cacheSet } from '../lib/asyncCache';
import { shiftDate, todayStr } from '../lib/date';
import { pitchDaysLeft } from '../lib/home';

export interface PitchSignal { releaseId: string; title: string; daysLeft: number }
export interface SplitSignal { releaseId: string; title: string; waiting: number }
export interface AiSignal { count: number; to: string }
/** 보낸 피칭 — 취소한 것은 없는 것과 같다 */
export interface PitchActive { releaseId: string; releaseTitle: string; track: string; status: string; platforms: string[]; createdAt: string }
export interface HomeSignals { pitch?: PitchSignal; pitchActive?: PitchActive; splits: SplitSignal[]; ai?: AiSignal }

interface PitchState {
  pitch: { status: string; track_title: string; platforms: string[]; created_at: string } | null;
  can_pitch: boolean; days_until_release: number | null; lead_days: number;
}
interface SplitSheet { tracks: { shares: { status: 'INVITED' | 'ACCEPTED' | 'DECLINED' }[] }[] }

/** 서버에 몇 건까지만 물어본다 — 홈을 열 때마다 발매 수만큼 요청이 늘지 않게 */
const PITCH_CHECK = 3;
/** 피칭 결과는 발매 뒤에도 한 달쯤 본다 */
const PITCH_RESULT_CHECK = 2;
const PITCH_RESULT_DAYS = 30;
const SPLIT_CHECK = 5;

const EMPTY: HomeSignals = { splits: [] };
/** 홈에 다시 들어와도 이만큼 안에 받은 신호는 다시 묻지 않는다 — 무언가 바꾸면(lib/asyncCache) 바로 비워진다 */
export const HOME_FRESH_MS = 60_000;

export function useHomeSignals(releases: Release[], loading: boolean): HomeSignals {
  // 목록이 같으면 다시 묻지 않는다 (상태·발매일이 바뀌면 다시)
  const key = releases.map(r => `${r.id}:${r.status}:${r.release_date ?? ''}`).join('|');
  const cacheKey = `${orgId()}|home-signals|${key}`;
  // 다시 들어온 홈은 지난번 신호를 바로 — 카드가 뒤늦게 끼어들며 화면이 밀리지 않게
  const [signals, setSignals] = useState<HomeSignals>(() => cacheGet<HomeSignals>(cacheKey) ?? EMPTY);

  useEffect(() => {
    if (MOCK || loading) return;
    const cached = cacheGet<HomeSignals>(cacheKey);
    if (cached) setSignals(cached);
    if (cached && cacheAge(cacheKey) < HOME_FRESH_MS) return;
    let alive = true;
    const today = todayStr();
    // 피칭은 접수한 발매가 나오기 전에만 — 발매일이 가까운 것부터
    const pitchable = releases
      .filter(r => ['review', 'scheduled'].includes(r.status) && r.release_date && r.release_date > today)
      .sort((a, b) => (a.release_date ?? '').localeCompare(b.release_date ?? ''))
      .slice(0, PITCH_CHECK);
    // 분배 수락은 접수한 발매에서 — 작성 중이면 아직 메일이 가지 않았을 수 있다
    const shared = releases.filter(r => !['draft', 'cancelled', 'rejected', 'closed'].includes(r.status)).slice(0, SPLIT_CHECK);

    // 결과를 기다리는 피칭 — 최근 한 달 안에 나온 발매
    const since = shiftDate(today, -PITCH_RESULT_DAYS);
    const recent = releases
      .filter(r => r.status === 'live' && r.release_date && r.release_date <= today && r.release_date >= since)
      .sort((a, b) => (b.release_date ?? '').localeCompare(a.release_date ?? ''))
      .slice(0, PITCH_RESULT_CHECK);

    const pitchStates = Promise.all([...pitchable, ...recent].map(r =>
      req<PitchState>(orgPath(`/releases/${encodeURIComponent(r.id)}/pitch`), { quiet401: true, quietServer: true })
        .then(s => ({ r, s }))
        .catch(() => undefined),
    )).then(list => list.filter((x): x is { r: Release; s: PitchState } => !!x));

    const pitch = pitchStates.then(list => list
      .map(({ r, s }): PitchSignal | undefined => {
        const active = s.pitch && s.pitch.status !== 'WITHDRAWN';
        if (active || !s.can_pitch || s.days_until_release === null) return undefined;
        return { releaseId: r.id, title: r.title, daysLeft: pitchDaysLeft(s.days_until_release, s.lead_days) };
      })
      .filter((p): p is PitchSignal => !!p && p.daysLeft >= 0)
      .sort((a, b) => a.daysLeft - b.daysLeft)[0]);

    // 가장 최근에 보낸 피칭 하나
    const pitchActive = pitchStates.then(list => list
      .filter(({ s }) => s.pitch && s.pitch.status !== 'WITHDRAWN')
      .map(({ r, s }): PitchActive => ({
        releaseId: r.id, releaseTitle: r.title, track: s.pitch!.track_title, status: s.pitch!.status,
        platforms: s.pitch!.platforms ?? [], createdAt: s.pitch!.created_at,
      }))
      .sort((a, b) => b.createdAt.localeCompare(a.createdAt))[0]);

    const splits = Promise.all(shared.map(r =>
      req<SplitSheet>(orgPath(`/releases/${encodeURIComponent(r.id)}/splits`), { quiet401: true, quietServer: true })
        .then((s): SplitSignal => ({
          releaseId: r.id, title: r.title,
          waiting: s.tracks.reduce((n, t) => n + t.shares.filter(sh => sh.status === 'INVITED').length, 0),
        }))
        .catch(() => undefined),
    )).then(list => list.filter((s): s is SplitSignal => !!s && s.waiting > 0));

    const ai = Promise.all([
      aiActionRequests().catch(() => []),
      aiPermissionRequests().catch(() => []),
      aiSubmissions().catch(() => []),
    ]).then(([actions, perms, subs]): AiSignal | undefined => {
      const a = actions.filter(x => x.status === 'PENDING');
      const p = perms.filter(x => x.status === 'PENDING');
      const count = a.length + p.length + subs.length;
      if (!count) return undefined;
      // 한 건이면 그 승인 화면으로 바로, 여러 건이면 연결된 AI 앱 목록으로
      const to = count > 1 ? '/profile?open=ai' : a[0] ? `/ai/actions/${a[0].id}` : p[0] ? `/ai/approve/${p[0].id}` : '/profile?open=ai';
      return { count, to };
    });

    void Promise.all([pitch, pitchActive, splits, ai]).then(([p, pa, s, a]) => {
      const next = { pitch: p, pitchActive: pa, splits: s, ai: a };
      cacheSet(cacheKey, next);
      if (alive) setSignals(next);
    });
    return () => { alive = false; };
    // releases는 key로 본다
  }, [cacheKey, loading]); // eslint-disable-line react-hooks/exhaustive-deps

  return signals;
}
