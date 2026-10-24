// 트랙 크레딧 한 줄 — 작곡·작사가 같은 사람들이면 한 번만 적는다 (같은 이름을 두 번 늘어놓아 세 줄로 꺾였다)
const names = (v: string) => v.split(',').map(s => s.trim()).filter(Boolean);

export function creditParts(composers?: string | null, lyricists?: string | null): string[] {
  const c = names(composers ?? '');
  const l = names(lyricists ?? '');
  if (c.length && [...c].sort().join('\n') === [...l].sort().join('\n')) return [`작사·작곡 ${c.join(', ')}`];
  return [`작곡 ${c.join(', ') || '입력 전'}`, `작사 ${l.join(', ') || '입력 전'}`];
}

// ---------------------------------------------------------------------------
// 크레딧 자동완성 — 예전 발매에 쓴 이름을 칸 아래 칩으로 제안한다 (같은 팀과 계속 작업하면 매번 타이핑하지 않게,
// 철자가 발매마다 달라져 수정 요청을 받는 일도 줄인다)
// ---------------------------------------------------------------------------
export type CreditField = 'composers' | 'lyricists' | 'arrangers';
export type CreditHistory = Record<CreditField, string[]>;
export const CREDIT_FIELDS: CreditField[] = ['composers', 'lyricists', 'arrangers'];

const split = (v: string | null | undefined) => (v ?? '').split(/[,，、]/).map(s => s.trim()).filter(Boolean);

/** 예전 트랙들(최근 것 먼저) → 칸마다 자주 쓴 이름 순, 같으면 최근에 쓴 것 먼저 */
export function creditHistory(tracks: Partial<Record<CreditField, string | null>>[]): CreditHistory {
  const out = {} as CreditHistory;
  for (const f of CREDIT_FIELDS) {
    const seen = new Map<string, { n: number; first: number }>();
    tracks.forEach((t, i) => {
      for (const name of new Set(split(t[f]))) {
        const cur = seen.get(name);
        if (cur) cur.n++;
        else seen.set(name, { n: 1, first: i });
      }
    });
    out[f] = [...seen.entries()].sort((a, b) => b[1].n - a[1].n || a[1].first - b[1].first).map(([name]) => name);
  }
  return out;
}

/** 칸에 이미 있는 이름은 빼고 앞에서 n개 */
export function creditChips(history: string[], value: string, n = 5): string[] {
  const have = new Set(split(value).map(s => s.toLowerCase()));
  return history.filter(name => !have.has(name.toLowerCase())).slice(0, n);
}

/** 이름 하나 덧붙이기 — '서린' + 'Joseph K' → '서린, Joseph K' */
export function addCredit(value: string, name: string): string {
  const list = split(value);
  return [...list, name].join(', ');
}
