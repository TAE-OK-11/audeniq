// 공용 서명 입력 칸 — 손가락·마우스·펜으로 서명을 그린다.
// - 초기화는 setTransform으로 매번 같은 상태를 만든다(여러 번 호출돼도 좌표가 어긋나지 않음)
// - 화면 회전·창 크기 변경 시 그린 서명을 보존한 채 해상도만 다시 맞춘다
import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from 'react';

export interface SignaturePadHandle {
  clear: () => void;
  toDataURL: () => string;
  isEmpty: () => boolean;
}

interface Props {
  id?: string;
  label?: string;
  placeholder?: string;
  height?: number;
  onChange?: (hasSignature: boolean) => void;
}

export const SignaturePad = forwardRef<SignaturePadHandle, Props>(function SignaturePad(
  { id, label = '서명 입력 공간', placeholder = '여기에 손가락으로 서명해 주세요.', height = 180, onChange },
  ref,
) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const drawing = useRef(false);
  const last = useRef<{ x: number; y: number } | null>(null);
  const [strokes, setStrokes] = useState(0);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  const setup = useCallback((keep: boolean) => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext('2d');
    if (!canvas || !ctx) return;
    const rect = canvas.getBoundingClientRect();
    if (!rect.width) return;
    const ratio = Math.min(window.devicePixelRatio || 1, 2);
    let snapshot: HTMLCanvasElement | null = null;
    if (keep && canvas.width > 1) {
      snapshot = document.createElement('canvas');
      snapshot.width = canvas.width; snapshot.height = canvas.height;
      snapshot.getContext('2d')?.drawImage(canvas, 0, 0);
    }
    canvas.width = Math.max(1, Math.round(rect.width * ratio));
    canvas.height = Math.max(1, Math.round(rect.height * ratio));
    if (snapshot) ctx.drawImage(snapshot, 0, 0, canvas.width, canvas.height);
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.strokeStyle = '#1c2740';
    ctx.lineWidth = 2.6;
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    // 모달 등장 애니메이션이 끝난 뒤 크기를 잰다
    const t = window.setTimeout(() => setup(false), 30);
    let lastW = 0;
    const ro = 'ResizeObserver' in window ? new ResizeObserver(([e]) => {
      const w = Math.round(e.contentRect.width);
      if (w && w !== lastW) { const first = !lastW; lastW = w; setup(!first); }
    }) : null;
    ro?.observe(canvas);
    return () => { window.clearTimeout(t); ro?.disconnect(); };
  }, [setup]);

  useEffect(() => { onChangeRef.current?.(strokes > 0); }, [strokes]);

  const point = (e: React.PointerEvent) => {
    const r = canvasRef.current!.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const down = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (e.pointerType === 'mouse' && e.button !== 0) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    drawing.current = true;
    const p = point(e);
    last.current = p;
    const ctx = e.currentTarget.getContext('2d')!;
    ctx.beginPath();
    ctx.moveTo(p.x, p.y);
    ctx.lineTo(p.x + 0.1, p.y + 0.1);
    ctx.stroke();
    setStrokes(s => s + 1);
  };
  const move = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (!drawing.current || !last.current) return;
    e.preventDefault();
    const p = point(e);
    const ctx = e.currentTarget.getContext('2d')!;
    ctx.beginPath();
    ctx.moveTo(last.current.x, last.current.y);
    ctx.lineTo(p.x, p.y);
    ctx.stroke();
    last.current = p;
  };
  const up = (e: React.PointerEvent<HTMLCanvasElement>) => {
    drawing.current = false;
    last.current = null;
    try { e.currentTarget.releasePointerCapture(e.pointerId); } catch { /* noop */ }
  };

  const clear = useCallback(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext('2d');
    if (!canvas || !ctx) return;
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.restore();
    setStrokes(0);
  }, []);

  useImperativeHandle(ref, () => ({
    clear,
    toDataURL: () => canvasRef.current?.toDataURL('image/png') ?? '',
    isEmpty: () => strokes === 0,
  }), [clear, strokes]);

  return (
    <div className="aq-pad">
      <canvas
        id={id} ref={canvasRef} aria-label={label}
        style={{ height, touchAction: 'none' }}
        onPointerDown={down} onPointerMove={move} onPointerUp={up} onPointerCancel={up}
      />
      {strokes === 0 && <span className="aq-pad-hint" aria-hidden="true">{placeholder}</span>}
      {strokes > 0 && <button type="button" className="aq-sign-clear" onClick={clear}>다시 그리기</button>}
    </div>
  );
});
