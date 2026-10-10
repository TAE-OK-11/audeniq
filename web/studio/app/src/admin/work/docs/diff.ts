// 버전 비교 — 줄 단위 차이(LCS). 문서 글자(plainText)를 비교하므로 서식 변경은 보이지 않고 내용만 본다.
// 아주 긴 문서는 앞뒤 같은 줄을 먼저 덜어 내고, 그래도 크면 통째로 '바뀜'으로 보여 준다(브라우저를 멈추지 않게).

export interface DiffLine { kind: 'same' | 'add' | 'del'; text: string }
const MAX_CELLS = 4_000_000;

export function diffLines(before: string, after: string): DiffLine[] {
  const a = before.split('\n');
  const b = after.split('\n');
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) { endA--; endB--; }
  const head = a.slice(0, start).map(text => ({ kind: 'same' as const, text }));
  const tail = a.slice(endA).map(text => ({ kind: 'same' as const, text }));
  const x = a.slice(start, endA);
  const y = b.slice(start, endB);
  if (x.length * y.length > MAX_CELLS) {
    return [...head, ...x.map(text => ({ kind: 'del' as const, text })), ...y.map(text => ({ kind: 'add' as const, text })), ...tail];
  }
  // LCS 길이 표 (뒤에서부터)
  const w = y.length + 1;
  const t = new Uint32Array((x.length + 1) * w);
  for (let i = x.length - 1; i >= 0; i--) {
    for (let j = y.length - 1; j >= 0; j--) {
      t[i * w + j] = x[i] === y[j] ? t[(i + 1) * w + j + 1] + 1 : Math.max(t[(i + 1) * w + j], t[i * w + j + 1]);
    }
  }
  const mid: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < x.length && j < y.length) {
    if (x[i] === y[j]) { mid.push({ kind: 'same', text: x[i] }); i++; j++; }
    else if (t[(i + 1) * w + j] >= t[i * w + j + 1]) { mid.push({ kind: 'del', text: x[i] }); i++; }
    else { mid.push({ kind: 'add', text: y[j] }); j++; }
  }
  while (i < x.length) mid.push({ kind: 'del', text: x[i++] });
  while (j < y.length) mid.push({ kind: 'add', text: y[j++] });
  return [...head, ...mid, ...tail];
}

export const diffStats = (d: DiffLine[]) => ({ added: d.filter(l => l.kind === 'add').length, removed: d.filter(l => l.kind === 'del').length });
