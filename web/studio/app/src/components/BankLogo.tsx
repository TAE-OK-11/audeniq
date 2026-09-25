const INSTITUTION_INDEX: Record<string, [string, string, string]> = {
  'IBK기업은행': ['IBK기업은행', 'IBK', 'ibk'],
  '신한은행': ['신한은행', '신한', 'shinhan'],
  'KB국민은행': ['KB국민은행', 'KB', 'kb'],
  '하나은행': ['하나은행', '하나', 'hana'],
  '우리은행': ['우리은행', '우리', 'woori'],
  'NH농협은행': ['NH농협은행', 'NH', 'nh'],
  'SC제일은행': ['SC제일은행', 'SC', 'sc'],
  'KDB산업은행': ['KDB산업은행', 'KDB', 'kdb'],
  '카카오뱅크': ['카카오뱅크', 'B', 'kakao'],
  '부산은행': ['부산은행', 'BNK', 'bnk'],
  'iM뱅크': ['iM뱅크', 'iM', 'im'],
  '경남은행': ['경남은행', 'BNK', 'bnk'],
  '토스뱅크': ['토스뱅크', 'T', 'toss'],
  '케이뱅크': ['케이뱅크', 'K', 'kbank'],
  '수협은행': ['수협은행', 'Sh', 'suhyup'],
  '한국씨티은행': ['한국씨티은행', 'citi', 'citi'],
  '새마을금고': ['새마을금고', 'MG', 'mg'],
  '신협': ['신협', 'CU', 'cu'],
  '우체국': ['우체국', 'POST', 'post'],
  '산림조합': ['산림조합', 'SJ', 'forest'],
  '전북은행': ['전북은행', 'JB', 'jb'],
  '광주은행': ['광주은행', 'KJ', 'jb'],
  '제주은행': ['제주은행', '제주', 'jeju'],
  'SBI저축은행': ['SBI저축은행', 'SBI', 'sbi'],
  'OK저축은행': ['OK저축은행', 'OK', 'ok'],
  '한국투자저축은행': ['한국투자저축은행', '한국', 'korea'],
  '웰컴저축은행': ['웰컴저축은행', 'W', 'welcome'],
  '애큐온저축은행': ['애큐온저축은행', 'ACUON', 'acuon'],
  '페퍼저축은행': ['페퍼저축은행', 'PEPPER', 'pepper'],
  '신한저축은행': ['신한저축은행', '신한', 'shinhan'],
  'KB저축은행': ['KB저축은행', 'KB', 'kb'],
  '하나저축은행': ['하나저축은행', '하나', 'hana'],
  '우리금융저축은행': ['우리금융저축은행', '우리', 'woori'],
  'NH저축은행': ['NH저축은행', 'NH', 'nh'],
  'IBK저축은행': ['IBK저축은행', 'IBK', 'ibk'],
  'BNK저축은행': ['BNK저축은행', 'BNK', 'bnk'],
  '다올저축은행': ['다올저축은행', '다올', 'daol'],
  '키움YES저축은행': ['키움YES저축은행', 'YES', 'kiwoom'],
  'JT친애저축은행': ['JT친애저축은행', 'JT', 'jt'],
  'JT저축은행': ['JT저축은행', 'JT', 'jt'],
  '상상인저축은행': ['상상인저축은행', '상상인', 'sangsangin'],
  '상상인플러스저축은행': ['상상인플러스저축은행', '상상인+', 'sangsangin'],
  '대신저축은행': ['대신저축은행', '대신', 'daishin'],
  'DB저축은행': ['DB저축은행', 'DB', 'db'],
  'OSB저축은행': ['OSB저축은행', 'OSB', 'osb'],
  '모아저축은행': ['모아저축은행', 'MOA', 'moa'],
  '푸른저축은행': ['푸른저축은행', '푸른', 'pureun'],
  '스마트저축은행': ['스마트저축은행', 'SMART', 'smart'],
  '예가람저축은행': ['예가람저축은행', '예가람', 'yegaram'],
  '바로저축은행': ['바로저축은행', '바로', 'baro'],
  '유안타저축은행': ['유안타저축은행', '유안타', 'yuanta'],
  '동원제일저축은행': ['동원제일저축은행', '동원', 'dongwon'],
  '청주저축은행': ['청주저축은행', '청주', 'cheongju'],
  '미래에셋증권': ['미래에셋증권', '미래', 'mirae'],
  '한국투자증권': ['한국투자증권', '한국', 'korea'],
  'NH투자증권': ['NH투자증권', 'NH', 'nh'],
  '삼성증권': ['삼성증권', '삼성', 'samsung'],
  'KB증권': ['KB증권', 'KB', 'kb'],
  '신한투자증권': ['신한투자증권', '신한', 'shinhan'],
  '키움증권': ['키움증권', '키움', 'kiwoom'],
  '하나증권': ['하나증권', '하나', 'hana'],
  '메리츠증권': ['메리츠증권', 'meritz', 'meritz'],
  '대신증권': ['대신증권', '대신', 'daishin'],
  '토스증권': ['토스증권', 'T', 'toss'],
  '카카오페이증권': ['카카오페이증권', 'K', 'kakao'],
  '우리투자증권': ['우리투자증권', '우리', 'woori'],
  '한화투자증권': ['한화투자증권', '한화', 'hanwha'],
  '유안타증권': ['유안타증권', '유안타', 'yuanta'],
  '현대차증권': ['현대차증권', 'HMC', 'hyundai'],
  '교보증권': ['교보증권', '교보', 'kyobo'],
  'DB증권': ['DB증권', 'DB', 'db'],
  'iM증권': ['iM증권', 'iM', 'im'],
  'LS증권': ['LS증권', 'LS', 'ls'],
  'SK증권': ['SK증권', 'SK', 'sk'],
  '신영증권': ['신영증권', '신영', 'shinyoung'],
  '유진투자증권': ['유진투자증권', '유진', 'eugene'],
  'IBK투자증권': ['IBK투자증권', 'IBK', 'ibk'],
  'BNK투자증권': ['BNK투자증권', 'BNK', 'bnk'],
  '다올투자증권': ['다올투자증권', '다올', 'daol'],
  '부국증권': ['부국증권', '부국', 'bookook'],
  '흥국증권': ['흥국증권', '흥국', 'heungkuk'],
  '케이프투자증권': ['케이프투자증권', 'CAPE', 'cape'],
  '상상인증권': ['상상인증권', '상상인', 'sangsangin'],
};

const LOGO_COLORS: Record<string, string> = {ibk:'#1678b8',shinhan:'#2864dc',kb:'#f2a900',hana:'#009b77',woori:'#0786ca',nh:'#f5b400',sc:'#16a085',kakao:'#ffd900',toss:'#3478f6',kbank:'#1834b8',kdb:'#1676bd',suhyup:'#0876bb',citi:'#056dae',im:'#00a789',bnk:'#e52335',mg:'#168bc2',cu:'#0875bb',post:'#e4362c',forest:'#258550',savings:'#53657a',sbi:'#1569ae',ok:'#e81f2e',korea:'#7a321f',welcome:'#ef3c55',acuon:'#6846d9',pepper:'#ef3c4f',daol:'#111',kiwoom:'#ec268f',jt:'#2a7fba',sangsangin:'#f15b38',daishin:'#666',db:'#16a34a',osb:'#da2031',moa:'#2361a9',pureun:'#2891cd',smart:'#2364ad',yegaram:'#713d91',baro:'#ec5d2f',yuanta:'#1384c7',dongwon:'#1976b9',cheongju:'#314f8b',mirae:'#f58220',samsung:'#064bd8',meritz:'#ed3024',hanwha:'#f58232',hyundai:'#23549c',kyobo:'#46b43d',ls:'#173f88',sk:'#e22922',shinyoung:'#243a78',eugene:'#c61d23',bookook:'#173f88',heungkuk:'#dd1c48',cape:'#2d5c9b',jb:'#1865a4',jeju:'#245fbb'};

function esc(s: string): string {
  return String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c] || c));
}

function bankMark(name: string): string {
  const item = INSTITUTION_INDEX[name] || [name, String(name || '?').slice(0, 3), 'savings'];
  const code = item[1], type = item[2], color = LOGO_COLORS[type] || '#315bd7';
  let mark = `<text x="24" y="28" text-anchor="middle" font-size="${code.length > 4 ? 8 : code.length > 2 ? 10 : 14}" font-family="Arial,sans-serif" font-weight="800" fill="${color}">${esc(code)}</text>`;
  if (type === 'ibk') mark = `<path d="M8 12 34 8l8 9-6 19-26 4-5-10Z" fill="${color}"/><text x="24" y="28" text-anchor="middle" font-size="10" font-family="Arial,sans-serif" font-weight="900" fill="#fff">IBK</text>`;
  if (type === 'shinhan') mark = `<circle cx="24" cy="24" r="18" fill="${color}"/><path d="M14 27c8-14 12 9 21-7-3 13-13 18-21 7Z" fill="#fff"/>`;
  if (type === 'kb') mark = `<g stroke="${color}" stroke-width="4" stroke-linecap="round"><path d="M13 13v22M8 19l20 12M8 31l20-12"/></g>`;
  if (type === 'hana') mark = `<circle cx="24" cy="13" r="3" fill="#e23b3b"/><path d="M11 19c8 0 9 7 13 7s5-7 13-7M24 20v17" fill="none" stroke="${color}" stroke-width="4" stroke-linecap="round"/>`;
  if (type === 'woori') mark = `<circle cx="24" cy="24" r="18" fill="#b8d8e7"/><path d="M7 27c7-9 24-13 34-5-7 12-24 18-34 5Z" fill="${color}"/>`;
  if (type === 'nh') mark = `<circle cx="24" cy="27" r="10" fill="none" stroke="${color}" stroke-width="5"/><path d="M13 13l11 8 11-8M18 10l6 11 6-11" fill="none" stroke="${color}" stroke-width="4" stroke-linejoin="round"/>`;
  if (type === 'sc') mark = `<path d="M16 9c14 0 15 9 6 15-8 5-5 13 8 15" fill="none" stroke="#13a89e" stroke-width="6" stroke-linecap="round"/><path d="M31 9c-13 1-15 10-6 15 8 5 5 13-8 15" fill="none" stroke="#168aca" stroke-width="6" stroke-linecap="round"/>`;
  if (type === 'kakao') mark = `<rect x="8" y="8" width="32" height="32" rx="7" fill="${color}"/><text x="24" y="31" text-anchor="middle" font-size="20" font-weight="900" fill="#171717">B</text>`;
  if (type === 'toss') mark = `<circle cx="24" cy="24" r="18" fill="${color}"/><path d="M14 25c7-1 11-7 15-13 6 6 7 12 3 17-4 5-12 7-18 3Z" fill="#fff"/>`;
  if (type === 'kbank') mark = `<text x="24" y="31" text-anchor="middle" font-size="24" font-family="Arial,sans-serif" font-weight="900" fill="${color}">K</text>`;
  if (type === 'kdb') mark = `<path d="M8 24 19 9h12L20 39Z" fill="#1685c3"/><path d="m40 24-11 15H17L28 9Z" fill="#2651a2" opacity=".8"/>`;
  if (type === 'citi') mark = `<path d="M15 18c5-7 15-7 20 0" fill="none" stroke="#e1262f" stroke-width="2.4"/><text x="24" y="31" text-anchor="middle" font-size="14" font-family="Arial,sans-serif" font-weight="800" fill="${color}">citi</text>`;
  if (type === 'im') mark = `<path d="M9 29V17h8v12m4 0V17c8 0 12 4 18 10" fill="none" stroke="${color}" stroke-width="5" stroke-linecap="round"/>`;
  if (type === 'bnk') mark = `<text x="24" y="29" text-anchor="middle" font-size="13" font-family="Arial,sans-serif" font-weight="900" font-style="italic" fill="${color}">BNK</text>`;
  if (type === 'mg') mark = `<circle cx="18" cy="19" r="8" fill="#29a6d5"/><circle cx="30" cy="19" r="8" fill="#1179bb"/><circle cx="24" cy="30" r="8" fill="#4db9de"/>`;
  if (type === 'post') mark = `<path d="M8 17h31l-9 6 9 5H8l10-5Z" fill="${color}"/>`;
  if (type === 'forest') mark = `<path d="m24 8 8 12h-5l8 12H13l8-12h-5Z" fill="${color}"/><path d="M22 31h4v9h-4z" fill="#6b4b2a"/>`;
  if (type === 'jb') mark = `<path d="M10 14h13v13H10zM25 14h13v13H25zM10 29h13v7H10z" fill="${color}"/><path d="M25 29h13L25 40z" fill="#69a7d8"/>`;
  return mark;
}

export function BankLogo({ name }: { name: string }) {
  return (
    <span className="aq-bank-logo" aria-hidden="true">
      <svg viewBox="0 0 48 48" role="img" dangerouslySetInnerHTML={{ __html: bankMark(name) }} />
    </span>
  );
}
