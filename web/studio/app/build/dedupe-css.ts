// 빌드 전용 — 묶인 CSS에서 늘 덮여 쓰이지 않는 선언을 지운다.
// 스타일 파일이 여러 겹(live → enhance → ui → design-align)으로 같은 규칙을 다시 정의해 와서, 앞 파일의 선언 상당수가
// 뒤 파일의 같은 선택자·같은 속성에 늘 덮인다. 파일마다 도는 PostCSS 단계에서는 다른 파일을 볼 수 없어
// 청크로 묶인 뒤(generateBundle)에 한 번 더 본다.
//
// 지우는 선언 — 아래가 모두 같을 때 앞 선언은 어떤 화면에서도 쓰이지 않는다(같은 우선순위면 뒤가 이긴다):
// - 선택자 문자열(공백 정리 후)과 감싼 @media·@supports 등의 조건 문자열이 모두 같다
// - 속성 이름이 같다
// - 뒤 선언이 !important이거나 앞 선언이 !important가 아니다
// 남기는 선언 — 오래된 브라우저용 대비 값일 수 있는 것:
// - 브라우저 접두사가 붙은 속성·값(-webkit- 등), 뒤 값이 새 단위·함수(dvh·color-mix 등)를 쓰는 경우,
//   브라우저가 값을 버릴 수 있는 line-clamp
import postcss, { type AtRule, type Declaration, type Root, type Rule } from 'postcss';
import type { Plugin as VitePlugin } from 'vite';

const MODERN = /\b(d|s|l)v[hwib]\b|\d(d|s|l)v(h|w|i|b|min|max)\b|color-mix\(|ok(lch|lab)\(|\blch\(|\blab\(|light-dark\(|anchor|calc-size\(|round\(|\bmod\(|\brem\(/;
const PREFIXED = /(^|[\s,(])-(webkit|moz|ms|o)-/;
const KEEP_PROP = /^-|^line-clamp$/;

function context(rule: Rule): string {
  const parts: string[] = [];
  for (let p = rule.parent; p && p.type !== 'root'; p = p.parent) {
    if (p.type === 'atrule') parts.push(`@${(p as AtRule).name} ${(p as AtRule).params.replace(/\s+/g, ' ').trim()}`);
  }
  return parts.reverse().join('|');
}

const norm = (sel: string) => sel.split(',').map(s => s.replace(/\s+/g, ' ').trim()).join(',');

/** 늘 덮이는 선언을 지우고, 비어 버린 규칙·@media를 정리한다. 지운 선언 수를 돌려준다 */
export function dropOverridden(root: Root): number {
  const decls: Declaration[] = [];
  root.walkDecls(d => {
    const rule = d.parent as Rule;
    if (rule?.type !== 'rule') return;
    const at = rule.parent as AtRule | undefined;
    if (at?.type === 'atrule' && /keyframes$/i.test(at.name)) return;
    decls.push(d);
  });
  // 뒤에서부터 보며 '이 자리에서 이기는 선언'을 기억한다
  const winner = new Map<string, Declaration>();
  const keys = new Map<Rule, string>();
  let dropped = 0;
  for (let i = decls.length - 1; i >= 0; i--) {
    const d = decls[i];
    const rule = d.parent as Rule;
    let rk = keys.get(rule);
    if (rk === undefined) keys.set(rule, rk = `${context(rule)}§${norm(rule.selector)}`);
    const k = `${rk}§${d.prop}`;
    const later = winner.get(k);
    if (!later) { winner.set(k, d); continue; }
    const overridden = later.important || !d.important;
    const fallback = KEEP_PROP.test(d.prop) || PREFIXED.test(d.value) || PREFIXED.test(later.value)
      || (MODERN.test(later.value) && !MODERN.test(d.value));
    if (overridden && !fallback) { d.remove(); dropped++; }
    else if (d.important && !later.important) winner.set(k, d);
  }
  root.walkRules(r => { if (!r.nodes.length) r.remove(); });
  // 비어 버린 @media 등 (안쪽부터 비므로 몇 번 돈다)
  for (let changed = true; changed;) {
    changed = false;
    root.walkAtRules(at => { if (at.nodes && at.nodes.length === 0) { at.remove(); changed = true; } });
  }
  return dropped;
}

/** 빌드 결과의 CSS 파일마다 dropOverridden을 돌린다 (선언 사이 공백·줄바꿈은 그대로) */
export function dedupeCss(): VitePlugin {
  return {
    name: 'aq-dedupe-css',
    apply: 'build',
    enforce: 'post',
    generateBundle(_, bundle) {
      for (const file of Object.values(bundle)) {
        if (file.type !== 'asset' || !file.fileName.endsWith('.css')) continue;
        const src = typeof file.source === 'string' ? file.source : new TextDecoder().decode(file.source);
        const root = postcss.parse(src);
        if (dropOverridden(root)) file.source = root.toString();
      }
    },
  };
}
