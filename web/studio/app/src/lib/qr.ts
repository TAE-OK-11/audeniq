// QR 코드 — 스마트링크·아티스트 소개 페이지 주소를 포스터·굿즈·공연장에 붙일 이미지로.
// 오류 정정 M(약 15% 가려져도 읽힘), 가장자리 여백 4칸(QR 규격의 조용한 영역), 흰 바탕에 짙은 남색 점.
import qrcode from 'qrcode-generator';

export const QR_INK = '#191F28';

/** 주소 → 칸마다 칠할지 (행 우선) */
export function qrMatrix(text: string): boolean[][] {
  const q = qrcode(0, 'M');
  q.addData(text);
  q.make();
  const n = q.getModuleCount();
  return Array.from({ length: n }, (_, r) => Array.from({ length: n }, (_, c) => q.isDark(r, c)));
}

/** 캔버스의 (x, y)에 한 변 size 픽셀로 그린다 — 여백 4칸 포함, 바탕은 흰색 */
export function drawQr(ctx: CanvasRenderingContext2D, matrix: boolean[][], x: number, y: number, size: number, ink = QR_INK) {
  const quiet = 4;
  const cells = matrix.length + quiet * 2;
  const cell = size / cells;
  ctx.fillStyle = '#fff';
  ctx.fillRect(x, y, size, size);
  ctx.fillStyle = ink;
  matrix.forEach((row, r) => row.forEach((dark, c) => {
    // 이웃 칸과 틈이 생기지 않게 반 픽셀 겹쳐 칠한다
    if (dark) ctx.fillRect(x + (c + quiet) * cell, y + (r + quiet) * cell, Math.ceil(cell) + 0.5, Math.ceil(cell) + 0.5);
  }));
}

/** 내려받을 PNG (기본 1024px — 인쇄해도 깨지지 않게) */
export function qrPng(text: string, px = 1024): Promise<Blob> {
  const canvas = document.createElement('canvas');
  canvas.width = canvas.height = px;
  const ctx = canvas.getContext('2d')!;
  drawQr(ctx, qrMatrix(text), 0, 0, px);
  return new Promise((resolve, reject) => canvas.toBlob(b => (b ? resolve(b) : reject(new Error('QR 이미지를 만들지 못했어요.'))), 'image/png'));
}

/** 파일 저장 — 휴대폰에서 공유 시트로 보낼 수 있으면 그쪽(사진 앱에 바로 저장), 아니면 내려받기 */
export async function saveImage(blob: Blob, filename: string): Promise<'shared' | 'downloaded'> {
  const file = new File([blob], filename, { type: blob.type });
  const nav = navigator as Navigator & { canShare?: (d: ShareData) => boolean };
  if (matchMedia('(pointer:coarse)').matches && nav.canShare?.({ files: [file] })) {
    try { await nav.share({ files: [file] }); return 'shared'; } catch (e) { if ((e as Error).name === 'AbortError') return 'shared'; }
  }
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url; a.download = filename;
  // 바로 해제하면 Safari에서 내려받기가 끊긴다 — 붙였다 떼고 잠시 뒤 해제
  document.body.appendChild(a); a.click(); a.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  return 'downloaded';
}
