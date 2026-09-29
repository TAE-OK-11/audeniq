// 빌드·개발 공용 PostCSS 플러그인 — 모든 `:hover` 규칙을 `@media (hover:hover) and (pointer:fine)` 안으로 옮긴다.
//
// 휴대폰(iOS Safari 등)은 손가락을 댔다 떼거나, 누르려다 스크롤하면 그 요소에 :hover가 남아
// 버튼·카드 색이 ‘눌린 채’로 돌아오지 않는다. 마우스처럼 진짜 호버가 되는 기기에서만 호버 효과를 켜고,
// 터치 기기의 눌림 표시는 손을 떼면 바로 풀리는 :active가 맡는다.
//
// - 이미 hover 조건이 붙은 @media 안의 규칙은 그대로 둔다.
// - 선택자 목록 중 :hover가 있는 것만 떼어 바로 뒤에 넣으므로 캐스케이드 순서는 그대로다.
// - 다른 @media 안의 규칙은 그 안에 한 겹 더 감싼다 (중첩 조건부 규칙).
import type { AtRule, Container, Plugin, Rule } from 'postcss';

export const HOVER_QUERY = '(hover:hover) and (pointer:fine)';
const HAS_HOVER = /:hover\b/;

function gated(rule: Rule): boolean {
  for (let p: Container | undefined = rule.parent as Container | undefined; p && p.type !== 'root'; p = p.parent as Container | undefined) {
    if (p.type === 'atrule') {
      const at = p as AtRule;
      if (/keyframes$/i.test(at.name)) return true;
      if (at.name === 'media' && /\bhover\b|any-hover|pointer\s*:\s*fine/.test(at.params)) return true;
    }
  }
  return false;
}

export function hoverGate(): Plugin {
  return {
    postcssPlugin: 'aq-hover-gate',
    OnceExit(root, { AtRule }) {
      root.walkRules(rule => {
        if (!HAS_HOVER.test(rule.selector) || gated(rule)) return;
        const hover = rule.selectors.filter(s => HAS_HOVER.test(s));
        const rest = rule.selectors.filter(s => !HAS_HOVER.test(s));
        const media = new AtRule({ name: 'media', params: HOVER_QUERY });
        media.append(rule.clone({ selectors: hover }));
        if (rest.length) {
          rule.selectors = rest;
          rule.after(media);
        } else {
          rule.replaceWith(media);
        }
      });
    },
  };
}
hoverGate.postcss = true;
