// 트랙 크레딧 한 줄 — 작곡·작사가 같은 사람들이면 한 번만 적는다 (같은 이름을 두 번 늘어놓아 세 줄로 꺾였다)
const names = (v: string) => v.split(',').map(s => s.trim()).filter(Boolean);

export function creditParts(composers?: string | null, lyricists?: string | null): string[] {
  const c = names(composers ?? '');
  const l = names(lyricists ?? '');
  if (c.length && [...c].sort().join('\n') === [...l].sort().join('\n')) return [`작사·작곡 ${c.join(', ')}`];
  return [`작곡 ${c.join(', ') || '입력 전'}`, `작사 ${l.join(', ') || '입력 전'}`];
}
