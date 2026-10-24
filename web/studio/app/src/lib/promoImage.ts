// 홍보 이미지 — 인스타그램 스토리(9:16)·피드(4:5)에 바로 올릴 수 있는 한 장.
// 흐린 커버 바탕 위에 커버 · 상태(발매 예정 날짜 또는 지금 들을 수 있어요) · 제목 · 아티스트, 아래에 QR과 스마트링크 주소.
// 커버가 없으면 AUDENIQ 파랑 그라데이션. 글자는 화면과 같은 글꼴(Pretendard)로 그린다.
import { drawQr, qrMatrix } from './qr';
import { daysBetween } from './home';
import { todayStr } from './date';

export type PromoFormat = 'story' | 'feed';
export const PROMO_SIZE: Record<PromoFormat, { w: number; h: number; label: string }> = {
  story: { w: 1080, h: 1920, label: '스토리 9:16' },
  feed: { w: 1080, h: 1350, label: '피드 4:5' },
};

export interface PromoInput { title: string; artist: string; releaseDate: string | null; url: string; cover?: string }

/** 위 작은 글자 — 발매 전이면 '11월 15일 발매 · D-22', 당일 '오늘 발매', 지난 뒤 '지금 들을 수 있어요' */
export function promoKicker(releaseDate: string | null, today = todayStr()): string {
  if (!releaseDate) return '지금 들을 수 있어요';
  const d = daysBetween(today, releaseDate);
  if (d > 0) return `${Number(releaseDate.slice(5, 7))}월 ${Number(releaseDate.slice(8, 10))}일 발매 · D-${d}`;
  if (d === 0) return '오늘 발매';
  return '지금 들을 수 있어요';
}

/** 폭에 맞춰 최대 lines줄, 넘치면 마지막 줄 끝을 … */
export function wrapText(measure: (s: string) => number, text: string, max: number, lines: number): string[] {
  const out: string[] = [];
  let rest = text.trim();
  while (rest && out.length < lines) {
    let n = rest.length;
    while (n > 1 && measure(rest.slice(0, n)) > max) n--;
    // 단어 중간에서 끊지 않게 — 공백이 있으면 그 앞에서
    if (n < rest.length) {
      const sp = rest.lastIndexOf(' ', n);
      if (sp > 0) n = sp;
    }
    let line = rest.slice(0, n).trim();
    rest = rest.slice(n).trim();
    if (rest && out.length === lines - 1) {
      while (line.length > 1 && measure(`${line}…`) > max) line = line.slice(0, -1);
      line = `${line}…`;
      rest = '';
    }
    out.push(line);
  }
  return out;
}

async function loadImage(src: string): Promise<HTMLImageElement> {
  // 다른 주소의 그림은 캔버스를 '오염'시켜 저장이 막힌다 — 받아서 같은 페이지의 blob으로 그린다
  let url = src;
  let made = '';
  if (/^https?:/.test(src) && !src.startsWith(window.location.origin)) {
    const r = await fetch(src, { credentials: 'include' });
    if (!r.ok) throw new Error('cover');
    made = url = URL.createObjectURL(await r.blob());
  }
  try {
    const img = new Image();
    img.decoding = 'async';
    img.src = url;
    await img.decode();
    return img;
  } finally {
    if (made) window.setTimeout(() => URL.revokeObjectURL(made), 1000);
  }
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
}

/** 이미지 한 장을 그려 PNG로 */
export async function renderPromo(input: PromoInput, format: PromoFormat): Promise<Blob> {
  const { w, h } = PROMO_SIZE[format];
  const canvas = document.createElement('canvas');
  canvas.width = w; canvas.height = h;
  const ctx = canvas.getContext('2d')!;
  await document.fonts?.ready;
  const font = getComputedStyle(document.body).fontFamily || 'sans-serif';
  const cover = input.cover ? await loadImage(input.cover).catch(() => null) : null;

  // 바탕 — 커버를 크게 흐리게 + 어둡게, 없으면 AUDENIQ 파랑
  const bg = ctx.createLinearGradient(0, 0, w, h);
  bg.addColorStop(0, '#3B63F3'); bg.addColorStop(1, '#14204A');
  ctx.fillStyle = bg; ctx.fillRect(0, 0, w, h);
  if (cover && 'filter' in ctx) {
    ctx.save();
    ctx.filter = 'blur(60px) saturate(1.2)';
    const s = Math.max(w, h) * 1.25;
    ctx.drawImage(cover, (w - s) / 2, (h - s) / 2, s, s);
    ctx.restore();
    ctx.fillStyle = 'rgba(10,16,36,.42)'; ctx.fillRect(0, 0, w, h);
  }

  // 커버 (없으면 파랑 그라데이션 + 음표 — 스튜디오의 빈 커버와 같은 모양)
  const story = format === 'story';
  const cs = story ? 760 : 640;
  const cx = (w - cs) / 2;
  const cy = story ? 230 : 110;
  ctx.save();
  ctx.shadowColor = 'rgba(0,0,0,.35)'; ctx.shadowBlur = 60; ctx.shadowOffsetY = 24;
  roundRect(ctx, cx, cy, cs, cs, 36);
  ctx.fillStyle = '#fff'; ctx.fill();
  ctx.restore();
  ctx.save();
  roundRect(ctx, cx, cy, cs, cs, 36); ctx.clip();
  if (cover) ctx.drawImage(cover, cx, cy, cs, cs);
  else {
    const g = ctx.createLinearGradient(cx, cy, cx + cs, cy + cs);
    g.addColorStop(0, '#9DB4FF'); g.addColorStop(1, '#3B63F3');
    ctx.fillStyle = g; ctx.fillRect(cx, cy, cs, cs);
    // Glyph 'music'과 같은 선 (24칸 기준)
    const k = cs * 0.34 / 24;
    ctx.translate(cx + cs / 2 - 12 * k, cy + cs / 2 - 12 * k);
    ctx.scale(k, k);
    ctx.strokeStyle = '#fff'; ctx.lineWidth = 2; ctx.lineCap = 'round'; ctx.lineJoin = 'round';
    ctx.stroke(new Path2D('M9 18V6l11-2v12'));
    ctx.beginPath(); ctx.arc(6.5, 18, 2.5, 0, Math.PI * 2); ctx.stroke();
    ctx.beginPath(); ctx.arc(17.5, 16, 2.5, 0, Math.PI * 2); ctx.stroke();
  }
  ctx.restore();

  // 아래 흰 알약 자리 (먼저 정해 두고 글 묶음을 커버와 그 사이 가운데에)
  const ph = story ? 210 : 170;
  const pw = w - (story ? 160 : 140);
  const px = (w - pw) / 2;
  const py = h - ph - (story ? 150 : 60);

  // 글자 — 상태 · 제목(두 줄까지) · 아티스트
  ctx.textAlign = 'center';
  ctx.textBaseline = 'alphabetic';
  const kSize = story ? 40 : 32;
  const tSize = story ? 88 : 66;
  const aSize = story ? 48 : 38;
  const tGap = Math.round(tSize * 1.18);
  ctx.font = `800 ${tSize}px ${font}`;
  const titleLines = wrapText(s => ctx.measureText(s).width, input.title || '제목 없는 발매', w - 160, 2);
  const blockH = kSize + 28 + tGap * titleLines.length + (input.artist ? aSize + 18 : 0);
  let y = cy + cs + Math.max(story ? 70 : 56, (py - (cy + cs) - blockH) / 2) + kSize;
  ctx.fillStyle = 'rgba(255,255,255,.8)';
  ctx.font = `700 ${kSize}px ${font}`;
  ctx.fillText(promoKicker(input.releaseDate), w / 2, y);
  y += 28 + tSize;
  ctx.fillStyle = '#fff';
  ctx.font = `800 ${tSize}px ${font}`;
  for (const line of titleLines) {
    ctx.fillText(line, w / 2, y);
    y += tGap;
  }
  if (input.artist) {
    ctx.fillStyle = 'rgba(255,255,255,.86)';
    ctx.font = `600 ${aSize}px ${font}`;
    ctx.fillText(wrapText(s => ctx.measureText(s).width, input.artist, w - 160, 1)[0] ?? '', w / 2, y - tGap + aSize + 26);
  }

  // 아래 — 흰 알약 안에 QR · '링크에서 듣기' · 주소
  roundRect(ctx, px, py, pw, ph, 40);
  ctx.fillStyle = '#fff'; ctx.fill();
  const qs = ph - 40;
  drawQr(ctx, qrMatrix(input.url), px + 20, py + 20, qs);
  ctx.textAlign = 'left';
  const tx = px + 20 + qs + 28;
  ctx.fillStyle = '#191F28';
  ctx.font = `800 ${story ? 44 : 38}px ${font}`;
  ctx.fillText('링크에서 듣기', tx, py + ph / 2 - 6);
  ctx.fillStyle = '#5B6475';
  ctx.font = `600 ${story ? 30 : 26}px ${font}`;
  const shown = input.url.replace(/^https?:\/\//, '');
  ctx.fillText(wrapText(s => ctx.measureText(s).width, shown, px + pw - tx - 28, 1)[0] ?? shown, tx, py + ph / 2 + (story ? 44 : 38));
  ctx.textAlign = 'center';
  ctx.fillStyle = 'rgba(255,255,255,.7)';
  ctx.font = `800 ${story ? 28 : 24}px ${font}`;
  ctx.fillText('AUDENIQ', w / 2, h - (story ? 70 : 22));

  return new Promise((resolve, reject) => canvas.toBlob(b => (b ? resolve(b) : reject(new Error('이미지를 만들지 못했어요.'))), 'image/png'));
}
