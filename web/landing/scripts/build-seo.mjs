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

const css = `:root{color-scheme:light;--ink:#202938;--muted:#59677c;--blue:#3456c4;--line:#dce3f2}*{box-sizing:border-box}body{margin:0;background:#fff;color:var(--ink);font:17px/1.85 system-ui,-apple-system,'Segoe UI',sans-serif;word-break:keep-all;overflow-wrap:anywhere}a{color:var(--blue);text-underline-offset:4px}a:hover{text-decoration-thickness:2px}:focus-visible{outline:3px solid var(--blue);outline-offset:5px}.wrap{width:calc(100% - 48px);max-width:920px;margin:auto}.skip{position:absolute;top:-80px;left:20px}.skip:focus{top:10px;background:#fff;padding:12px;z-index:2}header{border-bottom:1px solid var(--line)}.header-inner{display:flex;justify-content:space-between;align-items:center;gap:24px;padding-block:20px}.brand{font-weight:800;font-size:24px;letter-spacing:.02em;text-decoration:none}nav{display:flex;gap:20px;flex-wrap:wrap;font-size:14px}main{padding:36px 0 70px}.breadcrumb{font-size:14px;color:var(--muted)}.status{color:var(--blue);font-size:14px;font-weight:650;margin:28px 0 12px}h1{font-size:clamp(30px,5vw,46px);line-height:1.35;letter-spacing:-.03em;margin:0 0 28px}h2{font-size:26px;line-height:1.5;margin:0 0 18px}h3{font-size:20px;line-height:1.5}p{margin:0 0 18px;color:var(--muted)}.answer{background:#f1f5ff;border-radius:22px;padding:28px;margin-bottom:32px}.answer h2{font-size:21px}.answer p{font-size:19px;color:var(--ink);margin:0}.updated{font-size:14px}.table-scroll{overflow-x:auto;margin:24px 0 38px}table{border-collapse:collapse;width:100%;font-size:16px}caption{text-align:left;font-weight:700;padding:10px 0}th,td{text-align:left;vertical-align:top;border-bottom:1px solid var(--line);padding:16px 12px}th{width:25%;min-width:100px;background:#f8faff}section{margin-top:40px}details{border-bottom:1px solid var(--line);padding:18px 0}summary{cursor:pointer;font-weight:650}details p{margin:16px 0 0}.related{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:14px}.related a{border:1px solid var(--line);border-radius:14px;padding:20px;text-decoration:none}.contact{background:#f7f9fd;padding:26px;border-radius:18px}footer{border-top:1px solid var(--line);padding:28px 0;font-size:14px}footer nav{margin-top:14px}li{margin-bottom:8px}@media(max-width:600px){.wrap{width:calc(100% - 36px)}.header-inner{align-items:flex-start;flex-direction:column;gap:10px}nav{gap:12px 18px}.related{grid-template-columns:1fr}.answer{padding:22px}h2{font-size:23px}th,td{padding:12px 10px}body{font-size:16px}}`;
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
  const html = `<!doctype html>
<html lang="ko"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>${esc(title)}</title><meta name="description" content="${esc(page.description)}"><meta name="robots" content="index, follow, max-image-preview:large">
<link rel="canonical" href="${url}"><link rel="icon" type="image/svg+xml" href="/assets/favicon.svg"><link rel="stylesheet" href="/assets/guide.css">
<link rel="alternate" type="application/rss+xml" title="AUDENIQ 공식 안내 업데이트" href="${origin}/feed.xml">
<meta property="og:type" content="website"><meta property="og:locale" content="ko_KR"><meta property="og:site_name" content="AUDENIQ"><meta property="og:title" content="${esc(title)}"><meta property="og:description" content="${esc(page.description)}"><meta property="og:url" content="${url}"><meta property="og:image" content="${origin}/assets/social-preview.png"><meta property="og:image:width" content="1200"><meta property="og:image:height" content="630"><meta property="og:image:alt" content="AUDENIQ — 독립 음악을 위한 배급과 퍼블리싱 · 출시 준비 중">
<meta name="twitter:card" content="summary_large_image"><meta name="twitter:title" content="${esc(title)}"><meta name="twitter:description" content="${esc(page.description)}"><meta name="twitter:image" content="${origin}/assets/social-preview.png">
${schemaScript(graph)}
</head><body><a class="skip" href="#main">본문으로 건너뛰기</a><header><div class="wrap header-inner"><a class="brand" href="/">AUDENIQ</a><nav aria-label="주요 메뉴"><a href="/music-distribution/">음원 배급</a><a href="/publishing/">퍼블리싱</a><a href="/label-services/">레이블</a><a href="/faq/">FAQ</a></nav></div></header>
<main id="main" class="wrap"><div class="breadcrumb" aria-label="현재 위치"><a href="/">AUDENIQ</a> / ${esc(page.title)}</div>
<p class="status">AUDENIQ 공식 안내 · 서비스 출시 준비 중</p><h1>${esc(page.title)}</h1>
<div class="answer"><h2>${esc(page.question)}</h2><p>${esc(page.answer)}</p></div>
<p class="updated">업데이트 <time datetime="${data.updated}">${data.updated}</time> · 작성: AUDENIQ</p>
<div class="table-scroll"><table><caption>현재 준비 중인 서비스 안내</caption><tbody>${page.facts.map(([key,value])=>`<tr><th scope="row">${esc(key)}</th><td>${esc(value)}</td></tr>`).join('')}</tbody></table></div>
${page.sections.map(section=>`<section><h2>${esc(section.heading)}</h2>${section.paragraphs.map(text=>`<p>${esc(text)}</p>`).join('')}</section>`).join('\n')}
${page.faq.length ? `<section><h2>자주 묻는 질문</h2>${page.faq.map(faq=>`<details><summary>${esc(faq.q)}</summary><p>${esc(faq.a)}</p></details>`).join('\n')}</section>` : ''}
<section><h2>함께 확인할 공식 안내</h2><div class="related">${page.related.map(path=>{const related=pages.find(candidate=>candidate.path===path);if(!related)throw new Error(`Unknown related path ${path}`);return `<a href="${path}">${esc(related.title)} ↗</a>`;}).join('')}</div></section>
<section><h2>공식 정보 출처</h2><ul>${page.sources.map(source=>`<li><a href="${esc(source.url)}">${esc(source.name)}</a></li>`).join('')}</ul></section>
<section class="contact"><h2>서비스와 파트너십 문의</h2><p>신청 가능 시점과 확정된 지원 범위는 공식 홈페이지와 SNS에서 안내할 예정이에요.</p><a href="mailto:audeniq.official@gmail.com">audeniq.official@gmail.com ↗</a></section>
</main><footer><div class="wrap">© 2026 AUDENIQ · 서비스 출시 준비 중<nav aria-label="공식 링크"><a href="/about/">AUDENIQ 소개</a><a href="https://www.instagram.com/audeniq.official" rel="noopener noreferrer">Instagram</a><a href="https://www.threads.com/@audeniq.official" rel="noopener noreferrer">Threads</a><a href="https://x.com/audeniq" rel="noopener noreferrer">X</a><a href="/feed.xml">RSS</a></nav></div></footer></body></html>
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
