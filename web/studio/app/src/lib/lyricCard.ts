// 가사 카드 — 가사 몇 줄을 크게 얹은 인스타그램 스토리(9:16)·피드(4:5) 이미지.
// 홍보 이미지와 같은 바탕(흐린 커버)·아래 QR 알약을 쓰고, 위에는 작은 커버와 곡 제목·아티스트.
import { loadImage, makeCanvas, paintBackground, paintCover, paintLinkPill, pillBox, toPng, wrapText, type PromoFormat } from './promoImage';

export interface LyricCardInput { title: string; artist: string; lines: string[]; url: string; cover?: string }

/** 고를 수 있는 가사 줄 — 빈 줄은 빼고, 같은 줄이 이어지면 한 번만 (후렴 반복) */
export function lyricLines(lyrics: string): string[] {
  const out: string[] = [];
  for (const raw of lyrics.split(/\r?\n/)) {
    const line = raw.trim();
    if (line && line !== out[out.length - 1]) out.push(line);
  }
  return out;
}

/** 한 장에 넣는 줄 수 */
export const LYRIC_MAX_LINES = 4;

export async function renderLyricCard(input: LyricCardInput, format: PromoFormat): Promise<Blob> {
  const c = await makeCanvas(format);
  const { ctx, w, font, story } = c;
  const cover = input.cover ? await loadImage(input.cover).catch(() => null) : null;
  paintBackground(c, cover);

  // 위 — 작은 커버 · 제목 · 아티스트
  const pad = story ? 90 : 80;
  const top = story ? 200 : 90;
  const cs = story ? 132 : 112;
  paintCover(c, cover, pad, top, cs, 20, false);
  ctx.textAlign = 'left';
  ctx.textBaseline = 'alphabetic';
  const tx = pad + cs + 28;
  ctx.fillStyle = '#fff';
  ctx.font = `800 ${story ? 44 : 38}px ${font}`;
  ctx.fillText(wrapText(s => ctx.measureText(s).width, input.title || '제목 없는 곡', w - tx - pad, 1)[0] ?? '', tx, top + cs / 2 - 4);
  if (input.artist) {
    ctx.fillStyle = 'rgba(255,255,255,.78)';
    ctx.font = `600 ${story ? 34 : 30}px ${font}`;
    ctx.fillText(wrapText(s => ctx.measureText(s).width, input.artist, w - tx - pad, 1)[0] ?? '', tx, top + cs / 2 + (story ? 44 : 38));
  }

  // 가운데 — 따옴표와 가사 (줄마다 폭에 맞춰 두 줄까지)
  const size = story ? 66 : 54;
  const gap = Math.round(size * 1.5);
  ctx.font = `800 ${size}px ${font}`;
  const rows = input.lines.slice(0, LYRIC_MAX_LINES).flatMap(l => wrapText(s => ctx.measureText(s).width, l, w - pad * 2, 2));
  const { py } = pillBox(c);
  const areaTop = top + cs + (story ? 80 : 50);
  const quote = story ? 150 : 120;
  const blockH = quote + gap * rows.length;
  let y = areaTop + Math.max(0, (py - areaTop - blockH) / 2);
  ctx.fillStyle = 'rgba(157,180,255,.9)';
  ctx.font = `900 ${quote * 1.6}px Georgia, serif`;
  ctx.fillText('“', pad - 8, y + quote);
  y += quote + size;
  ctx.fillStyle = '#fff';
  ctx.font = `800 ${size}px ${font}`;
  for (const r of rows) {
    ctx.fillText(r, pad, y);
    y += gap;
  }

  paintLinkPill(c, input.url);
  return toPng(c.canvas);
}
