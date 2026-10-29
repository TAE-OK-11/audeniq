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

/** 소스의 모든 .css 파일에서 animation·사용자 속성 값에 쓰인 이름 (한 번만 읽는다) */
let animNames: Set<string> | null = null;
function cssAnimNames(roots: string[]): Set<string> {
  if (animNames) return animNames;
  animNames = new Set();
  const files: string[] = [];
  const walkCss = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const p = join(dir, name);
      if (statSync(p).isDirectory()) walkCss(p);
      else if (name.endsWith('.css')) files.push(p);
    }
  };
  for (const r of roots) if (statSync(r).isDirectory()) walkCss(r);
  for (const f of files) {
    for (const m of readFileSync(f, 'utf8').matchAll(/(?:animation(?:-name)?|--[\w-]+)\s*:([^;}]*)/g)) {
      for (const t of m[1].matchAll(/[A-Za-z_][\w-]*/g)) animNames.add(t[0]);
    }
  }
  return animNames;
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

/** 시작 파일들에서 import로 닿는 소스 파일 (상대 경로만, exclude는 따라가지 않는다) */
export function importClosure(roots: string[], exclude: string[] = []): string[] {
  const seen = new Set<string>();
  const skip = new Set(exclude);
  const resolve = (from: string, spec: string): string | null => {
    const base = join(from, '..', spec);
    for (const c of [base, `${base}.ts`, `${base}.tsx`, join(base, 'index.ts'), join(base, 'index.tsx')]) {
      try { if (statSync(c).isFile()) return c; } catch { /* 없음 */ }
    }
    return null;
  };
  const visit = (file: string) => {
    if (seen.has(file) || skip.has(file)) return;
    seen.add(file);
    const text = readFileSync(file, 'utf8');
    for (const m of text.matchAll(/(?:from\s*|import\s*\(\s*|import\s+)['"](\.{1,2}\/[^'"]+)['"]/g)) {
      const r = resolve(file, m[1]);
      if (r && /\.tsx?$/.test(r)) visit(r);
    }
  };
  roots.forEach(visit);
  return [...seen];
}

export function purgeCss(opts: {
  content: string[]; safelist?: RegExp[];
  /** 이 이름의 CSS(예: public.css)는 아래 파일들에서 import로 닿는 코드에 나오는 클래스만 남긴다 */
  scoped?: { file: RegExp; roots: string[]; exclude?: string[]; extra?: string[] };
}): Plugin {
  let tokens: ReturnType<typeof collectTokens> | null = null;
  let scopedTokens: ReturnType<typeof collectTokens> | null = null;
  const usedIn = (t: ReturnType<typeof collectTokens>, name: string) =>
    t.exact.has(name) || !!opts.safelist?.some(r => r.test(name)) || t.prefixes.some(p => name.startsWith(p));
  const used = (name: string) => usedIn(tokens ??= collectTokens(opts.content), name);
  const usedScoped = (name: string) => {
    const sc = opts.scoped!;
    scopedTokens ??= collectTokens([...importClosure(sc.roots, sc.exclude), ...(sc.extra ?? [])]);
    return usedIn(scopedTokens, name);
  };
  return {
    postcssPlugin: 'aq-purge-css',
    OnceExit(root: Root) {
      const file = root.source?.input.file ?? '';
      const test = opts.scoped && opts.scoped.file.test(file) ? usedScoped : used;
      root.walkRules((rule: Rule) => {
        const parent = rule.parent as AtRule | undefined;
        if (parent?.type === 'atrule' && /keyframes$/i.test(parent.name)) return;
        const keep = rule.selectors.filter(s => selectorUsed(s, test));
        if (!keep.length) rule.remove();
        else if (keep.length !== rule.selectors.length) rule.selectors = keep;
      });
      // 비어 버린 @media 등 정리
      root.walkAtRules((at: AtRule) => {
        if (at.nodes && at.nodes.length === 0) at.remove();
      });
      // 사용되지 않는 @keyframes 제거 — CSS는 파일마다 따로 처리되므로 다른 스타일 파일에서 쓰는 이름도 쓰는 것으로 본다
      // (예전엔 enhance.css의 aq-badge-pop을 design-align.css만 써서 정의가 지워져 D-day 배지가 튀지 않았다)
      const animUsed = new Set<string>(cssAnimNames(opts.content));
      // 사용자 속성(--x: 이름)으로 넘겨 animation: var(--x)로 쓰는 키프레임도 쓰는 것으로 본다
      root.walkDecls(/^((-webkit-)?animation(-name)?|--.+)$/, d => {
        for (const m of d.value.matchAll(/[A-Za-z_][\w-]*/g)) animUsed.add(m[0]);
      });
      root.walkAtRules(/keyframes$/i, at => {
        if (!animUsed.has(at.params.trim())) at.remove();
      });
    },
  };
}
purgeCss.postcss = true;

/**
 * 의미를 바꾸지 않는 안전한 압축 — 주석과 공백만 지운다.
 * Lightning CSS 압축기는 같은 선택자 규칙이 뒤에 다시 나오면 앞 규칙의 `!important` 선언을
 * 잘못 제거하는 문제가 있어(예: `.a{x:1!important}` … `.a{x:2}` → x:2로 바뀜) 사용하지 않는다.
 */
export function compactCss(): Plugin {
  return {
    postcssPlugin: 'aq-compact-css',
    OnceExit(root: Root) {
      root.walkComments(c => { c.remove(); });
      root.raws = {};
      root.walk(node => {
        node.raws.before = '';
        if (node.type === 'decl') {
          node.raws.between = ':';
          node.value = node.value.replace(/\s*\n\s*/g, ' ').trim();
          if (node.important) node.raws.important = '!important';
        } else if (node.type === 'rule') {
          node.raws.between = '';
          node.raws.after = '';
          node.raws.semicolon = false;
          node.selector = node.selectors.map(s => s.replace(/\s+/g, ' ').trim()).join(',');
        } else if (node.type === 'atrule') {
          node.raws.between = '';
          node.raws.after = '';
          node.raws.afterName = node.params ? ' ' : '';
          node.raws.semicolon = false;
        }
      });
    },
  };
}
compactCss.postcss = true;
