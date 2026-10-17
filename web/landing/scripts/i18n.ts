// Translation helpers for the localized homepages. The Korean index.html stays the
// single design source: every other language is the same markup with each Korean
// text segment replaced from content/i18n/<code>.json. A segment is a run of text
// (and <br> tags) between two other tags, compared with whitespace collapsed.

export const HANGUL = /[ᄀ-ᇿ㄰-㆏가-힯]/;
// Korean that may legitimately appear on other language versions: the official
// Korean brand name and the native name of the Korean version in the language menu.
export const ALLOWED_HANGUL = ['어데닉', '한국어'];
const TRANSLATED_ATTRIBUTES = /(\s(?:alt|aria-label|title|placeholder|data-label-open|data-label-close)=")([^"]*)(")/g;
const TOKEN = /(<!--[\s\S]*?-->|<script\b[\s\S]*?<\/script>|<style\b[\s\S]*?<\/style>|<[^>]+>)/;

export const normalize = text => text.replace(/\s+/g, ' ').trim();
const isBreak = token => /^<br\b[^>]*>$/i.test(token);

/** Calls onSegment(key, raw) for each text segment and onTag(tag) for each other tag. */
function walk(html, onSegment, onTag) {
  const tokens = html.split(TOKEN);
  const out = [];
  let buffer = [];
  const flush = () => {
    if (!buffer.length) return;
    const raw = buffer.join('');
    buffer = [];
    const key = normalize(raw);
    if (!key) { out.push(raw); return; }
    const lead = raw.match(/^\s*/)[0], trail = raw.match(/\s*$/)[0];
    const replaced = onSegment(key, raw);
    out.push(replaced === undefined ? raw : lead + replaced + trail);
  };
  tokens.forEach((token, index) => {
    if (index % 2 === 0 || isBreak(token)) { buffer.push(token); return; }
    flush();
    out.push(/^<(?:!--|script|style)/i.test(token) ? token : onTag(token));
  });
  flush();
  return out.join('');
}

/** Korean segments and attribute values that a translation must cover, in page order. */
export function translatableKeys(html) {
  const keys = [];
  const add = key => { if (HANGUL.test(key) && !keys.includes(key)) keys.push(key); };
  walk(html, key => { add(key); }, tag => {
    for (const match of tag.matchAll(TRANSLATED_ATTRIBUTES)) add(normalize(match[2]));
    return tag;
  });
  return keys;
}

/** Translates segments and attributes. Returns html and the Korean keys left untranslated. */
export function translateHtml(html, map) {
  const missing = [];
  const used = new Set();
  const lookup = key => {
    if (Object.hasOwn(map, key)) { used.add(key); return map[key]; }
    if (HANGUL.test(key) && !ALLOWED_HANGUL.includes(key) && !missing.includes(key)) missing.push(key);
    return undefined;
  };
  const result = walk(html, key => lookup(key), tag => tag.replace(TRANSLATED_ATTRIBUTES, (all, open, value, close) => {
    const translated = lookup(normalize(value));
    return translated === undefined ? all : open + translated.replace(/"/g, '&quot;') + close;
  }));
  return { html: result, missing, unused: Object.keys(map).filter(key => !used.has(key)) };
}

/** Hangul visible to a reader or crawler outside the allowed brand/language names. */
export function strayHangul(html) {
  let text = html.replace(/<!--[\s\S]*?-->/g, '').replace(/<style\b[\s\S]*?<\/style>/g, '');
  for (const allowed of ALLOWED_HANGUL) text = text.split(allowed).join('');
  return [...new Set(text.match(/[^<>"\n]{0,24}[ᄀ-ᇿ㄰-㆏가-힯]+[^<>"\n]{0,24}/g) || [])];
}
