// 배급 계약서 본문(서버 crates/core/src/agreement.rs가 만든 AUD-DIST 2.0) → 서류 화면용 조·별첨 구조
export type Block =
  | { kind: 'kv'; rows: [string, string][] }
  | { kind: 'p'; text: string }
  | { kind: 'list'; items: string[] }
  | { kind: 'check'; items: { text: string; required: boolean }[] };
export interface Article { no: string; title: string; blocks: Block[] }
export interface Parsed { meta: [string, string][]; intro: string[]; articles: Article[]; annexes: Article[] }

// 표로 보여 줄 ‘항목: 값’ 줄 — 문장 속 콜론(예: ‘배급수수료: 회사 … 중 회사 8%’)도 같은 표로
const KV = /^([^:：]{1,20}):\s+(.+)$/;

/** 서버 계약서 본문(AUD-DIST 2.0) → 조·별첨 단위 구조. 예전 1.0 본문은 문단으로만 나온다. */
export function parseAgreement(body: string): Parsed {
  const lines = body.split('\n').map(l => l.trim());
  const meta: [string, string][] = [];
  const intro: string[] = [];
  const articles: Article[] = [];
  const annexes: Article[] = [];
  let cur: Article | null = null;
  const push = (b: Block) => {
    if (!cur) return;
    const last = cur.blocks.at(-1);
    if (b.kind === 'kv' && last?.kind === 'kv') last.rows.push(...b.rows);
    else if (b.kind === 'list' && last?.kind === 'list') last.items.push(...b.items);
    else if (b.kind === 'check' && last?.kind === 'check') last.items.push(...b.items);
    else cur.blocks.push(b);
  };
  for (const [i, line] of lines.entries()) {
    if (!line || (i === 0 && /계약서$/.test(line))) continue;
    const art = /^제(\d+)조\s*\((.+)\)$/.exec(line);
    if (art) { cur = { no: art[1], title: art[2], blocks: [] }; articles.push(cur); continue; }
    const annex = /^별첨\s*(\d+)\s*·\s*(.+)$/.exec(line);
    if (annex) { cur = { no: `별첨 ${annex[1]}`, title: annex[2], blocks: [] }; annexes.push(cur); continue; }
    if (!cur) {
      const m = /^서식\s+(\S+(?:\s\d\.\d)?)\s*·\s*적용 약관\s+(.+)$/.exec(line);
      if (m) { meta.push(['서식', m[1]], ['적용 약관', m[2]]); continue; }
      const no = /^신청서 번호\s+(.+)$/.exec(line);
      if (no) { meta.push(['신청서 번호', no[1]]); continue; }
      intro.push(line);
      continue;
    }
    const check = /^□\s*\((필수|해당 시)\)\s*(.+)$/.exec(line);
    if (check) { push({ kind: 'check', items: [{ text: check[2], required: check[1] === '필수' }] }); continue; }
    const li = /^\d+\.\s+(.+)$/.exec(line);
    if (li) { push({ kind: 'list', items: [li[1]] }); continue; }
    const kv = KV.exec(line);
    if (kv && !/[.다]$/.test(kv[1])) { push({ kind: 'kv', rows: [[kv[1], kv[2]]] }); continue; }
    push({ kind: 'p', text: line });
  }
  return { meta, intro, articles, annexes };
}
