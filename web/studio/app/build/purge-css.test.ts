import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import postcss from 'postcss';
import { describe, expect, it } from 'vitest';
import { purgeCss } from './purge-css';

function run(css: string, source: string) {
  const dir = mkdtempSync(join(tmpdir(), 'purge-'));
  writeFileSync(join(dir, 'a.tsx'), source);
  return postcss([purgeCss({ content: [dir] })]).process(css, { from: undefined }).css;
}

describe('purgeCss', () => {
  it('사용하지 않는 클래스 규칙과 keyframes를 지운다', () => {
    const out = run(
      '.used{a:1}.unused{b:2}@keyframes k1{to{c:3}}@keyframes k2{to{d:4}}.used{animation:k1 1s}',
      'const x = <div className="used" />; // unused 주석은 무시',
    );
    expect(out).toContain('.used');
    expect(out).not.toContain('.unused');
    expect(out).toContain('k1');
    expect(out).not.toContain('k2');
  });
  it('선택자 목록 중 사용되는 것만 남긴다', () => {
    expect(run('.a,.zzz{x:1}', "cls('a')")).toBe('.a{x:1}');
  });
  it('템플릿 접두사와 :not() 내부는 보존', () => {
    const out = run('.is-fwd{x:1}.btn:not(.nothere){y:2}', 'const c = `is-${dir}`; const b = "btn";');
    expect(out).toContain('.is-fwd');
    expect(out).toContain(':not(.nothere)');
  });
  it('요소·속성 선택자와 빈 @media 정리', () => {
    const out = run('button[aria-selected=true]{x:1}@media(min-width:1px){.gone{y:2}}', '');
    expect(out).toContain('button[aria-selected=true]');
    expect(out).not.toContain('@media');
  });
});
