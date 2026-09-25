// 계약서 서명 모달 — 라이브 openSignatureFlow 대응
import { useEffect, useRef, useState } from 'react';
import { Modal } from './Modal';
import { useToast } from './Toast';
import { updateDoc, type DocRecord } from '../store/docs';
import { getProfileSnapshot } from '../store/profile';
import { stripSampleSuffix } from '../lib/format';

function stampNow(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

const CERT_PROVIDERS = ['PASS', '카카오 인증서', '네이버 인증서', '토스 인증서'];

export function SignatureModal({
  doc,
  onBack,
  onSaved,
}: {
  doc: DocRecord;
  onBack: () => void;
  onSaved: () => void;
}) {
  const toast = useToast();
  const readOnly = doc.reviewStatus !== 'approved';
  const [tab, setTab] = useState<'draw' | 'cert'>('draw');
  const [name, setName] = useState(doc.signerName || getProfileSnapshot().name || '');
  const [ack, setAck] = useState(false);
  const [strokes, setStrokes] = useState(0);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const drawing = useRef(false);
  const last = useRef<{ x: number; y: number } | null>(null);

  const initCanvas = () => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const rect = canvas.getBoundingClientRect();
    const ratio = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.max(1, Math.round(rect.width * ratio));
    canvas.height = Math.max(1, Math.round(rect.height * ratio));
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.strokeStyle = '#212D49';
    ctx.lineWidth = 2.7;
    ctx.clearRect(0, 0, rect.width, rect.height);
  };

  // 라이브는 모달 열 때 한 번만 initCanvas (탭 전환 시 다시 그리지 않음)
  useEffect(() => {
    const t = window.setTimeout(initCanvas, 0);
    return () => window.clearTimeout(t);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const xy = (e: React.PointerEvent) => {
    const r = canvasRef.current!.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const startDraw = (e: React.PointerEvent) => {
    if (e.button !== 0 && e.pointerType === 'mouse') return;
    e.preventDefault();
    const canvas = canvasRef.current!;
    canvas.setPointerCapture(e.pointerId);
    drawing.current = true;
    setStrokes(s => s + 1);
    const p = xy(e);
    last.current = p;
    const ctx = canvas.getContext('2d')!;
    ctx.beginPath();
    ctx.moveTo(p.x, p.y);
    ctx.lineTo(p.x + 0.12, p.y + 0.12);
    ctx.stroke();
  };

  const moveDraw = (e: React.PointerEvent) => {
    if (!drawing.current) return;
    e.preventDefault();
    const next = xy(e);
    const ctx = canvasRef.current!.getContext('2d')!;
    ctx.beginPath();
    ctx.moveTo(last.current!.x, last.current!.y);
    ctx.lineTo(next.x, next.y);
    ctx.stroke();
    last.current = next;
  };

  const endDraw = (e: React.PointerEvent) => {
    if (drawing.current) {
      drawing.current = false;
      last.current = null;
      try { canvasRef.current!.releasePointerCapture(e.pointerId); } catch { /* noop */ }
    }
  };

  const clear = () => {
    const canvas = canvasRef.current!;
    const ctx = canvas.getContext('2d')!;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    setStrokes(0);
  };

  const save = () => {
    if (!name.trim()) { toast('서명자 이름을 입력해 주세요.'); return; }
    if (!strokes) { toast('서명을 직접 그려 주세요.'); return; }
    if (!ack) { toast('문서 내용을 확인해 주세요.'); return; }
    const data = canvasRef.current!.toDataURL('image/png');
    const at = stampNow();
    updateDoc(doc.id, {
      signerName: name.trim(),
      localSignatureData: data,
      localSignatureAt: at,
      reviewHistory: [
        ...doc.reviewHistory,
        { status: '직접 서명 입력 보관', time: at, detail: '서명 이미지 보관 · 본인 인증 및 법적 전자서명 대기' },
      ],
    });
    onSaved();
    toast('서명 입력을 보관했어요.');
  };

  const title = stripSampleSuffix(doc.title);

  return (
    <Modal title="계약서 서명" onClose={onBack} modalClass="aq-signature-mode">
      <p className="aq-sign-intro">계약서 내용을 확인하고 서명 방법을 선택해 주세요.</p>
      <div className="aq-sign-doc">
        <small>{readOnly ? '서명 절차를 확인할 문서' : '서명할 문서'} · v{doc.version || '1.0'}</small>
        <strong>{title}</strong>
        <div className="help">검토 완료 · {doc.releaseTitle || '공통 문서'}</div>
      </div>
      <div className="aq-sign-tabs" role="tablist" aria-label="서명 방법">
        <button
          type="button" id="aqTabDraw" role="tab"
          aria-selected={tab === 'draw'} aria-controls="aqDrawPane"
          onClick={() => setTab('draw')}
        >
          직접 서명
        </button>
        <button
          type="button" id="aqTabCert" role="tab"
          aria-selected={tab === 'cert'} aria-controls="aqCertPane"
          onClick={() => setTab('cert')}
        >
          민간인증서
        </button>
      </div>

      <section id="aqDrawPane" role="tabpanel" hidden={tab !== 'draw'}>
        <div className="field">
          <label htmlFor="aqSignerName">서명자 이름</label>
          <input
            type="text" maxLength={100} id="aqSignerName"
            value={name} onChange={e => setName(e.target.value)}
            autoComplete="name"
            placeholder="실명 또는 계약서의 서명자명을 입력해 주세요."
          />
        </div>
        <div className="field">
          <label htmlFor="aqDrawCanvas">서명 입력</label>
          <div className="aq-sign-sheet">
            <canvas
              id="aqDrawCanvas" ref={canvasRef}
              aria-label="손가락이나 마우스로 서명을 그리는 공간"
              onPointerDown={startDraw} onPointerMove={moveDraw}
              onPointerUp={endDraw} onPointerCancel={endDraw}
              style={{ touchAction: 'none', width: '100%', height: 180 }}
            />
            <span id="aqSignPlaceholder" className="aq-sign-placeholder" hidden={strokes > 0}>
              여기에 손가락으로 서명해 주세요.
            </span>
          </div>
        </div>
        <div className="aq-sign-tools">
          <button type="button" className="link-btn" id="aqSignClear" onClick={clear}>다시 그리기</button>
        </div>
        <label className="aq-sign-check">
          <input type="checkbox" id="aqSignAck" checked={ack} onChange={e => setAck(e.target.checked)} />
          <span>
            서명할 문서의 내용을 확인했어요.
            <small>이 단계에서는 서명 이미지를 입력·보관하며, 법적 전자서명과 본인 인증은 별도 연동이 필요해요.</small>
          </span>
        </label>
        {readOnly && <div className="notice">문서 검토가 완료되면 서명을 저장할 수 있어요.</div>}
        <div className="aq-sign-foot">
          <button type="button" className="button" id="aqSignSave" disabled={readOnly} onClick={save}>
            서명 입력 저장
          </button>
          <button type="button" className="button secondary" id="aqSignReturn" onClick={onBack}>
            문서로 돌아가기
          </button>
        </div>
      </section>

      <section id="aqCertPane" role="tabpanel" hidden={tab !== 'cert'}>
        <p className="aq-sign-intro">본인 명의의 인증서를 선택해 계약서 서명을 진행할 수 있어요.</p>
        <div className="aq-sign-provider-list">
          {CERT_PROVIDERS.map(x => (
            <button
              key={x} type="button" className="aq-sign-provider" data-cert={x}
              onClick={() => toast(x + ' 인증 연결을 준비하고 있어요.')}
            >
              <span>{x}</span><small>인증 연동 준비 중</small>
            </button>
          ))}
        </div>
        <div className="notice">
          인증 사업자 연결 및 계약 원문에 대한 서명 검증이 준비되면 이 화면에서 진행할 수 있어요. 아직 인증 요청이 전송되지는 않아요.
        </div>
        <div className="aq-sign-foot">
          <button type="button" className="button secondary" id="aqCertReturn" onClick={onBack}>
            문서로 돌아가기
          </button>
        </div>
      </section>
    </Modal>
  );
}
