import postcss from 'postcss';
import { describe, expect, it } from 'vitest';
import { dropOverridden } from './dedupe-css';

const run = (css: string) => {
  const root = postcss.parse(css);
  dropOverridden(root);
  return root.toString().replace(/\s+/g, ' ').trim();
};

describe('dropOverridden', () => {
  it('뒤에 같은 선택자·속성이 다시 나오면 앞 선언을 지운다', () => {
    expect(run('.a{color:red;margin:0}.b{color:blue}.a{color:green}')).toBe('.a{margin:0}.b{color:blue}.a{color:green}');
  });
  it('조건(@media)이 다르면 남긴다', () => {
    const css = '.a{color:red}@media (min-width:1px){.a{color:blue}}';
    expect(run(css)).toBe(css);
  });
  it('같은 @media 안끼리는 지우고 빈 @media는 정리한다', () => {
    expect(run('@media (x){.a{c:1}}@media (x){.a{c:2}}')).toBe('@media (x){.a{c:2}}');
  });
  it('!important는 보통 선언에 덮이지 않는다', () => {
    const css = '.a{c:1!important}.a{c:2}';
    expect(run(css)).toBe(css);
    expect(run('.a{c:1}.a{c:2!important}')).toBe('.a{c:2!important}');
    expect(run('.a{c:1!important}.a{c:2}.a{c:3!important}')).toBe('.a{c:3!important}');
  });
  it('선택자 목록이 다르거나 공백만 다르면', () => {
    expect(run('.a,.b{c:1}.a{c:2}')).toBe('.a,.b{c:1}.a{c:2}');
    expect(run('.a  .b{c:1}.a .b{c:2}')).toBe('.a .b{c:2}');
  });
  it('오래된 브라우저용 대비 값은 남긴다', () => {
    const fallback = '.a{height:100vh;height:100dvh}';
    expect(run(fallback)).toBe(fallback);
    const prefixed = '.a{display:-webkit-box}.a{display:flex}';
    expect(run(prefixed)).toBe(prefixed);
    const clamp = '.a{line-clamp:2}.a{line-clamp:3}';
    expect(run(clamp)).toBe(clamp);
  });
  it('keyframes 안은 건드리지 않는다', () => {
    const css = '@keyframes k{from{opacity:0}from{opacity:.5}}';
    expect(run(css)).toBe(css);
  });
});
