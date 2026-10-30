// 프로필 사진 자르기 — 고른 사진을 정사각형으로: 끌어서 위치, 막대로 확대. 동그란 안내선은 플랫폼에서 프로필이 동그랗게 잘리는 자리.
// 잘린 부분의 짧은 변이 1000px보다 작으면 플랫폼이 흐리게 보여 쓰지 않는다 (확대할수록 작아진다). 결과는 최대 3000px JPEG.
import { useEffect, useRef, useState, type PointerEvent as RPointerEvent } from 'react';
import { useModalClose } from './Modal';
import '../styles/split/ArtistStudio.css';

export const PHOTO_MIN = 1000;
const PHOTO_MAX = 3000;
const ZOOM_MAX = 3;

export interface CroppedPhoto { file: File; preview: string; size: number }

/** 고른 파일을 그림으로 읽는다 (JPEG·PNG만) */
export function readImage(file: File): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(file);
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => { URL.revokeObjectURL(url); reject(new Error('IMAGE_UNREADABLE')); };
    img.src = url;
  });
}

export function PhotoCropper({ image, onDone }: { image: HTMLImageElement; onDone: (photo: CroppedPhoto) => Promise<void> }) {
  const close = useModalClose();
  const stage = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState(300);
  const [zoom, setZoom] = useState(1);
  // 가운데에서 벗어난 거리 (화면 px)
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const [busy, setBusy] = useState(false);
  const drag = useRef<{ x: number; y: number; px: number; py: number } | null>(null);

  useEffect(() => {
    const el = stage.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setBox(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const w = image.naturalWidth;
  const h = image.naturalHeight;
  const short = Math.min(w, h);
  // 짧은 변이 칸을 꽉 채우는 크기에 확대를 곱한다
  const scale = (box / short) * zoom;
  const shownW = w * scale;
  const shownH = h * scale;
  const clamp = (p: { x: number; y: number }) => ({
    x: Math.max(-(shownW - box) / 2, Math.min((shownW - box) / 2, p.x)),
    y: Math.max(-(shownH - box) / 2, Math.min((shownH - box) / 2, p.y)),
  });
  const at = clamp(pos);
  // 잘릴 부분의 원본 크기
  const side = Math.round(box / scale);
  const tooSmall = side < PHOTO_MIN;

  const down = (e: RPointerEvent) => {
    (e.target as HTMLElement).setPointerCapture?.(e.pointerId);
    drag.current = { x: e.clientX, y: e.clientY, px: at.x, py: at.y };
  };
  const move = (e: RPointerEvent) => {
    const d = drag.current;
    if (!d) return;
    setPos(clamp({ x: d.px + e.clientX - d.x, y: d.py + e.clientY - d.y }));
  };
  const up = () => { drag.current = null; };
  const nudge = (dx: number, dy: number) => setPos(p => clamp({ x: clamp(p).x + dx, y: clamp(p).y + dy }));

  const use = async () => {
    if (tooSmall || busy) return;
    setBusy(true);
    try {
      const out = Math.min(PHOTO_MAX, side);
      const canvas = document.createElement('canvas');
      canvas.width = out;
      canvas.height = out;
      const ctx = canvas.getContext('2d');
      if (!ctx) throw new Error('CANVAS');
      ctx.imageSmoothingQuality = 'high';
      // 칸 왼쪽 위가 원본의 어디인지
      const sx = (shownW / 2 - box / 2 - at.x) / scale;
      const sy = (shownH / 2 - box / 2 - at.y) / scale;
      ctx.fillStyle = '#fff';
      ctx.fillRect(0, 0, out, out);
      ctx.drawImage(image, sx, sy, side, side, 0, 0, out, out);
      const blob = await new Promise<Blob | null>(r => canvas.toBlob(r, 'image/jpeg', 0.92));
      if (!blob) throw new Error('CANVAS');
      const file = new File([blob], 'artist-photo.jpg', { type: 'image/jpeg' });
      await onDone({ file, preview: canvas.toDataURL('image/jpeg', 0.85), size: out });
      close();
    } catch { /* onDone이 알린다 */ } finally { setBusy(false); }
  };

  return (
    <div className="aq-crop">
      <div
        ref={stage} className="aq-crop-stage" role="application" tabIndex={0}
        aria-label="사진 위치 — 끌거나 방향키로 옮겨요"
        onPointerDown={down} onPointerMove={move} onPointerUp={up} onPointerCancel={up}
        onKeyDown={e => {
          const k = { ArrowLeft: [8, 0], ArrowRight: [-8, 0], ArrowUp: [0, 8], ArrowDown: [0, -8] }[e.key];
          if (k) { e.preventDefault(); nudge(k[0], k[1]); }
        }}
      >
        <img src={image.src} alt="" draggable={false}
          style={{ width: shownW, height: shownH, transform: `translate(calc(-50% + ${at.x}px), calc(-50% + ${at.y}px))` }} />
        <i className="aq-crop-ring" aria-hidden="true" />
      </div>
      <div className="aq-crop-zoom">
        <label htmlFor="cropZoom">확대</label>
        <input id="cropZoom" type="range" min={1} max={ZOOM_MAX} step={0.01} value={zoom}
          onChange={e => setZoom(Number(e.target.value))} aria-valuetext={`${Math.round(zoom * 100)}%`} />
      </div>
      <p className={`ui-field-help${tooSmall ? ' aq-help-error' : ''}`} role={tooSmall ? 'alert' : undefined}>
        {tooSmall
          ? `잘린 사진이 ${side}px이라 플랫폼에서 흐리게 보여요. 덜 확대하거나 더 큰 사진을 골라 주세요 (1000px 이상).`
          : `동그란 선 안이 플랫폼 프로필에 보여요 · ${Math.min(PHOTO_MAX, side)}×${Math.min(PHOTO_MAX, side)}px로 저장해요`}
      </p>
      <button type="button" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={tooSmall || busy} onClick={() => void use()}>
        {busy ? '올리는 중' : '이 사진 쓰기'}
      </button>
    </div>
  );
}
