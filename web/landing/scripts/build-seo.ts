import { readFileSync, writeFileSync, mkdirSync, rmSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { createHash } from 'node:crypto';
import { translateHtml, translatableKeys, strayHangul } from './i18n.ts';
import type { Faq, GuidePage, GuideUi, Locale, LocaleFile, LocaleMeta, LocaleRegistry, PagesFile, ResolvedGuidePage, SiteManifest } from './content-types.ts';

/** A schema.org JSON-LD node */
type JsonLd = Record<string, unknown>;

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = (path: string): string => readFileSync(join(root, path), 'utf8');
const write = (path: string, text: string): void => {
  mkdirSync(dirname(join(root, path)), { recursive: true });
  writeFileSync(join(root, path), text);
};
const entities: Record<string, string> = {'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'};
const esc = (value: string | undefined): string => String(value).replace(/[&<>"']/g, char => entities[char]);
const clean = (text: string): string => text.replace(/<span[^>]*>[\s\S]*?<\/span>/g, '').replace(/<br[^>]*>/g, ' ').replace(/<[^>]*>/g, '').replace(/&amp;/g, '&').replace(/\s+/g, ' ').trim();
const origin = 'https://audeniq.com';
const org = `${origin}/#organization`;
const site = `${origin}/#website`;
const data: PagesFile = JSON.parse(read('content/pages.json'));

// ---- Languages -------------------------------------------------------------
const registry: LocaleRegistry = JSON.parse(read('content/locales.json'));
const printKeys = process.argv.includes('--print-keys');
const locales = registry.locales.filter(locale => locale.status === 'published' && (!printKeys || locale.code === registry.default));
const byCode: Record<string, Locale> = Object.fromEntries(locales.map(locale => [locale.code, locale]));
if (!byCode[registry.default] || byCode[registry.default].prefix !== '/') throw new Error('The Korean root must stay published at /');
for (const locale of locales) {
  if (!/^\/(?:[a-z]{2}(?:-[a-z]{2})?\/)?$/.test(locale.prefix)) throw new Error(`Invalid prefix ${locale.prefix}`);
  if (!['ltr','rtl'].includes(locale.dir)) throw new Error(`Invalid dir ${locale.code}`);
}
const i18n: Record<string, LocaleFile> = Object.fromEntries(locales.map(locale => [locale.code, JSON.parse(read(`content/i18n/${locale.code}.json`))]));
// The Korean source file always carries the guide UI strings, including the Korean-only RSS title.
const ko = i18n[registry.default] as LocaleFile & { ui: GuideUi & { rssTitle: string } };
const isDefault = (locale: Locale): boolean => locale.code === registry.default;
const localeUrl = (locale: Locale, key: string): string => origin + locale.prefix + key.slice(1);
const localePath = (locale: Locale, key: string): string => locale.prefix + key.slice(1);
const updatedOf = (locale: Locale): string => isDefault(locale) ? data.updated : i18n[locale.code].updated;
for (const locale of locales.filter(locale => !isDefault(locale))) {
  const file = i18n[locale.code];
  for (const field of ['title','description','ogDescription','ogImageAlt','orgDescription','websiteDescription','languageLabel','languageNav'] as const) {
    if (!file.meta?.[field]) throw new Error(`content/i18n/${locale.code}.json: meta.${field} missing`);
  }
  if (!/^\d{4}-\d{2}-\d{2}$/.test(file.updated || '')) throw new Error(`content/i18n/${locale.code}.json: updated date missing`);
  for (const value of Object.values(file.home || {})) {
    if (/<(?!br\b)[^>]*>/i.test(value)) throw new Error(`content/i18n/${locale.code}.json: only <br> markup is allowed in translations: ${value}`);
  }
}
// Guide pages per language (Korean from pages.json, others optional in their i18n file).
const guidesOf = (locale: Locale): GuidePage[] => isDefault(locale) ? data.pages : (i18n[locale.code].pages || []);
const pageKeys = ['/', ...data.pages.map(page => page.path)];
const hasPage = (locale: Locale, key: string): boolean => key === '/' || guidesOf(locale).some(page => page.path === key);
const versions = (key: string): Locale[] => locales.filter(locale => hasPage(locale, key));
const xDefault = (key: string): string => {
  const preferred = byCode[registry.xDefault];
  return localeUrl(preferred && hasPage(preferred, key) ? preferred : byCode[registry.default], key);
};
const hreflangLinks = (key: string): string => [...versions(key).map(locale => `<link rel="alternate" hreflang="${locale.hreflang}" href="${localeUrl(locale, key)}">`),
  `<link rel="alternate" hreflang="x-default" href="${xDefault(key)}">`].join('\n');
const ogAlternates = (locale: Locale, key: string): string => versions(key).filter(other => other !== locale)
  .map(other => `<meta property="og:locale:alternate" content="${other.ogLocale}">`).join('');
const languageNav = (locale: Locale, key: string): string => {
  const meta = isDefault(locale) ? ko.meta : i18n[locale.code].meta;
  return `<nav class="footer-languages" aria-label="${esc(meta.languageNav)}"><span>${esc(meta.languageLabel)}</span>${versions(key).map(other =>
    `<a href="${localePath(other, key)}" hreflang="${other.hreflang}" lang="${other.lang}"${other === locale ? ' aria-current="page"' : ''}>${esc(other.name)}</a>`).join('')}</nav>`;
};

// ---- Structured data ---------------------------------------------------------
const organization = (locale: Locale): JsonLd => ({
  '@type':'Organization', '@id':org, name:'AUDENIQ', alternateName:'어데닉', url:`${origin}/`,
  description:(isDefault(locale) ? ko : i18n[locale.code]).meta.orgDescription,
  email:'audeniq.official@gmail.com',
  logo:{'@type':'ImageObject',url:`${origin}/assets/AUDENIQ_Logo_Light.svg`},
  sameAs:['https://www.instagram.com/audeniq.official','https://www.threads.com/@audeniq.official','https://x.com/audeniq']
});
const website = (locale: Locale): JsonLd => ({'@type':'WebSite','@id':site,url:`${origin}/`,name:'AUDENIQ',alternateName:'어데닉',
  inLanguage:locales.map(item => item.lang),
  description:(isDefault(locale) ? ko : i18n[locale.code]).meta.websiteDescription, publisher:{'@id':org}});
const questions = (faqs: Faq[]): JsonLd[] => faqs.map(faq => ({'@type':'Question',name:faq.q,acceptedAnswer:{'@type':'Answer',text:faq.a}}));
const schemaScript = (graph: JsonLd[]): string => `<script type="application/ld+json">${JSON.stringify({'@context':'https://schema.org','@graph':graph}).replace(/</g,'\\u003c')}</script>`;
const faqOf = (html: string): Faq[] => {
  const section = html.match(/<section[^>]*id="faq"[\s\S]*?<\/section>/)?.[0];
  if (!section) throw new Error('Homepage FAQ section missing');
  return [...section.matchAll(/<details><summary>([\s\S]*?)<\/summary><p>([\s\S]*?)<\/p><\/details>/g)].map(match => ({q:clean(match[1]), a:clean(match[2])}));
};

// ---- Homepage (Korean source) ------------------------------------------------
let source = read('index.html');
source = source.replace('<p class="eyebrow">독립 음악을 위한 배급과 퍼블리싱</p>\n      <h1 id="hero-title">', '<h1 id="hero-title"><span class="hero-service eyebrow">독립 음악을 위한 배급과 퍼블리싱</span>');
// Keep discovery resources in the head and assets, without a directory/RSS block
// beneath the homepage FAQ. Service cards provide the relevant guide links.
source = source.replace(/<!-- SEARCH_GUIDES_START -->[\s\S]*?<!-- SEARCH_GUIDES_END -->\s*/g, '');
source = source.replace(/<!-- SEO_BUILD_START -->[\s\S]*?<!-- SEO_BUILD_END -->\s*/g, '');
source = source.replace(/<!-- LANGUAGE_LINKS_START -->[\s\S]*?<!-- LANGUAGE_LINKS_END -->/, '<!-- LANGUAGE_LINKS_START --><!-- LANGUAGE_LINKS_END -->');
if (printKeys) {
  console.log(JSON.stringify(translatableKeys(source.split('</head>')[1]), null, 2));
  process.exit(0);
}
const koFaq = faqOf(source);
if (koFaq.length < 8) throw new Error('Review homepage FAQs before changing the generated reference');

const sharedStyle = '<style>.hero h1 .hero-service{display:block;font-size:15px;line-height:1.7;letter-spacing:.03em;font-weight:700;color:var(--blue);margin-bottom:22px}@media(max-width:680px){.hero h1 .hero-service{font-size:12px}}.footer-languages{display:flex;flex-wrap:wrap;align-items:center;gap:0 16px;margin-top:14px;font-size:12px;color:var(--muted)}.footer-languages span{font-weight:650}.footer-languages a{padding:8px 0}.footer-languages a:hover,.footer-languages a[aria-current]{color:var(--blue)}</style>';
const localeStyle = `<style>${read('content/locale.css').replace(/\/\*[\s\S]*?\*\//g, '').replace(/\s*\n\s*/g, '')}</style>`;
const fontLinks = /\s*<link rel="preconnect" href="https:\/\/cdn\.jsdelivr\.net" crossorigin>\s*<link rel="stylesheet" href="https:\/\/cdn\.jsdelivr\.net\/gh\/orioncactus\/pretendard[^"]+">/;
if (!fontLinks.test(source)) throw new Error('Homepage font links changed; review fonts for system-font languages');

const metaReplace = (html: string, pattern: RegExp, value: string, label: string): string => {
  if (!pattern.test(html)) throw new Error(`Homepage head is missing ${label}`);
  return html.replace(pattern, value);
};
const homes: Record<string, { html: string; faq: Faq[]; title: string }> = {};
const report: string[] = [];
for (const locale of locales) {
  const key = '/';
  const url = localeUrl(locale, key);
  let html = source;
  let title: string, description: string;
  if (isDefault(locale)) {
    title = clean(html.match(/<title>([\s\S]*?)<\/title>/)![1]);
    description = html.match(/<meta name="description" content="([^"]*)">/)![1];
  } else {
    const file = i18n[locale.code];
    // The validation loop above requires these meta fields for every non-default language.
    const meta = file.meta as LocaleMeta & Required<Pick<LocaleMeta, 'title' | 'description' | 'ogDescription' | 'ogImageAlt'>>;
    ({ title, description } = meta);
    html = html.replace('<html lang="ko">', `<html lang="${locale.lang}"${locale.dir === 'rtl' ? ' dir="rtl"' : ''}>`);
    html = metaReplace(html, /<title>[\s\S]*?<\/title>/, `<title>${esc(title)}</title>`, 'title');
    html = metaReplace(html, /<meta name="description" content="[^"]*">/, `<meta name="description" content="${esc(description)}">`, 'description');
    html = metaReplace(html, /<link rel="canonical" href="[^"]*">/, `<link rel="canonical" href="${url}">`, 'canonical');
    html = metaReplace(html, /<meta property="og:locale" content="[^"]*">/, `<meta property="og:locale" content="${locale.ogLocale}">`, 'og:locale');
    html = metaReplace(html, /<meta property="og:title" content="[^"]*">/, `<meta property="og:title" content="${esc(title)}">`, 'og:title');
    html = metaReplace(html, /<meta property="og:description" content="[^"]*">/, `<meta property="og:description" content="${esc(meta.ogDescription)}">`, 'og:description');
    html = metaReplace(html, /<meta property="og:url" content="[^"]*">/, `<meta property="og:url" content="${url}">`, 'og:url');
    html = metaReplace(html, /<meta property="og:image:alt" content="[^"]*">/, `<meta property="og:image:alt" content="${esc(meta.ogImageAlt)}">`, 'og:image:alt');
    html = metaReplace(html, /<meta name="twitter:title" content="[^"]*">/, `<meta name="twitter:title" content="${esc(title)}">`, 'twitter:title');
    html = metaReplace(html, /<meta name="twitter:description" content="[^"]*">/, `<meta name="twitter:description" content="${esc(meta.ogDescription)}">`, 'twitter:description');
    // Pretendard carries Korean-style Hanja and no Vietnamese tone marks: these
    // languages use their platform fonts and skip the extra stylesheet.
    if (locale.fonts === 'system') html = html.replace(fontLinks, '');
  }
  html = html.replace(/<!-- LANGUAGE_LINKS_START --><!-- LANGUAGE_LINKS_END -->/, `<!-- LANGUAGE_LINKS_START -->${languageNav(locale, key)}<!-- LANGUAGE_LINKS_END -->`);
  if (!isDefault(locale)) {
    const result = translateHtml(html, i18n[locale.code].home || {});
    if (result.missing.length) throw new Error(`content/i18n/${locale.code}.json is missing ${result.missing.length} homepage translation(s):\n${result.missing.map(key => `  ${JSON.stringify(key)}`).join('\n')}`);
    if (result.unused.length) report.push(`${locale.code}: ${result.unused.length} unused translation(s) — ${result.unused.slice(0, 3).map(key => JSON.stringify(key)).join(', ')}`);
    html = result.html.replace(/<!--[\s\S]*?-->\s*/g, '');
  }
  const faq = isDefault(locale) ? koFaq : faqOf(html);
  if (faq.length !== koFaq.length) throw new Error(`${locale.code}: FAQ count differs from Korean`);
  const graph: JsonLd[] = [organization(locale), website(locale), {
    '@type':['WebPage','FAQPage'],'@id':`${url}#webpage`,url,name:title,description,
    inLanguage:locale.lang,dateModified:updatedOf(locale),isPartOf:{'@id':site},about:{'@id':org},mainEntity:questions(faq)
  }];
  html = html.replace(/<script type="application\/ld\+json">[\s\S]*?<\/script>/, schemaScript(graph));
  const seo = [hreflangLinks(key), ogAlternates(locale, key),
    isDefault(locale) ? `<link rel="alternate" type="application/rss+xml" title="${esc(ko.ui.rssTitle)}" href="${origin}/feed.xml">` : '',
    sharedStyle, isDefault(locale) ? '' : localeStyle].filter(Boolean).join('\n');
  html = html.replace('</head>', `<!-- SEO_BUILD_START -->\n${seo}\n<!-- SEO_BUILD_END -->\n</head>`);
  if (!isDefault(locale)) {
    const stray = strayHangul(html);
    if (stray.length) throw new Error(`${locale.code}: untranslated Korean remains: ${stray.slice(0, 5).join(' | ')}`);
  }
  homes[locale.code] = { html, faq, title };
  if (isDefault(locale)) write('index.html', html);
  write(`public${localePath(locale, key)}index.html`, html);
}

// ---- Guide pages -------------------------------------------------------------
const homepageStyles = [...homes[registry.default].html.matchAll(/<style(?:\s[^>]*)?>([\s\S]*?)<\/style>/g)].map(match=>match[1]).join('\n');
if (!homepageStyles.includes('--font-body') || !homepageStyles.includes('--blue:')) throw new Error('Homepage brand styles missing');
const css = homepageStyles + '\n' + read('content/guide.css');
write('assets/guide.css',css);
write('public/assets/guide.css',css);
// Guides use the current logo immediately, including without JavaScript.
const currentLogo = (html: string): string=>html.replace(/src="data:image\/svg\+xml;base64,[^"]+"/g,'src="/assets/AUDENIQ_Logo_Light.svg"').replace(/width="1600" height="520"/g,'width="1060" height="182"');
const guideRecords: { locale: Locale; page: ResolvedGuidePage }[] = [];
for (const locale of locales) {
  const ui = isDefault(locale) ? ko.ui : i18n[locale.code].ui;
  const pages = guidesOf(locale).map(page => ({...page, faq: page.faqFromHome ? homes[locale.code].faq : page.faq}));
  if (!pages.length) continue;
  if (!ui) throw new Error(`content/i18n/${locale.code}.json: ui strings are required for guide pages`);
  const home = homes[locale.code].html;
  const brandImage = currentLogo(home.match(/<a class="brand"[^>]*>([\s\S]*?)<\/a>/)?.[1] || '');
  const baseFooter = currentLogo(home.match(/<footer\b[\s\S]*?<\/footer>/)?.[0] || '');
  if (!brandImage || !baseFooter) throw new Error('Homepage brand header/footer missing');
  const homePath = localePath(locale, '/');
  const updated = updatedOf(locale);
  for (const page of pages) {
    if (!pageKeys.includes(page.path)) throw new Error(`${locale.code}: unknown guide path ${page.path}`);
    const url = localeUrl(locale, page.path);
    const title = `${page.title} | AUDENIQ`;
    const label = ui.guides?.[page.path];
    if (!label) throw new Error(`Missing guide label ${locale.code} ${page.path}`);
    const webpage: JsonLd = {
      '@type': page.kind === 'about' ? 'AboutPage' : page.faq.length ? ['WebPage','FAQPage'] : 'WebPage',
      '@id':`${url}#webpage`,url,name:title,description:page.description,inLanguage:locale.lang,dateModified:updated,
      isPartOf:{'@id':site},about:{'@id':org},...(page.faq.length ? {mainEntity:questions(page.faq)} : {})
    };
    const graph: JsonLd[] = [organization(locale),website(locale),webpage,{'@type':'BreadcrumbList','@id':`${url}#breadcrumb`,itemListElement:[
      {'@type':'ListItem',position:1,name:'AUDENIQ',item:origin+homePath},
      {'@type':'ListItem',position:2,name:page.title,item:url}]}];
    webpage.breadcrumb = {'@id':`${url}#breadcrumb`};
    if (page.kind === 'service') graph.push({'@type':'Service','@id':`${url}#service`,name:page.title,
      serviceType:page.serviceType,description:page.answer,url,provider:{'@id':org},
      audience:{'@type':'Audience',audienceType:page.audience}});
    const navLinks = pages.map(item=>`<a href="${localePath(locale, item.path)}"${item.path===page.path?' aria-current="page"':''}>${esc(ui.guides[item.path].name)}</a>`).join('') + `<a class="nav-cta" href="#contact">${esc(ui.contact)}</a>`;
    const footer = baseFooter.replace(/<nav class="footer-languages"[\s\S]*?<\/nav>/, languageNav(locale, page.path));
    const fonts = locale.fonts === 'system' ? '' : '<link rel="preconnect" href="https://cdn.jsdelivr.net" crossorigin><link rel="stylesheet" href="https://cdn.jsdelivr.net/gh/orioncactus/pretendard@v1.3.9/dist/web/variable/pretendardvariable-dynamic-subset.min.css">';
    const html = `<!doctype html>
<html lang="${locale.lang}"${locale.dir === 'rtl' ? ' dir="rtl"' : ''}><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>${esc(title)}</title><meta name="description" content="${esc(page.description)}"><meta name="robots" content="index, follow, max-image-preview:large">
<link rel="canonical" href="${url}"><link rel="icon" type="image/svg+xml" href="/assets/favicon.svg">${fonts}<link rel="stylesheet" href="/assets/guide.css">
${hreflangLinks(page.path)}
${isDefault(locale) ? `<link rel="alternate" type="application/rss+xml" title="${esc(ui.rssTitle)}" href="${origin}/feed.xml">\n` : ''}<meta property="og:type" content="website"><meta property="og:locale" content="${locale.ogLocale}">${ogAlternates(locale, page.path)}<meta property="og:site_name" content="AUDENIQ"><meta property="og:title" content="${esc(title)}"><meta property="og:description" content="${esc(page.description)}"><meta property="og:url" content="${url}"><meta property="og:image" content="${origin}/assets/social-preview.png"><meta property="og:image:width" content="1200"><meta property="og:image:height" content="630"><meta property="og:image:alt" content="${esc(ui.ogImageAlt)}">
<meta name="twitter:card" content="summary_large_image"><meta name="twitter:title" content="${esc(title)}"><meta name="twitter:description" content="${esc(page.description)}"><meta name="twitter:image" content="${origin}/assets/social-preview.png">
${schemaScript(graph)}
${isDefault(locale) ? '' : localeStyle}</head><body class="guide-page"><a class="skip-link" href="#main">${esc(ui.skip)}</a>
<header class="site-header guide-header"><div class="nav-shell"><a class="brand" href="${homePath}" aria-label="${esc(ui.home)}">${brandImage}</a>
<nav class="primary-nav" aria-label="${esc(ui.mainMenu)}">${navLinks}</nav>
<details class="mobile-menu"><summary><span class="sr-only">${esc(ui.menuToggle)}</span><svg viewBox="0 0 24 24" fill="none" aria-hidden="true"><path d="M3 8h18M3 16h18" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg></summary><nav aria-label="${esc(ui.mobileMenu)}">${navLinks}</nav></details></div></header>
<main id="main"><div class="guide-breadcrumb container" aria-label="${esc(ui.breadcrumb)}"><a href="${homePath}">AUDENIQ</a><span aria-hidden="true">/</span><span>${esc(label.name)}</span></div>
<section class="hero wide guide-hero" id="top" aria-labelledby="guide-title"><div class="hero-copy"><p class="eyebrow">${esc(label.eyebrow)}</p><h1 id="guide-title">${esc(page.title)}</h1>
<div class="guide-intro"><h2>${esc(page.question)}</h2><p>${esc(page.answer)}</p></div>
<div class="guide-meta"><span class="pill">${esc(ui.status)}</span><p class="small">${esc(ui.official)} · <time datetime="${updated}">${updated}</time></p></div></div></section>
<section class="section guide-overview" aria-labelledby="overview-title"><div class="container guide-facts-layout"><div class="guide-section-heading"><p class="eyebrow">${esc(ui.overviewEyebrow)}</p><h2 id="overview-title">${esc(ui.overviewTitle)}</h2><p>${ui.overviewText}</p></div>
<div class="guide-table"><table><caption>${esc(ui.overviewCaption)}</caption><tbody>${page.facts.map(([key,value])=>`<tr><th scope="row">${esc(key)}</th><td>${esc(value)}</td></tr>`).join('')}</tbody></table></div></div></section>
${page.sections.length ? `<section class="section guide-story" aria-label="${esc(`${label.name} ${ui.detailsSuffix}`)}"><div class="container">${page.sections.map((section,index)=>`<div class="guide-story-row"><div class="guide-story-heading"><span class="service-no" aria-hidden="true">${String(index+1).padStart(2,'0')}</span><h2>${esc(section.heading)}</h2></div><div>${section.paragraphs.map(text=>`<p>${esc(text)}</p>`).join('')}</div></div>`).join('\n')}</div></section>` : ''}
${page.faq.length ? `<section class="section faq chapter-soft guide-faq" aria-labelledby="faq-title"><div class="container faq-layout"><div class="guide-section-heading"><p class="eyebrow">${esc(ui.faqEyebrow)}</p><h2 id="faq-title">${esc(ui.faqTitle)}</h2><p>${ui.faqText}</p></div><div class="faq-list">${page.faq.map(faq=>`<details><summary>${esc(faq.q)}<span aria-hidden="true">＋</span></summary><p>${esc(faq.a)}</p></details>`).join('\n')}</div></div></section>` : ''}
<section class="section guide-related" aria-labelledby="related-title"><div class="container"><div class="section-heading"><p class="eyebrow">${esc(ui.relatedEyebrow)}</p><h2 id="related-title">${esc(ui.relatedTitle)}</h2></div><div class="service-grid">${page.related.map(path=>{const related=pages.find(candidate=>candidate.path===path);if(!related)throw new Error(`Unknown related path ${locale.code} ${path}`);return `<a class="service-item" href="${localePath(locale, path)}"><div class="service-top"><span class="service-no">${esc(ui.guides[path].name)}</span><span class="pill">${esc(ui.preparing)}</span></div><h3>${esc(related.title)}</h3><p>${esc(related.answer)}</p><div class="service-caption"><strong>${esc(ui.more)}</strong><span aria-hidden="true">→</span></div></a>`;}).join('')}</div>
<div class="guide-sources"><span>${esc(ui.sources)}</span>${page.sources.map(source=>`<a href="${esc(source.url)}">${esc(source.name)} ↗</a>`).join('')}</div></div></section>
<section class="survey-section wide guide-contact" aria-labelledby="contact-title"><div class="survey-inner"><p class="eyebrow">${esc(ui.contactEyebrow)}</p><h2 id="contact-title">${ui.contactTitle}</h2><p>${ui.contactText}</p><a class="button" href="mailto:audeniq.official@gmail.com">${esc(ui.contactButton)} <span aria-hidden="true">↗</span></a><p class="small">${ui.contactNote}</p></div></section>
</main>${footer}</body></html>
`;
    if (!isDefault(locale)) {
      const stray = strayHangul(html);
      if (stray.length) throw new Error(`${locale.code}${page.path}: untranslated Korean remains: ${stray.slice(0, 5).join(' | ')}`);
    }
    write(`public${localePath(locale, page.path)}index.html`, html);
    guideRecords.push({ locale, page });
  }
}

// ---- Discovery resources -----------------------------------------------------
// Remove stale generated language folders (e.g. a language moved back to draft).
for (const entry of readdirSync(join(root, 'public'), { withFileTypes: true })) {
  if (entry.isDirectory() && /^[a-z]{2}(?:-[a-z]{2})?$/.test(entry.name) && !locales.some(locale => locale.prefix === `/${entry.name}/`)) {
    rmSync(join(root, 'public', entry.name), { recursive: true });
  }
}
const publicPages: { locale: Locale; key: string; path: string }[] = [];
for (const locale of locales) for (const key of pageKeys) if (hasPage(locale, key)) publicPages.push({ locale, key, path: localePath(locale, key) });
const publicPaths = publicPages.map(page => page.path);
const sitemap = `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">\n${publicPages.map(({locale,key})=>
  `  <url><loc>${localeUrl(locale, key)}</loc><lastmod>${updatedOf(locale)}</lastmod>${versions(key).map(other=>`<xhtml:link rel="alternate" hreflang="${other.hreflang}" href="${localeUrl(other, key)}"/>`).join('')}<xhtml:link rel="alternate" hreflang="x-default" href="${xDefault(key)}"/></url>`).join('\n')}\n</urlset>\n`;
const privatePaths = ['/studio','/admin','/api','/login','/health'];
// Every crawler is welcome: search engines, AI search/answer bots and model-training crawlers
// (GPTBot, ClaudeBot, Google-Extended, CCBot, Applebot-Extended …) all follow the single `*` group.
// Only non-page application routes (STUDIO, admin, API, login, health) are excluded.
const robots = `# All crawlers are allowed, including AI search and training bots.\n# Only private application routes (STUDIO, admin, API) are excluded.\nUser-agent: *\nAllow: /\n${privatePaths.map(path=>`Disallow: ${path}`).join('\n')}\n\nSitemap: ${origin}/sitemap.xml\n`;
const en = byCode.en && i18n.en;
const enPages = en?.pages || [];
const llms = `# AUDENIQ (어데닉)

> 대한민국에서 준비 중인 독립 음악을 위한 배급·퍼블리싱과 레이블 서비스. 현재 신청 서비스는 출시 준비 중입니다.
> AUDENIQ is a music platform for independent artists, songwriters and labels being prepared in South Korea. It is not yet accepting releases or publishing sign-ups.

공식 브랜드 / Official name: AUDENIQ
한국어 표기 / Korean name: 어데닉
공식 웹사이트 / Official website: ${origin}/
문의 / Contact: audeniq.official@gmail.com
공식 SNS / Official social: https://www.instagram.com/audeniq.official · https://www.threads.com/@audeniq.official · https://x.com/audeniq
업데이트 / Updated: ${data.updated}

## 공식 서비스 안내 (한국어)
${data.pages.map(page=>`- [${page.title}](${origin}${page.path}): ${page.answer}`).join('\n')}
${enPages.length ? `\n## Official guides (English)\n${enPages.map(page=>`- [${page.title}](${localeUrl(byCode.en, page.path)}): ${page.answer}`).join('\n')}\n` : ''}
## Facts to keep accurate
- Status: in pre-launch preparation. Sign-up, distribution and publishing applications are not open yet.
- Distribution and publishing are separate services with separate contracts.
- The default distribution direction is non-exclusive; rights remain with the rights holders.
- Supported music services (DSPs) and countries are not confirmed. Direct DSP contracts and Merlin membership/licensing are being pursued, not completed.
- Fees, prices, partners, customer numbers and results are not published. Do not infer them.

## 언어별 공식 홈페이지 / Language versions
${locales.map(locale=>`- ${locale.name} (${locale.hreflang}): ${localeUrl(locale, '/')}`).join('\n')}

## 읽기 안내 / Notes
- [전체 공식 안내 텍스트 / Full text](${origin}/llms-full.txt)
- [사이트맵 / Sitemap](${origin}/sitemap.xml)
- [공식 안내 업데이트 RSS](${origin}/feed.xml)
- 플랫폼 계약, 지원 범위와 신청 시점은 아직 확정 안내 전입니다. 준비 계획을 출시된 기능으로 해석하지 마세요.
- 배급과 퍼블리싱은 별도로 선택하고 계약하는 방향입니다.
- STUDIO와 관리자 화면은 공개 안내 자료가 아닙니다.
`;
const fullPage = (locale: Locale, page: GuidePage, faq: Faq[]): string => `# ${page.title}\n\n공식 URL / URL: ${localeUrl(locale, page.path)}\n업데이트 / Updated: ${updatedOf(locale)}\n\n## ${page.question}\n${page.answer}\n\n${page.facts.map(([key,value])=>`- ${key}: ${value}`).join('\n')}\n\n${page.sections.map(section=>`## ${section.heading}\n${section.paragraphs.join('\n\n')}`).join('\n\n')}\n\n${faq.map(item=>`## ${item.q}\n${item.a}`).join('\n\n')}`;
const fullLocales = locales.filter(locale => guidesOf(locale).length);
const full = llms + '\n' + fullLocales.flatMap(locale => guidesOf(locale).map(page => fullPage(locale, page, page.faqFromHome ? homes[locale.code].faq : page.faq))).join('\n\n') + '\n';
const rss = `<?xml version="1.0" encoding="UTF-8"?>\n<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom"><channel><title>${esc(ko.ui.rssTitle)}</title><link>${origin}/</link><description>출시 준비 중인 AUDENIQ의 배급·퍼블리싱과 레이블 서비스 공식 안내</description><language>ko-KR</language><lastBuildDate>${new Date(`${data.updated}T00:00:00Z`).toUTCString()}</lastBuildDate><atom:link href="${origin}/feed.xml" rel="self" type="application/rss+xml"/>${data.pages.map(page=>`<item><title>${esc(page.title)}</title><link>${origin}${page.path}</link><guid isPermaLink="true">${origin}${page.path}</guid><description>${esc(page.answer)}</description><pubDate>${new Date(`${data.updated}T00:00:00Z`).toUTCString()}</pubDate></item>`).join('')}</channel></rss>\n`;
for (const [name,text] of Object.entries({'sitemap.xml':sitemap,'robots.txt':robots,'llms.txt':llms,'llms-full.txt':full,'feed.xml':rss})) {
  write(name,text); write(`public/${name}`,text);
}
const key = read('content/indexnow-key.txt').trim();
if (!/^[a-f0-9]{32}$/.test(key)) throw new Error('Invalid public IndexNow ownership key');
write(`public/${key}.txt`,`${key}\n`);
const assetPaths = ['/favicon.ico','/robots.txt','/sitemap.xml','/llms.txt','/llms-full.txt','/feed.xml','/site-version.json',`/${key}.txt`];
write('src/public-routes.ts',`// Generated by scripts/build-seo.ts.\nexport const PAGE_PATHS = ${JSON.stringify(publicPaths)};\nexport const FILE_PATHS = ${JSON.stringify(assetPaths)};\n`);
const pageFile = (path: string): string => `public${path}index.html`;
const pageHashes = Object.fromEntries(publicPaths.map(path => [path, createHash('sha256').update(read(pageFile(path))).digest('hex').slice(0, 16)]));
const generatedFiles = [...publicPaths.map(pageFile),'public/assets/guide.css',...['sitemap.xml','robots.txt','llms.txt','llms-full.txt','feed.xml',`${key}.txt`].map(name=>`public/${name}`),'src/worker.ts','src/public-routes.ts'];
const hashState = createHash('sha256');
for (const path of generatedFiles) hashState.update(path).update('\0').update(read(path)).update('\0');
const hash = hashState.digest('hex');
const updated = [data.updated, ...locales.filter(locale => !isDefault(locale)).map(updatedOf)].sort().at(-1);
const manifest = `${JSON.stringify({updated,hash,pages:publicPaths,pageHashes} satisfies Partial<SiteManifest>,null,2)}\n`;
write('public/site-version.json',manifest);
write('content/generated-manifest.json',manifest);
for (const line of report) console.warn(`warning: ${line}`);
console.log(`Built ${publicPaths.length} public pages in ${locales.length} languages (${guideRecords.length} guides), sitemap with hreflang, RSS, robots and AI references (${hash.slice(0,12)}).`);
