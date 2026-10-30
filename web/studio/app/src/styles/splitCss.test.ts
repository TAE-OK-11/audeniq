// styles/split/*.css는 한 화면 조각에서만 쓰는 규칙을 전역 스타일에서 떼어 낸 것이다 — 그 조각을 받을 때만 붙는다.
// 머리 주석의 anchors 클래스를 다른 파일에서도 쓰기 시작하면, 그 파일이 이 CSS를 import하지 않는 한
// 조각을 받기 전 화면에서는 스타일이 빠진다. 그런 파일이 있으면 실패한다
// (고치려면: 그 파일에서 split CSS를 import하거나, 규칙을 원래 전역 파일의 같은 자리로 되돌린다).
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

const SRC = join(__dirname, '..');
const SPLIT = join(__dirname, 'split');
const walk = (dir: string, out: string[] = []) => {
  for (const n of readdirSync(dir)) {
    const p = join(dir, n);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (/\.tsx?$/.test(n) && !/\.test\./.test(n)) out.push(p);
  }
  return out;
};
// 문자열·템플릿 안의 낱말 (className에 쓰이는 형태)
const words = (text: string) => {
  const out = new Set<string>();
  for (const lit of text.matchAll(/'(?:\\.|[^'\\\n])*'|"(?:\\.|[^"\\\n])*"|`(?:\\.|[^`\\])*`/g)) {
    for (const m of lit[0].matchAll(/[A-Za-z_][\w-]*/g)) out.add(m[0]);
  }
  return out;
};

const files = walk(SRC).map(f => {
  const text = readFileSync(f, 'utf8');
  return { f, text, words: words(text) };
});

describe('split CSS', () => {
  for (const name of readdirSync(SPLIT).filter(n => n.endsWith('.css'))) {
    it(`${name}: anchors를 쓰는 파일이 모두 불러온다`, () => {
      const css = readFileSync(join(SPLIT, name), 'utf8');
      const anchors = /anchors:([^*]*)\*\//.exec(css)?.[1].trim().split(/\s+/) ?? [];
      expect(anchors.length).toBeGreaterThan(0);
      const missing: string[] = [];
      for (const { f, text, words: w } of files) {
        const used = anchors.filter(a => w.has(a));
        if (!used.length) continue;
        let rel = relative(dirname(f), join(SPLIT, name)).split('\\').join('/');
        if (!rel.startsWith('.')) rel = './' + rel;
        if (!text.includes(`import '${rel}';`)) missing.push(`${relative(SRC, f)} (${used.join(' ')})`);
      }
      expect(missing).toEqual([]);
    });
  }
});
