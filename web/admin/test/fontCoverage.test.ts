// 화면 문구에 쓴 한글이 글꼴 첫 조각(core)에 모두 있는지 — 없으면 그 글자만 두 번째 조각을 기다렸다 그려진다.
// 실패하면: cd ../studio/app && python3 build/fonts.py ../../admin (core를 다시 만들고 index.html 범위를 갱신)
import { describe, expect, it } from 'bun:test';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';

const APP = join(import.meta.dir, '..');
const walk = (dir: string, out: string[] = []) => {
  for (const n of readdirSync(dir)) {
    const p = join(dir, n);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (/\.(tsx?|txt)$/.test(n) && !/\.test\./.test(n)) out.push(p);
  }
  return out;
};

describe('글꼴 첫 조각', () => {
  it('화면 문구의 한글을 모두 담고 있다', () => {
    const core = new Set(readFileSync(join(APP, 'src/assets/fonts/core-hangul.txt'), 'utf8').trim());
    const missing = new Set<string>();
    for (const f of [...walk(join(APP, 'src')), join(APP, 'index.html')]) {
      for (const ch of readFileSync(f, 'utf8')) if (ch >= '가' && ch <= '힣' && !core.has(ch)) missing.add(ch);
    }
    expect([...missing].join('')).toBe('');
  });
});
