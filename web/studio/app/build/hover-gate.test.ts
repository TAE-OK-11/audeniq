import { describe, expect, it } from 'vitest';
import postcss from 'postcss';
import { hoverGate, HOVER_QUERY } from './hover-gate';

const Q = HOVER_QUERY.replace(/\s+/g, '');
const run = (css: string) => postcss([hoverGate()]).process(css, { from: undefined }).css.replace(/\s+/g, '');

describe('hoverGate', () => {
  it('moves hover-only rules into the pointer media query', () => {
    expect(run('.a:hover{color:red}')).toBe(`@media${Q}{.a:hover{color:red}}`);
  });

  it('splits mixed selector lists and keeps the non-hover part in place', () => {
    const out = run('.a:focus-visible,.a:hover{color:red}.b{color:blue}');
    expect(out).toBe(`.a:focus-visible{color:red}@media${Q}{.a:hover{color:red}}.b{color:blue}`);
  });

  it('leaves rules already gated by a hover media query alone', () => {
    const css = '@media (hover:hover){.a:hover{color:red}}';
    expect(run(css)).toBe(css.replace(/\s+/g, ''));
  });

  it('nests inside other media queries', () => {
    expect(run('@media (max-width:760px){.a:hover{color:red}}'))
      .toBe(`@media(max-width:760px){@media${Q}{.a:hover{color:red}}}`);
  });

  it('does not touch :active or :focus', () => {
    const css = '.a:active{transform:scale(.97)}.a:focus{outline:0}';
    expect(run(css)).toBe(css.replace(/\s+/g, ''));
  });
});
