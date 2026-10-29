#!/usr/bin/env python3
"""Audeniq Sans 웹 폰트를 같은 출처에서 서빙할 두 조각으로 나눈다 (Safari 폰트 preload·캐시 대응).

- 다른 도메인(jsDelivr) 폰트는 Safari가 CORS 없이 받아, crossorigin으로 미리 받은(preload) 것을 다시 쓰지 못하고 한 번 더 받았다
  (참고: https://blog.banksalad.com/tech/font-preload-on-safari/). 같은 출처에서 주면 그런 차이가 없고, 연결도 하나 준다.
- core: 스튜디오 화면 문구에 쓰인 한글 + 라틴·숫자·문장부호·통화·화살표 등 — 첫 화면에 미리 받는다 (preload)
- more: 원래 서브셋의 나머지(사용자가 입력한 이름·제목의 한글, 다른 문자) — 그 글자가 화면에 나올 때만 받는다
  두 조각은 같은 family의 @font-face 두 개라 글자 모양·굵기·간격은 그대로다 (같은 폰트에서 잘라 낸다).
- 서브셋에 없는 한글은 지금처럼 'Audeniq Sans Full'(jsDelivr, 2 MB)이 필요할 때만 받는다.

사용: pip install fonttools brotli && python3 build/fonts.py
화면 문구에 새 한글이 생기면 다시 돌린다 (src/lib/fontCoverage.test.ts가 알려 준다).
"""
import glob, os, re, sys, urllib.request
from fontTools.ttLib import TTFont
from fontTools import subset

APP = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
COMMIT = '308d641281443c6172d94b50785d43890f5aafde'
SOURCE = f'https://cdn.jsdelivr.net/gh/TAE-OK-11/audeniq-font@{COMMIT}/fonts/web/AudeniqSansVariable-Subset.woff2'
OUT = os.path.join(APP, 'src', 'assets', 'fonts')
# 첫 화면에 늘 쓰이는 글자 묶음 (나머지 비한글은 more로)
CORE_BLOCKS = [(0x20, 0x7E), (0xA0, 0xFF), (0x2000, 0x206F), (0x20A0, 0x20CF), (0x2100, 0x218F), (0x2190, 0x21FF),
               (0x2200, 0x22FF), (0x25A0, 0x25FF), (0x3000, 0x303F), (0x3130, 0x318F), (0xFF00, 0xFFEF)]
HANGUL = (0xAC00, 0xD7A3)


def ui_codepoints():
    used = set()
    for f in glob.glob(os.path.join(APP, 'src', '**', '*'), recursive=True):
        if os.path.isfile(f) and f.endswith(('.ts', '.tsx', '.txt')) and '.test.' not in f:
            used.update(map(ord, open(f, encoding='utf-8', errors='ignore').read()))
    used.update(map(ord, open(os.path.join(APP, 'index.html'), encoding='utf-8').read()))
    return used


def ranges(cps):
    out, cps = [], sorted(cps)
    i = 0
    while i < len(cps):
        j = i
        while j + 1 < len(cps) and cps[j + 1] == cps[j] + 1:
            j += 1
        out.append(f'U+{cps[i]:X}' if i == j else f'U+{cps[i]:X}-{cps[j]:X}')
        i = j + 1
    return ','.join(out)


def make(src, unis, out):
    opts = subset.Options()
    opts.flavor = 'woff2'
    opts.layout_features = ['*']  # 글자 모양·간격에 쓰는 기능은 모두 남긴다
    opts.name_IDs = ['*']
    opts.notdef_outline = True
    font = TTFont(src)
    s = subset.Subsetter(opts)
    s.populate(unicodes=sorted(unis))
    s.subset(font)
    font.flavor = 'woff2'
    font.save(out)
    return os.path.getsize(out)


def main():
    os.makedirs(OUT, exist_ok=True)
    src = os.path.join(OUT, '.source.woff2')
    urllib.request.urlretrieve(SOURCE, src)
    cmap = set(TTFont(src).getBestCmap())
    used = ui_codepoints()
    core = {c for c in cmap if (HANGUL[0] <= c <= HANGUL[1] and c in used) or any(a <= c <= b for a, b in CORE_BLOCKS) or (c in used and not HANGUL[0] <= c <= HANGUL[1])}
    more = cmap - core
    a = make(src, core, os.path.join(OUT, 'audeniq-sans-core.woff2'))
    b = make(src, more, os.path.join(OUT, 'audeniq-sans-more.woff2'))
    os.remove(src)
    open(os.path.join(OUT, 'core-hangul.txt'), 'w').write(''.join(chr(c) for c in sorted(core) if HANGUL[0] <= c <= HANGUL[1]) + '\n')
    # index.html의 @font-face 블록을 새 범위로 바꾼다
    face = (
        '<style id="aq-fonts">'
        # more를 먼저: 범위가 겹치면 뒤에 적은 face부터 본다 — core에 있는 글자는 core, 없으면 more, 그래도 없으면 Full
        '@font-face{font-family:"Audeniq Sans";font-style:normal;font-weight:45 930;font-display:swap;'
        'src:url("/src/assets/fonts/audeniq-sans-more.woff2") format("woff2");unicode-range:U+0-10FFFF}'
        '@font-face{font-family:"Audeniq Sans";font-style:normal;font-weight:45 930;font-display:swap;'
        f'src:url("/src/assets/fonts/audeniq-sans-core.woff2") format("woff2");unicode-range:{ranges(core)}}}'
        '@font-face{font-family:"Audeniq Sans Full";font-style:normal;font-weight:45 930;font-display:swap;'
        f'src:url("https://cdn.jsdelivr.net/gh/TAE-OK-11/audeniq-font@{COMMIT}/fonts/web/AudeniqSansVariable.woff2") format("woff2");'
        'unicode-range:U+1100-11FF,U+3130-318F,U+A960-A97F,U+AC00-D7A3,U+D7B0-D7FF}'
        '</style>'
    )
    html_path = os.path.join(APP, 'index.html')
    html = open(html_path, encoding='utf-8').read()
    html, n = re.subn(r'<style id="aq-fonts">.*?</style>', lambda _: face, html, flags=re.S)
    if n != 1:
        sys.exit('index.html에 <style id="aq-fonts"> 블록이 하나 있어야 해요.')
    open(html_path, 'w', encoding='utf-8').write(html)
    print(f'core {a:,} B ({sum(1 for c in core if HANGUL[0] <= c <= HANGUL[1])} 한글) · more {b:,} B')


if __name__ == '__main__':
    main()
