import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { createHash } from 'node:crypto';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = path => readFileSync(join(root, path), 'utf8');
const write = (path, text) => {
  mkdirSync(dirname(join(root, path)), { recursive: true });
  writeFileSync(join(root, path), text);
};
const esc = value => String(value).replace(/[&<>"']/g, char => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[char]));
const clean = text => text.replace(/<span[^>]*>[\s\S]*?<\/span>/g, '').replace(/<[^>]*>/g, '').replace(/&amp;/g, '&').trim();
const origin = 'https://audeniq.com';
const org = `${origin}/#organization`;
const site = `${origin}/#website`;
const data = JSON.parse(read('content/pages.json'));
let home = read('index.html');
home = home.replace('<p class="eyebrow">독립 음악을 위한 배급과 퍼블리싱</p>\n      <h1 id="hero-title">', '<h1 id="hero-title"><span class="hero-service eyebrow">독립 음악을 위한 배급과 퍼블리싱</span>');
const faqSection = home.match(/<section[^>]*id="faq"[\s\S]*?<\/section>/)?.[0];
if (!faqSection) throw new Error('Homepage FAQ section missing');
const homeFaq = [...faqSection.matchAll(/<details><summary>([\s\S]*?)<\/summary><p>([\s\S]*?)<\/p><\/details>/g)]
  .map(match => ({q:clean(match[1]), a:clean(match[2])}));
if (homeFaq.length !== 11) throw new Error('Review homepage FAQs before changing the generated reference');
const pages = data.pages.map(page => ({...page, faq: page.faqFromHome ? homeFaq : page.faq}));
const organization = {
  '@type':'Organization', '@id':org, name:'AUDENIQ', url:`${origin}/`,
  description:'대한민국에서 준비 중인 독립 음악을 위한 배급·퍼블리싱과 레이블 서비스',
  email:'audeniq.official@gmail.com',
  logo:{'@type':'ImageObject',url:`${origin}/assets/AUDENIQ_Logo_Light.svg`},
  sameAs:['https://www.instagram.com/audeniq.official','https://www.threads.com/@audeniq.official','https://x.com/audeniq']
};
const website = {'@type':'WebSite','@id':site,url:`${origin}/`,name:'AUDENIQ',inLanguage:'ko-KR',
  description:'독립 아티스트, 작곡가·작사가와 레이블을 위한 배급과 퍼블리싱 서비스를 준비하고 있어요.', publisher:{'@id':org}};
const questions = faqs => faqs.map(faq => ({'@type':'Question',name:faq.q,acceptedAnswer:{'@type':'Answer',text:faq.a}}));
const schemaScript = graph => `<script type="application/ld+json">${JSON.stringify({'@context':'https://schema.org','@graph':graph}).replace(/</g,'\\u003c')}</script>`;
// Keep discovery resources in the head and assets, without a directory/RSS block
// beneath the homepage FAQ. Service cards provide the relevant guide links.
home = home.replace(/<!-- SEARCH_GUIDES_START -->[\s\S]*?<!-- SEARCH_GUIDES_END -->\s*/g, '');
const homeGraph = [organization, website, {
  '@type':['WebPage','FAQPage'],'@id':`${origin}/#webpage`,url:`${origin}/`,name:'AUDENIQ | 독립 음악을 위한 배급과 퍼블리싱',
  inLanguage:'ko-KR',dateModified:data.updated,isPartOf:{'@id':site},about:{'@id':org},mainEntity:questions(homeFaq)
}];
home = home.replace(/<script type="application\/ld\+json">[\s\S]*?<\/script>/, schemaScript(homeGraph));
home = home.replace(/<!-- SEO_BUILD_START -->[\s\S]*?<!-- SEO_BUILD_END -->\s*/g, '');
home = home.replace('</head>', `<!-- SEO_BUILD_START -->\n<link rel="alternate" type="application/rss+xml" title="AUDENIQ 공식 안내 업데이트" href="${origin}/feed.xml">\n<style>.hero h1 .hero-service{display:block;font-size:15px;line-height:1.7;letter-spacing:.03em;font-weight:700;color:var(--blue);margin-bottom:22px}@media(max-width:680px){.hero h1 .hero-service{font-size:12px}}</style>\n<!-- SEO_BUILD_END -->\n</head>`);
write('index.html',home);
write('public/index.html',home);

const homepageStyles = [...home.matchAll(/<style(?:\s[^>]*)?>([\s\S]*?)<\/style>/g)].map(match=>match[1]).join('\n');
if (!homepageStyles.includes('--font-body') || !homepageStyles.includes('--blue:')) throw new Error('Homepage brand styles missing');
const css = homepageStyles + '\n' + read('content/guide.css');
const homepageBrand = home.match(/<a class="brand"[^>]*>([\s\S]*?)<\/a>/)?.[1];
const homepageFooter = home.match(/<footer\b[\s\S]*?<\/footer>/)?.[0];
if (!homepageBrand || !homepageFooter) throw new Error('Homepage brand header/footer missing');
// The homepage switches its legacy inline logo to this asset at runtime.
// Guides use the current logo immediately, including without JavaScript.
const currentLogo = html=>html.replace(/src="data:image\/svg\+xml;base64,[^"]+"/g,'src="/assets/AUDENIQ_Logo_Light.svg"').replace(/width="1600" height="520"/g,'width="1060" height="182"');
const brandImage = currentLogo(homepageBrand);
const footer = currentLogo(homepageFooter);
const guideLabels = {
  '/music-distribution/': {name:'음원 배급',eyebrow:'AUDENIQ DISTRIBUTION'},
  '/publishing/': {name:'퍼블리싱',eyebrow:'AUDENIQ PUBLISHING'},
  '/label-services/': {name:'레이블 서비스',eyebrow:'FOR INDEPENDENT LABELS'},
  '/about/': {name:'AUDENIQ 소개',eyebrow:'ABOUT AUDENIQ'},
  '/faq/': {name:'자주 묻는 질문',eyebrow:'AUDENIQ FAQ'}
};
write('assets/guide.css',css);
write('public/assets/guide.css',css);
for (const page of pages) {
  const url = origin + page.path;
  const title = `${page.title} | AUDENIQ`;
  const webpage = {
    '@type': page.kind === 'about' ? 'AboutPage' : page.faq.length ? ['WebPage','FAQPage'] : 'WebPage',
    '@id':`${url}#webpage`,url,name:title,description:page.description,inLanguage:'ko-KR',dateModified:data.updated,
    isPartOf:{'@id':site},about:{'@id':org},...(page.faq.length ? {mainEntity:questions(page.faq)} : {})
  };
  const graph = [organization,website,webpage,{'@type':'BreadcrumbList','@id':`${url}#breadcrumb`,itemListElement:[
    {'@type':'ListItem',position:1,name:'AUDENIQ',item:`${origin}/`},
    {'@type':'ListItem',position:2,name:page.title,item:url}]}];
  webpage.breadcrumb = {'@id':`${url}#breadcrumb`};
  if (page.kind === 'service') graph.push({'@type':'Service','@id':`${url}#service`,name:page.title,
    serviceType:page.serviceType,description:page.answer,url,provider:{'@id':org},
    audience:{'@type':'Audience',audienceType:page.audience}});
  const label = guideLabels[page.path];
  if (!label) throw new Error(`Missing guide label ${page.path}`);
  const navLinks = pages.map(item=>`<a href="${item.path}"${item.path===page.path?' aria-current="page"':''}>${esc(guideLabels[item.path].name)}</a>`).join('') + '<a class="nav-cta" href="#contact">문의</a>';
  const html = `<!doctype html>
<html lang="ko"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>${esc(title)}</title><meta name="description" content="${esc(page.description)}"><meta name="robots" content="index, follow, max-image-preview:large">
<link rel="canonical" href="${url}"><link rel="icon" type="image/svg+xml" href="/assets/favicon.svg"><link rel="preconnect" href="https://cdn.jsdelivr.net" crossorigin><link rel="stylesheet" href="https://cdn.jsdelivr.net/gh/orioncactus/pretendard@v1.3.9/dist/web/variable/pretendardvariable-dynamic-subset.min.css"><link rel="stylesheet" href="/assets/guide.css">
<link rel="alternate" type="application/rss+xml" title="AUDENIQ 공식 안내 업데이트" href="${origin}/feed.xml">
<meta property="og:type" content="website"><meta property="og:locale" content="ko_KR"><meta property="og:site_name" content="AUDENIQ"><meta property="og:title" content="${esc(title)}"><meta property="og:description" content="${esc(page.description)}"><meta property="og:url" content="${url}"><meta property="og:image" content="${origin}/assets/social-preview.png"><meta property="og:image:width" content="1200"><meta property="og:image:height" content="630"><meta property="og:image:alt" content="AUDENIQ — 독립 음악을 위한 배급과 퍼블리싱 · 출시 준비 중">
<meta name="twitter:card" content="summary_large_image"><meta name="twitter:title" content="${esc(title)}"><meta name="twitter:description" content="${esc(page.description)}"><meta name="twitter:image" content="${origin}/assets/social-preview.png">
${schemaScript(graph)}
</head><body class="guide-page"><a class="skip-link" href="#main">본문으로 바로가기</a>
<header class="site-header guide-header"><div class="nav-shell"><a class="brand" href="/" aria-label="AUDENIQ 홈">${brandImage}</a>
<nav class="primary-nav" aria-label="주 메뉴">${navLinks}</nav>
<details class="mobile-menu"><summary><span class="sr-only">메뉴 열기 및 닫기</span><svg viewBox="0 0 24 24" fill="none" aria-hidden="true"><path d="M3 8h18M3 16h18" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg></summary><nav aria-label="모바일 메뉴">${navLinks}</nav></details></div></header>
<main id="main"><div class="guide-breadcrumb container" aria-label="현재 위치"><a href="/">AUDENIQ</a><span aria-hidden="true">/</span><span>${esc(label.name)}</span></div>
<section class="hero wide guide-hero" id="top" aria-labelledby="guide-title"><div class="hero-copy"><p class="eyebrow">${esc(label.eyebrow)}</p><h1 id="guide-title">${esc(page.title)}</h1>
<div class="guide-intro"><h2>${esc(page.question)}</h2><p>${esc(page.answer)}</p></div>
<div class="guide-meta"><span class="pill">서비스 출시 준비 중</span><p class="small">AUDENIQ 공식 안내 · <time datetime="${data.updated}">${data.updated}</time></p></div></div></section>
<section class="section guide-overview" aria-labelledby="overview-title"><div class="container guide-facts-layout"><div class="guide-section-heading"><p class="eyebrow">AT A GLANCE</p><h2 id="overview-title">서비스를 한눈에.</h2><p>현재 준비 중인 내용과<br>확인할 점을 모아두었어요.</p></div>
<div class="guide-table"><table><caption>현재 준비 중인 서비스 안내</caption><tbody>${page.facts.map(([key,value])=>`<tr><th scope="row">${esc(key)}</th><td>${esc(value)}</td></tr>`).join('')}</tbody></table></div></div></section>
${page.sections.length ? `<section class="section guide-story" aria-label="${esc(label.name)} 상세 안내"><div class="container">${page.sections.map((section,index)=>`<div class="guide-story-row"><div class="guide-story-heading"><span class="service-no" aria-hidden="true">${String(index+1).padStart(2,'0')}</span><h2>${esc(section.heading)}</h2></div><div>${section.paragraphs.map(text=>`<p>${esc(text)}</p>`).join('')}</div></div>`).join('\n')}</div></section>` : ''}
${page.faq.length ? `<section class="section faq chapter-soft guide-faq" aria-labelledby="faq-title"><div class="container faq-layout"><div class="guide-section-heading"><p class="eyebrow">궁금한 점이 있나요?</p><h2 id="faq-title">자주 묻는 질문</h2><p>더 궁금한 점은<br><a href="mailto:audeniq.official@gmail.com">이메일로 편하게 물어보세요 ↗</a></p></div><div class="faq-list">${page.faq.map(faq=>`<details><summary>${esc(faq.q)}<span aria-hidden="true">＋</span></summary><p>${esc(faq.a)}</p></details>`).join('\n')}</div></div></section>` : ''}
<section class="section guide-related" aria-labelledby="related-title"><div class="container"><div class="section-heading"><p class="eyebrow">음악의 다음을 함께</p><h2 id="related-title">더 알아보세요.</h2></div><div class="service-grid">${page.related.map(path=>{const related=pages.find(candidate=>candidate.path===path);if(!related)throw new Error(`Unknown related path ${path}`);return `<a class="service-item" href="${path}"><div class="service-top"><span class="service-no">${esc(guideLabels[path].name)}</span><span class="pill">준비 중</span></div><h3>${esc(related.title)}</h3><p>${esc(related.answer)}</p><div class="service-caption"><strong>자세히 보기</strong><span aria-hidden="true">→</span></div></a>`;}).join('')}</div>
<div class="guide-sources"><span>공식 홈페이지에서 더 알아보기</span>${page.sources.map(source=>`<a href="${esc(source.url)}">${esc(source.name)} ↗</a>`).join('')}</div></div></section>
<section class="survey-section wide guide-contact" aria-labelledby="contact-title"><div class="survey-inner"><p class="eyebrow">YOUR MUSIC. YOUR NEXT.</p><h2 id="contact-title">당신의 음악,<br>다음을 함께 준비해요.</h2><p>서비스에 대한 질문과<br>음악 파트너의 협업 제안을 기다려요.</p><a class="button" href="mailto:audeniq.official@gmail.com">서비스 및 파트너십 문의 <span aria-hidden="true">↗</span></a><p class="small">신청 가능 시점과 확정된 지원 범위는<br>공식 홈페이지와 SNS에서 안내할 예정이에요.</p></div></section>
</main>${footer}</body></html>
`;
  write(`public${page.path}index.html`,html);
}

const publicPaths = ['/',...pages.map(page=>page.path)];
const sitemap = `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${publicPaths.map(path=>`  <url><loc>${origin}${path}</loc><lastmod>${data.updated}</lastmod></url>`).join('\n')}\n</urlset>\n`;
const privatePaths = ['/studio','/admin','/api','/login','/health'];
const searchBots = ['*','Googlebot','Bingbot','Yeti','OAI-SearchBot','ChatGPT-User','PerplexityBot','Perplexity-User','Claude-SearchBot','Claude-User'];
const robots = `# Public search is allowed. STUDIO and private application routes are excluded.\n${searchBots.map(bot=>`User-agent: ${bot}`).join('\n')}\nAllow: /\n${privatePaths.map(path=>`Disallow: ${path}`).join('\n')}\n\n# Search retrieval and model training are different uses.\n${['GPTBot','ClaudeBot','Google-Extended','CCBot','Applebot-Extended'].map(bot=>`User-agent: ${bot}`).join('\n')}\nDisallow: /\n\nSitemap: ${origin}/sitemap.xml\n`;
const llms = `# AUDENIQ\n\n> 대한민국에서 준비 중인 독립 음악을 위한 배급·퍼블리싱과 레이블 서비스. 현재 신청 서비스는 출시 준비 중입니다.\n\n공식 브랜드: AUDENIQ\n공식 웹사이트: ${origin}/\n문의: audeniq.official@gmail.com\n업데이트: ${data.updated}\n\n## 공식 서비스 안내\n${pages.map(page=>`- [${page.title}](${origin}${page.path}): ${page.answer}`).join('\n')}\n\n## 읽기 안내\n- [전체 공식 안내 텍스트](${origin}/llms-full.txt)\n- [사이트맵](${origin}/sitemap.xml)\n- [공식 안내 업데이트 RSS](${origin}/feed.xml)\n- 플랫폼 계약, 지원 범위와 신청 시점은 아직 확정 안내 전입니다. 준비 계획을 출시된 기능으로 해석하지 마세요.\n- 배급과 퍼블리싱은 별도로 선택하고 계약하는 방향입니다.\n- STUDIO와 관리자 화면은 공개 안내 자료가 아닙니다.\n`;
const full = llms + '\n' + pages.map(page=>`# ${page.title}\n\n공식 URL: ${origin}${page.path}\n업데이트: ${data.updated}\n\n## ${page.question}\n${page.answer}\n\n${page.facts.map(([key,value])=>`- ${key}: ${value}`).join('\n')}\n\n${page.sections.map(section=>`## ${section.heading}\n${section.paragraphs.join('\n\n')}`).join('\n\n')}\n\n${page.faq.map(faq=>`## ${faq.q}\n${faq.a}`).join('\n\n')}`).join('\n\n');
const rss = `<?xml version="1.0" encoding="UTF-8"?>\n<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom"><channel><title>AUDENIQ 공식 안내 업데이트</title><link>${origin}/</link><description>출시 준비 중인 AUDENIQ의 배급·퍼블리싱과 레이블 서비스 공식 안내</description><language>ko-KR</language><lastBuildDate>${new Date(`${data.updated}T00:00:00Z`).toUTCString()}</lastBuildDate><atom:link href="${origin}/feed.xml" rel="self" type="application/rss+xml"/>${pages.map(page=>`<item><title>${esc(page.title)}</title><link>${origin}${page.path}</link><guid isPermaLink="true">${origin}${page.path}</guid><description>${esc(page.answer)}</description><pubDate>${new Date(`${data.updated}T00:00:00Z`).toUTCString()}</pubDate></item>`).join('')}</channel></rss>\n`;
for (const [name,text] of Object.entries({'sitemap.xml':sitemap,'robots.txt':robots,'llms.txt':llms,'llms-full.txt':full,'feed.xml':rss})) {
  write(name,text); write(`public/${name}`,text);
}
const key = read('content/indexnow-key.txt').trim();
if (!/^[a-f0-9]{32}$/.test(key)) throw new Error('Invalid public IndexNow ownership key');
write(`public/${key}.txt`,`${key}\n`);
const assetPaths = ['/favicon.ico','/robots.txt','/sitemap.xml','/llms.txt','/llms-full.txt','/feed.xml','/site-version.json',`/${key}.txt`];
write('src/public-routes.js',`// Generated by scripts/build-seo.mjs.\nexport const PAGE_PATHS = ${JSON.stringify(publicPaths)};\nexport const FILE_PATHS = ${JSON.stringify(assetPaths)};\n`);
const generatedFiles = ['public/index.html',...pages.map(page=>`public${page.path}index.html`),'public/assets/guide.css',...['sitemap.xml','robots.txt','llms.txt','llms-full.txt','feed.xml',`${key}.txt`].map(name=>`public/${name}`),'src/worker.js','src/public-routes.js'];
const hashState = createHash('sha256');
for (const path of generatedFiles) hashState.update(path).update('\0').update(read(path)).update('\0');
const hash = hashState.digest('hex');
write('public/site-version.json',`${JSON.stringify({updated:data.updated,hash,pages:publicPaths},null,2)}\n`);
write('content/generated-manifest.json',`${JSON.stringify({updated:data.updated,hash,pages:publicPaths},null,2)}\n`);
console.log(`Built ${publicPaths.length} public pages, sitemap, RSS, robots and AI references (${hash.slice(0,12)}).`);
