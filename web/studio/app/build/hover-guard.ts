// PostCSS 플러그인 — `:hover` 규칙을 실제 마우스가 있는 환경에서만 적용한다.
// 휴대폰은 탭한 요소에 :hover가 남아(sticky hover) 누르다 만 버튼·카드가 눌린 색·그림자로
// 계속 보이는 문제가 있다. 모든 :hover 선택자를 `@media (hover:hover) and (pointer:fine)` 안으로
// 옮기고, 같은 규칙의 다른 선택자(:focus-visible 등)는 그대로 둔다. 원래 위치에 그대로 넣으므로
// 캐스케이드 순서·우선순위는 바뀌지 않는다. 터치 피드백은 손을 떼면 풀리는 :active가 맡는다.
import type { AtRule, Container, Plugin, Rule } from 'postcss';

export const HOVER_MEDIA = '(hover:hover) and (pointer:fine)';
const HAS_HOVER = /:hover(?![\w-])/;

function insideHoverMedia(node: Rule): boolean {
  for (let p: Container | undefined = node.parent as Container | undefined; p; p = p.parent as Container | undefined) {
    if (p.type === 'atrule' && (p as AtRule).name === 'media' && /\bhover\s*:\s*hover\b/.test((p as AtRule).params)) return true;
  }
  return false;
}

export function hoverGuard(): Plugin {
  return {
    postcssPlugin: 'aq-hover-guard',
    Once(root, { AtRule }) {
      const targets: Rule[] = [];
      root.walkRules(rule => {
        const parent = rule.parent as AtRule | undefined;
        if (parent?.type === 'atrule' && /keyframes$/i.test(parent.name)) return;
        if (HAS_HOVER.test(rule.selector) && !insideHoverMedia(rule)) targets.push(rule);
      });
      for (const rule of targets) {
        const hover = rule.selectors.filter(s => HAS_HOVER.test(s));
        const rest = rule.selectors.filter(s => !HAS_HOVER.test(s));
        const media = new AtRule({ name: 'media', params: HOVER_MEDIA });
        if (rest.length) {
          const copy = rule.clone({ selectors: hover });
          media.append(copy);
          rule.selectors = rest;
          rule.after(media);
        } else {
          rule.replaceWith(media);
          media.append(rule);
        }
      }
    },
  };
}
hoverGuard.postcss = true;
