import postcss from 'postcss';
import { describe, expect, it } from 'vitest';
import { hoverGuard } from './hover-guard';

const run = (css: string) => postcss([hoverGuard()]).process(css, { from: undefined }).css.replace(/\s*([{}])\s*/g, '$1').trim();

describe('hoverGuard', () => {
  it(':hover 규칙을 마우스 전용 미디어로 감싼다', () => {
    expect(run('.a:hover{x:1}')).toBe('@media (hover:hover) and (pointer:fine){.a:hover{x:1}}');
  });
  it('함께 묶인 다른 선택자는 밖에 남기고 순서를 지킨다', () => {
    const out = run('.a:hover,.a:focus-visible{x:1}.b{y:2}');
    expect(out).toBe('.a:focus-visible{x:1}@media (hover:hover) and (pointer:fine){.a:hover{x:1}}.b{y:2}');
  });
  it('이미 hover 미디어 안에 있거나 keyframes면 그대로', () => {
    const src = '@media (hover:hover){.a:hover{x:1}}';
    expect(run(src)).toBe(src);
  });
  it('다른 미디어 안의 :hover도 감싼다', () => {
    expect(run('@media (max-width:760px){.a:hover{x:1}}')).toBe('@media (max-width:760px){@media (hover:hover) and (pointer:fine){.a:hover{x:1}}}');
  });
  it(':hover가 아닌 이름은 건드리지 않는다', () => {
    expect(run('.a:hovered{x:1}')).toBe('.a:hovered{x:1}');
  });
});
