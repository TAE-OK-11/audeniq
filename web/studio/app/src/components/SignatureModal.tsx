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

type CertStep = 'select' | 'phone' | 'rrn' | 'verify' | 'done';

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

  // 민간인증서 플로우 상태
  const [certStep, setCertStep] = useState<CertStep>('select');
  const [certProvider, setCertProvider] = useState('');
  const [phone, setPhone] = useState('');
  const [rrnFront, setRrnFront] = useState('');
  const [rrnBack, setRrnBack] = useState('');
  const [verifyCode, setVerifyCode] = useState('');
  const [certName, setCertName] = useState(getProfileSnapshot().name || '');

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

  // 직접 서명 탭으로 돌아오면 캔버스 다시 초기화
  useEffect(() => {
    if (tab === 'draw') {
      const t = window.setTimeout(initCanvas, 0);
      return () => window.clearTimeout(t);
    }
  }, [tab]); // eslint-disable-line react-hooks/exhaustive-deps

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

  // --- 민간인증서 플로우 ---
  const formatPhone = (v: string) => {
    const d = v.replace(/\D/g, '').slice(0, 11);
    if (d.length <= 3) return d;
    if (d.length <= 7) return `${d.slice(0, 3)}-${d.slice(3)}`;
    return `${d.slice(0, 3)}-${d.slice(3, 7)}-${d.slice(7)}`;
  };

  const selectProvider = (p: string) => {
    setCertProvider(p);
    setCertStep('phone');
  };

  const submitPhone = () => {
    const digits = phone.replace(/\D/g, '');
    if (digits.length < 10 || digits.length > 11) {
      toast('휴대폰 번호를 정확히 입력해 주세요.');
      return;
    }
    if (!certName.trim()) {
      toast('이름을 입력해 주세요.');
      return;
    }
    setCertStep('rrn');
  };

  const submitRrn = () => {
    if (rrnFront.length !== 6 || !/^\d{6}$/.test(rrnFront)) {
      toast('주민등록번호 앞 6자리를 입력해 주세요.');
      return;
    }
    if (rrnBack.length !== 1 || !/^[1-4]$/.test(rrnBack)) {
      toast('주민등록번호 뒷자리 첫 번째 숫자를 입력해 주세요.');
      return;
    }
    setCertStep('verify');
    toast('인증번호 6자리를 발송했어요.');
  };

  const submitVerify = () => {
    if (verifyCode.length !== 6 || !/^\d{6}$/.test(verifyCode)) {
      toast('인증번호 6자리를 입력해 주세요.');
      return;
    }
    const at = stampNow();
    updateDoc(doc.id, {
      signerName: certName.trim(),
      localSignatureAt: at,
      reviewHistory: [
        ...doc.reviewHistory,
        { status: `${certProvider} 본인 인증 완료`, time: at, detail: `휴대폰 본인 확인 · ${phone}` },
      ],
    });
    setCertStep('done');
  };

  const resetCert = () => {
    setCertStep('select');
    setCertProvider('');
    setPhone('');
    setRrnFront('');
    setRrnBack('');
    setVerifyCode('');
  };

  const title = stripSampleSuffix(doc.title);

  const renderCertBody = () => {
    switch (certStep) {
      case 'select':
        return (
          <>
            <p className="aq-sign-intro">본인 명의의 인증서를 선택해 계약서 서명을 진행할 수 있어요.</p>
            <div className="aq-sign-provider-list">
              {CERT_PROVIDERS.map(x => (
                <button
                  key={x} type="button" className="aq-sign-provider" data-cert={x}
                  onClick={() => selectProvider(x)}
                >
                  <span>{x}</span><small>본인 인증 진행</small>
                </button>
              ))}
            </div>
            <div className="notice">
              선택한 인증서로 휴대폰 본인 확인을 진행해요. 실제 인증 연동 전까지는 데모 흐름으로 동작해요.
            </div>
          </>
        );
      case 'phone':
        return (
          <>
            <p className="aq-sign-intro">
              <strong>{certProvider}</strong> 본인 확인을 위해 이름과 휴대폰 번호를 입력해 주세요.
            </p>
            <div className="field">
              <label htmlFor="aqCertName">이름</label>
              <input
                type="text" id="aqCertName" maxLength={20}
                value={certName} onChange={e => setCertName(e.target.value)}
                placeholder="본인 이름을 입력해 주세요."
                autoComplete="name"
              />
            </div>
            <div className="field">
              <label htmlFor="aqCertPhone">휴대폰 번호</label>
              <input
                type="tel" id="aqCertPhone"
                value={phone} onChange={e => setPhone(formatPhone(e.target.value))}
                placeholder="010-0000-0000"
                autoComplete="tel" inputMode="tel"
              />
            </div>
            <div className="notice">입력한 번호로 인증번호 6자리가 발송돼요.</div>
          </>
        );
      case 'rrn':
        return (
          <>
            <p className="aq-sign-intro">
              본인 확인을 위해 주민등록번호를 입력해 주세요.
            </p>
            <div className="field">
              <label htmlFor="aqCertRrnFront">주민등록번호</label>
              <div className="aq-rrn-row">
                <input
                  type="text" id="aqCertRrnFront"
                  value={rrnFront}
                  onChange={e => setRrnFront(e.target.value.replace(/\D/g, '').slice(0, 6))}
                  placeholder="앞 6자리" inputMode="numeric" maxLength={6}
                />
                <span className="aq-rrn-dash">-</span>
                <input
                  type="password" id="aqCertRrnBack" aria-label="주민등록번호 뒷자리 첫 자리"
                  value={rrnBack}
                  onChange={e => setRrnBack(e.target.value.replace(/\D/g, '').slice(0, 1))}
                  placeholder="●" inputMode="numeric" maxLength={1}
                />
                <span className="aq-rrn-mask">●●●●●●</span>
              </div>
            </div>
            <div className="notice">뒷자리는 첫 번째 숫자만 입력해요. 나머지는 저장되지 않아요.</div>
          </>
        );
      case 'verify':
        return (
          <>
            <p className="aq-sign-intro">
              <strong>{phone}</strong>으로 발송된 인증번호 6자리를 입력해 주세요.
            </p>
            <div className="field">
              <label htmlFor="aqCertCode">인증번호</label>
              <input
                type="text" id="aqCertCode"
                value={verifyCode}
                onChange={e => setVerifyCode(e.target.value.replace(/\D/g, '').slice(0, 6))}
                placeholder="6자리 숫자" inputMode="numeric" maxLength={6}
                autoComplete="one-time-code"
              />
            </div>
            <button type="button" className="link-btn" onClick={() => toast('인증번호를 다시 발송했어요.')}>
              인증번호 다시 받기
            </button>
          </>
        );
      case 'done':
        return (
          <div className="aq-cert-done">
            <div className="aq-cert-done-icon">✓</div>
            <strong>본인 인증이 완료됐어요</strong>
            <p>{certProvider} · {certName} · {phone}</p>
            <p className="help">계약서 서명 기록에 본인 인증 내역이 저장됐어요.</p>
          </div>
        );
    }
  };

  const renderCertFoot = () => {
    switch (certStep) {
      case 'select':
        return (
          <button type="button" className="button secondary" onClick={onBack}>
            문서로 돌아가기
          </button>
        );
      case 'phone':
        return (
          <>
            <button type="button" className="button" onClick={submitPhone}>
              다음
            </button>
            <button type="button" className="button secondary" onClick={() => setCertStep('select')}>
              이전
            </button>
          </>
        );
      case 'rrn':
        return (
          <>
            <button type="button" className="button" onClick={submitRrn}>
              인증번호 받기
            </button>
            <button type="button" className="button secondary" onClick={() => setCertStep('phone')}>
              이전
            </button>
          </>
        );
      case 'verify':
        return (
          <>
            <button type="button" className="button" onClick={submitVerify}>
              인증 완료
            </button>
            <button type="button" className="button secondary" onClick={() => setCertStep('rrn')}>
              이전
            </button>
          </>
        );
      case 'done':
        return (
          <>
            <button type="button" className="button" onClick={() => { onSaved(); }}>
              확인
            </button>
            <button type="button" className="button secondary" onClick={resetCert}>
              다시 인증하기
            </button>
          </>
        );
    }
  };

  return (
    <Modal title="계약서 서명" onClose={onBack} modalClass="aq-signature-mode">
      <div className="aq-sign-body">
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
            onClick={() => { setTab('cert'); setCertStep('select'); }}
          >
            민간인증서
          </button>
        </div>

        {tab === 'draw' ? (
          <section id="aqDrawPane" role="tabpanel">
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
          </section>
        ) : (
          <section id="aqCertPane" role="tabpanel">
            {renderCertBody()}
          </section>
        )}
      </div>
      <div className="aq-sign-foot">
        {tab === 'draw' ? (
          <>
            <button type="button" className="button" id="aqSignSave" disabled={readOnly} onClick={save}>
              서명 입력 저장
            </button>
            <button type="button" className="button secondary" id="aqSignReturn" onClick={onBack}>
              문서로 돌아가기
            </button>
          </>
        ) : (
          renderCertFoot()
        )}
      </div>
    </Modal>
  );
}
