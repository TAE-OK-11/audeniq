// 빌드 전용 PostCSS 플러그인 — 소스(ts/tsx/html)에 등장하지 않는 클래스·ID를 쓰는 규칙과
// 참조되지 않는 @keyframes를 제거한다. live.css는 랜딩 페이지까지 포함한 원본 포팅이라
// 스튜디오 앱에서 쓰지 않는 규칙이 많다.
//
// 보수적 규칙:
// - 소스 문자열 리터럴(따옴표·템플릿) 안의 식별자 형태 토큰을 사용 목록으로 본다.
// - `is-${x}`처럼 템플릿으로 조립되는 클래스는 'is-' 같은 '-'로 끝나는 조각을 접두사로 인정한다.
// - :not()/:is()/:where() 안의 클래스는 판정에서 제외한다(지우면 안 되는 규칙을 지키기 위해).
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import type { AtRule, Plugin, Root, Rule } from 'postcss';

function walk(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (/\.(tsx?|html)$/.test(name) && !/\.test\.tsx?$/.test(name)) out.push(p);
  }
  return out;
}

export function collectTokens(paths: string[]): { exact: Set<string>; prefixes: string[] } {
  const exact = new Set<string>();
  for (const root of paths) {
    const files = statSync(root).isDirectory() ? walk(root) : [root];
    for (const f of files) {
      const text = readFileSync(f, 'utf8');
      // 클래스·ID는 항상 문자열(따옴표·템플릿) 안에 있으므로 문자열 리터럴만 본다 (주석·변수명 제외).
      // HTML은 속성값이 모두 문자열이라 전체를 본다.
      const chunks = f.endsWith('.html')
        ? [text]
        : [...text.matchAll(/'(?:\\.|[^'\\\n])*'|"(?:\\.|[^"\\\n])*"|`(?:\\.|[^`\\])*`/g)].map(m => m[0]);
      for (const c of chunks) for (const m of c.matchAll(/[A-Za-z_][\w-]*/g)) exact.add(m[0]);
    }
  }
  const prefixes = [...exact].filter(t => t.endsWith('-') && t.length >= 3);
  return { exact, prefixes };
}

const STRIP_PSEUDO = /:(not|is|where)\((?:[^()]|\([^()]*\))*\)/g;

function selectorUsed(sel: string, used: (name: string) => boolean): boolean {
  const s = sel.replace(STRIP_PSEUDO, '').replace(/\[[^\]]*\]/g, '');
  for (const m of s.matchAll(/[.#]((?:\\.|[\w-])+)/g)) {
    const name = m[1].replace(/\\/g, '');
    if (!used(name)) return false;
  }
  return true;
}

export function purgeCss(opts: { content: string[]; safelist?: RegExp[] }): Plugin {
  let tokens: ReturnType<typeof collectTokens> | null = null;
  const used = (name: string) => {
    tokens ??= collectTokens(opts.content);
    if (tokens.exact.has(name)) return true;
    if (opts.safelist?.some(r => r.test(name))) return true;
    return tokens.prefixes.some(p => name.startsWith(p));
  };
  return {
    postcssPlugin: 'aq-purge-css',
    OnceExit(root: Root) {
      root.walkRules((rule: Rule) => {
        const parent = rule.parent as AtRule | undefined;
        if (parent?.type === 'atrule' && /keyframes$/i.test(parent.name)) return;
        const keep = rule.selectors.filter(s => selectorUsed(s, used));
        if (!keep.length) rule.remove();
        else if (keep.length !== rule.selectors.length) rule.selectors = keep;
      });
      // 비어 버린 @media 등 정리
      root.walkAtRules((at: AtRule) => {
        if (at.nodes && at.nodes.length === 0) at.remove();
      });
      // 사용되지 않는 @keyframes 제거
      const animUsed = new Set<string>();
      root.walkDecls(/^(-webkit-)?animation(-name)?$/, d => {
        for (const m of d.value.matchAll(/[A-Za-z_][\w-]*/g)) animUsed.add(m[0]);
      });
      root.walkAtRules(/keyframes$/i, at => {
        if (!animUsed.has(at.params.trim())) at.remove();
      });
    },
  };
}
purgeCss.postcss = true;
