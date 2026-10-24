// 홍보 이미지 — 인스타그램 스토리(9:16)·피드(4:5)에 바로 올릴 수 있는 한 장.
// 흐린 커버 바탕 위에 커버 · 상태(발매 예정 날짜 또는 지금 들을 수 있어요) · 제목 · 아티스트, 아래에 QR과 스마트링크 주소.
// 커버가 없으면 AUDENIQ 파랑 그라데이션. 글자는 화면과 같은 글꼴(Pretendard)로 그린다.
// D-day 이미지(dday)는 같은 판에 큰 'D-7'을 얹은 것 — 발매 전에 D-7·D-3·D-1·당일 이미지를 미리 만들어 둘 수 있다.
// 바탕·아래 QR 알약은 가사 카드(lyricCard.ts)도 같이 쓴다.
import { drawQr, qrMatrix } from './qr';
import { daysBetween } from './home';
import { todayStr } from './date';

export type PromoFormat = 'story' | 'feed';
export const PROMO_SIZE: Record<PromoFormat, { w: number; h: number; label: string }> = {
  story: { w: 1080, h: 1920, label: '스토리 9:16' },
  feed: { w: 1080, h: 1350, label: '피드 4:5' },
};

export interface PromoInput {
  title: string; artist: string; releaseDate: string | null; url: string; cover?: string;
  /** D-day 이미지: 발매까지 남은 날 (0이면 당일) */
  dday?: number;
  /** 동시 발매 시각 'HH:MM' (D-day 이미지 날짜 줄에) */
  releaseTime?: string | null;
}

/** 큰 글자 — 'D-7', 당일은 'D-DAY' */
export const ddayLabel = (n: number) => (n > 0 ? `D-${n}` : 'D-DAY');

/** D-day 이미지 날짜 줄 — '11월 15일 18:00 발매', 당일은 '오늘 발매' (+시각) */
export function ddayKicker(releaseDate: string, n: number, time?: string | null): string {
  const t = time && /^\d{2}:\d{2}$/.test(time) ? ` ${time}` : '';
  if (n <= 0) return `오늘${t} 발매`;
  return `${Number(releaseDate.slice(5, 7))}월 ${Number(releaseDate.slice(8, 10))}일${t} 발매`;
}

/** 발매 전이면 고를 수 있는 D-day — 오늘부터 발매일까지 남은 것만 (D-7·D-3·D-1·당일 중) */
export function ddayChoices(releaseDate: string | null, today = todayStr()): number[] {
  if (!releaseDate) return [];
  const left = daysBetween(today, releaseDate);
  return left < 0 ? [] : [7, 3, 1, 0].filter(n => n <= left);
}

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

export async function loadImage(src: string): Promise<HTMLImageElement> {
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

export function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
}

export interface Canvas { canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D; w: number; h: number; font: string; story: boolean }

/** 판 — 크기, 글꼴 */
export async function makeCanvas(format: PromoFormat): Promise<Canvas> {
  const { w, h } = PROMO_SIZE[format];
  const canvas = document.createElement('canvas');
  canvas.width = w; canvas.height = h;
  const ctx = canvas.getContext('2d')!;
  await document.fonts?.ready;
  const font = getComputedStyle(document.body).fontFamily || 'sans-serif';
  return { canvas, ctx, w, h, font, story: format === 'story' };
}

/** 바탕 — 커버를 크게 흐리게 + 어둡게, 없으면 AUDENIQ 파랑 */
export function paintBackground({ ctx, w, h }: Canvas, cover: HTMLImageElement | null) {
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
}

/** 커버 한 장 (없으면 파랑 그라데이션 + 음표 — 스튜디오의 빈 커버와 같은 모양) */
export function paintCover({ ctx }: Canvas, cover: HTMLImageElement | null, x: number, y: number, size: number, radius: number, shadow = true) {
  if (shadow) {
    ctx.save();
    ctx.shadowColor = 'rgba(0,0,0,.35)'; ctx.shadowBlur = size / 12; ctx.shadowOffsetY = size / 30;
    roundRect(ctx, x, y, size, size, radius);
    ctx.fillStyle = '#fff'; ctx.fill();
    ctx.restore();
  }
  ctx.save();
  roundRect(ctx, x, y, size, size, radius); ctx.clip();
  if (cover) ctx.drawImage(cover, x, y, size, size);
  else {
    const g = ctx.createLinearGradient(x, y, x + size, y + size);
    g.addColorStop(0, '#9DB4FF'); g.addColorStop(1, '#3B63F3');
    ctx.fillStyle = g; ctx.fillRect(x, y, size, size);
    // Glyph 'music'과 같은 선 (24칸 기준)
    const k = size * 0.34 / 24;
    ctx.translate(x + size / 2 - 12 * k, y + size / 2 - 12 * k);
    ctx.scale(k, k);
    ctx.strokeStyle = '#fff'; ctx.lineWidth = 2; ctx.lineCap = 'round'; ctx.lineJoin = 'round';
    ctx.stroke(new Path2D('M9 18V6l11-2v12'));
    ctx.beginPath(); ctx.arc(6.5, 18, 2.5, 0, Math.PI * 2); ctx.stroke();
    ctx.beginPath(); ctx.arc(17.5, 16, 2.5, 0, Math.PI * 2); ctx.stroke();
  }
  ctx.restore();
}

/** 아래 흰 알약 자리 */
export function pillBox({ w, h, story }: Canvas) {
  const ph = story ? 210 : 170;
  const pw = w - (story ? 160 : 140);
  return { px: (w - pw) / 2, py: h - ph - (story ? 150 : 60), pw, ph };
}

/** 아래 — 흰 알약 안에 QR · '링크에서 듣기' · 주소, 맨 아래 AUDENIQ */
export function paintLinkPill(c: Canvas, url: string) {
  const { ctx, w, h, font, story } = c;
  const { px, py, pw, ph } = pillBox(c);
  roundRect(ctx, px, py, pw, ph, 40);
  ctx.fillStyle = '#fff'; ctx.fill();
  const qs = ph - 40;
  drawQr(ctx, qrMatrix(url), px + 20, py + 20, qs);
  ctx.textAlign = 'left';
  const tx = px + 20 + qs + 28;
  ctx.fillStyle = '#191F28';
  ctx.font = `800 ${story ? 44 : 38}px ${font}`;
  ctx.fillText('링크에서 듣기', tx, py + ph / 2 - 6);
  ctx.fillStyle = '#5B6475';
  ctx.font = `600 ${story ? 30 : 26}px ${font}`;
  const shown = url.replace(/^https?:\/\//, '');
  ctx.fillText(wrapText(s => ctx.measureText(s).width, shown, px + pw - tx - 28, 1)[0] ?? shown, tx, py + ph / 2 + (story ? 44 : 38));
  ctx.textAlign = 'center';
  ctx.fillStyle = 'rgba(255,255,255,.7)';
  ctx.font = `800 ${story ? 28 : 24}px ${font}`;
  ctx.fillText('AUDENIQ', w / 2, h - (story ? 70 : 22));
}

export function toPng(canvas: HTMLCanvasElement): Promise<Blob> {
  return new Promise((resolve, reject) => canvas.toBlob(b => (b ? resolve(b) : reject(new Error('이미지를 만들지 못했어요.'))), 'image/png'));
}

/** 이미지 한 장을 그려 PNG로 */
export async function renderPromo(input: PromoInput, format: PromoFormat): Promise<Blob> {
  const c = await makeCanvas(format);
  const { ctx, w, font, story } = c;
  const cover = input.cover ? await loadImage(input.cover).catch(() => null) : null;
  paintBackground(c, cover);

  // D-day 이미지는 큰 숫자 자리를 내려고 커버를 조금 작게
  const dday = input.dday !== undefined && input.releaseDate ? input.dday : undefined;
  const cs = dday !== undefined ? (story ? 600 : 460) : (story ? 760 : 640);
  const cx = (w - cs) / 2;
  const cy = story ? 230 : (dday !== undefined ? 90 : 110);
  paintCover(c, cover, cx, cy, cs, 36);
  const { py } = pillBox(c);

  // 글자 — (D-day 큰 숫자) · 상태 · 제목(두 줄까지) · 아티스트
  ctx.textAlign = 'center';
  ctx.textBaseline = 'alphabetic';
  const dSize = story ? 200 : 140;
  const kSize = story ? 40 : 32;
  const tSize = dday !== undefined ? (story ? 72 : 56) : (story ? 88 : 66);
  const aSize = story ? 48 : 38;
  const tGap = Math.round(tSize * 1.18);
  ctx.font = `800 ${tSize}px ${font}`;
  const titleLines = wrapText(s => ctx.measureText(s).width, input.title || '제목 없는 발매', w - 160, dday !== undefined ? 1 : 2);
  const ddayH = dday !== undefined ? Math.round(dSize * 0.9) + 20 : 0;
  const blockH = ddayH + kSize + 28 + tGap * titleLines.length + (input.artist ? aSize + 18 : 0);
  let y = cy + cs + Math.max(story ? 70 : 48, (py - (cy + cs) - blockH) / 2);
  if (dday !== undefined) {
    y += Math.round(dSize * 0.9);
    ctx.fillStyle = '#fff';
    ctx.font = `900 ${dSize}px ${font}`;
    ctx.fillText(ddayLabel(dday), w / 2, y);
    y += 20;
  }
  y += kSize;
  ctx.fillStyle = 'rgba(255,255,255,.8)';
  ctx.font = `700 ${kSize}px ${font}`;
  ctx.fillText(dday !== undefined ? ddayKicker(input.releaseDate!, dday, input.releaseTime) : promoKicker(input.releaseDate), w / 2, y);
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

  paintLinkPill(c, input.url);
  return toPng(c.canvas);
}
