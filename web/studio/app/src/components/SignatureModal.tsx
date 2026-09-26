// 계약서 서명 모달 — 라이브 openSignatureFlow 대응
import { useEffect, useRef, useState } from 'react';
import { Modal } from './Modal';
import { useToast } from './Toast';
import { updateDoc, type DocRecord } from '../store/docs';
import { getProfileSnapshot } from '../store/profile';
import { localStamp, stripSampleSuffix } from '../lib/format';
import { stampNow } from '../lib/date';
import { uid } from '../lib/store';
import { pushNotice } from '../store/support';
import { MOCK } from '../api/client';
import * as portal from '../api/portal';
import { errorMessage } from '../api/errors';
import { refreshDocs } from '../store/portalSync';
import { compactSignature } from '../lib/application';
import { parseStamp } from '../lib/date';
import { MOCK_REVIEW_SECONDS } from '../store/mockReviewer';

const CERT_PROVIDERS = ['PASS', '카카오 인증서', '네이버 인증서', '토스 인증서'];

type CertStep = 'select' | 'phone' | 'rrn' | 'verify' | 'done';

export function SignatureModal({
  doc,
  onBack,
  onSaved,
  onDone,
}: {
  doc: DocRecord;
  /** 서명 창을 닫고 문서 상세로 돌아감 */
  onBack: () => void;
  /** 서명·인증이 저장될 때마다 호출 (창은 닫지 않음) */
  onSaved?: () => void;
  /** 계약서 완성 후 확인 — 없으면 onBack */
  onDone?: () => void;
}) {
  const toast = useToast();
  const readOnly = doc.reviewStatus !== 'approved';
  // 순차 단계: 1) 서명 진행 → 2) 전자서명 진행 → 3) 계약서 완성
  const [phase, setPhase] = useState<'sign' | 'cert' | 'complete'>('sign');
  const [name, setName] = useState(doc.signerName || getProfileSnapshot().name || '');
  const [ack, setAck] = useState(false);
  const [signing, setSigning] = useState(false);
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

  // 서명 단계에 들어올 때마다 빈 캔버스로 초기화하고(이전 획 수가 남아 빈 서명이 저장되던 문제 방지),
  // 화면 회전·창 크기 변경 시에는 그린 서명을 보존한 채 해상도만 다시 맞춘다.
  useEffect(() => {
    if (phase !== 'sign' || readOnly) return;
    setStrokes(0);
    const t = window.setTimeout(initCanvas, 0);
    const canvas = canvasRef.current;
    let lastWidth = 0;
    const ro = canvas && 'ResizeObserver' in window ? new ResizeObserver(entries => {
      const w = Math.round(entries[0].contentRect.width);
      if (!lastWidth) { lastWidth = w; return; }
      if (w === lastWidth || !canvas.width) return;
      lastWidth = w;
      const snapshot = document.createElement('canvas');
      snapshot.width = canvas.width; snapshot.height = canvas.height;
      snapshot.getContext('2d')?.drawImage(canvas, 0, 0);
      initCanvas();
      const ctx = canvas.getContext('2d');
      if (ctx) {
        ctx.save();
        ctx.setTransform(1, 0, 0, 1, 0, 0);
        ctx.drawImage(snapshot, 0, 0, canvas.width, canvas.height);
        ctx.restore();
      }
    }) : null;
    if (canvas && ro) ro.observe(canvas);
    return () => { window.clearTimeout(t); ro?.disconnect(); };
  }, [phase, readOnly]); // eslint-disable-line react-hooks/exhaustive-deps

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

  const save = async () => {
    if (!name.trim()) { toast('서명자 이름을 입력해 주세요.'); return; }
    if (!strokes) { toast('서명을 직접 그려 주세요.'); return; }
    if (!ack) { toast('문서 내용을 확인해 주세요.'); return; }
    const data = canvasRef.current!.toDataURL('image/png');
    if (!MOCK) {
      // 실서버: 확인 기록 후 서명을 서버에 저장하면 계약이 체결된다.
      // (휴대폰 본인 인증은 인증 기관 연동 전이라 실서버에서는 진행하지 않는다)
      if (signing) return;
      setSigning(true);
      try {
        let rv = doc.rowVersion ?? 0;
        if (!doc.checkedAt) rv = await portal.checkDocument(doc.id);
        const compact = await compactSignature(data);
        await portal.signDocument(doc.id, name.trim(), compact || data, rv);
        await refreshDocs();
        onSaved?.();
        setPhase('complete');
      } catch (err) {
        toast(errorMessage(err, '서명을 저장하지 못했어요. 잠시 후 다시 시도해 주세요.'));
      } finally {
        setSigning(false);
      }
      return;
    }
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
    onSaved?.();
    // 2단계: 전자서명 진행으로
    setCertStep('select');
    setPhase('cert');
    toast('서명이 저장됐어요. 전자서명을 진행해 주세요.');
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
        { status: '계약서 완성', time: at, detail: '직접 서명 + 전자서명(본인 인증) 완료' },
      ],
    });
    onSaved?.();
    pushNotice({
      id: uid('n'), kind: '서류', time: at, link: '/contracts',
      title: `${stripSampleSuffix(doc.releaseTitle || doc.title)} 계약서 서명이 완료됐어요.`,
      detail: '직접 서명과 본인 인증이 완료됐어요. 계약서 사본은 계약서 메뉴에서 언제든 확인할 수 있어요.',
    });
    // 3단계: 계약서 완성
    setPhase('complete');
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
          <button type="button" className="button secondary" onClick={() => setPhase('sign')}>
            이전: 서명 단계로
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
            <button type="button" className="button" onClick={() => (onDone ?? onBack)()}>
              확인
            </button>
            <button type="button" className="button secondary" onClick={resetCert}>
              다시 인증하기
            </button>
          </>
        );
    }
  };

  const phaseLabel = phase === 'sign' ? '1단계 · 서명 진행' : phase === 'cert' ? '2단계 · 전자서명 진행' : '3단계 · 계약서 완성';
  const phaseIndex = phase === 'sign' ? 0 : phase === 'cert' ? 1 : 2;

  return (
    <Modal title="계약서 서명" onClose={onBack} modalClass="aq-signature-mode">
      <div className="aq-sign-body">
        <div className="aq-phase-steps" aria-label="서명 절차 단계">
          {['서명 진행', '전자서명 진행', '계약서 완성'].map((label, i) => (
            <span key={label} className={`aq-phase-step${i === phaseIndex ? ' active' : ''}${i < phaseIndex ? ' done' : ''}`}>
              <em>{i + 1}</em>{label}
            </span>
          ))}
        </div>
        <p className="aq-sign-intro">{phaseLabel}</p>
        <div className="aq-sign-doc">
          <small>{readOnly ? '서명 절차를 확인할 문서' : '서명할 문서'} · v{doc.version || '1.0'}</small>
          <strong>{title}</strong>
          <div className="help">{readOnly ? '검토 진행 중' : '검토 완료'} · {doc.releaseTitle || '공통 문서'}</div>
        </div>

        {phase === 'sign' && readOnly ? (
          <section id="aqWaitPane" className="aq-sign-wait">
            <span className="aq-sign-wait-icon" aria-hidden="true">
              <svg viewBox="0 0 24 24" width="26" height="26" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></svg>
            </span>
            <strong>계약서를 검토하고 있어요.</strong>
            <p>AUDENIQ 담당자가 신청 내용을 확인한 뒤 서명할 수 있어요. 검토가 끝나면 알림으로 알려드릴게요.</p>
            <ol className="aq-sign-wait-steps">
              <li className="is-done"><span>✓</span>신청서 접수</li>
              <li className="is-current"><span>2</span>담당자 검토</li>
              <li><span>3</span>서명·본인 인증</li>
            </ol>
            {MOCK && (
              <p className="aq-sign-wait-demo">
                체험 모드에서는 접수 후 약 {Math.round(MOCK_REVIEW_SECONDS / 60)}분 뒤 검토가 자동으로 완료돼요.
                {(() => {
                  const last = parseStamp(doc.reviewHistory.at(-1)?.time);
                  return last ? ` (접수 ${last.getHours()}:${String(last.getMinutes()).padStart(2, '0')})` : '';
                })()}
              </p>
            )}
          </section>
        ) : phase === 'sign' ? (
          <section id="aqDrawPane">
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
                {strokes > 0 && (
                  <button type="button" className="aq-sign-clear" id="aqSignClear" onClick={clear}>다시 그리기</button>
                )}
              </div>
            </div>
            <label className="aq-sign-check">
              <input type="checkbox" id="aqSignAck" checked={ack} onChange={e => setAck(e.target.checked)} />
              <span>
                서명할 문서의 내용을 확인했어요.
                <small>서명을 저장하면 다음 단계에서 본인 인증(전자서명)을 진행해요.</small>
              </span>
            </label>
          </section>
        ) : phase === 'cert' ? (
          <section id="aqCertPane">
            {renderCertBody()}
          </section>
        ) : (
          <section id="aqCompletePane">
            <div className="aq-complete-hero">
              <span className="aq-complete-check" aria-hidden="true">✓</span>
              <h3>계약서 서명이 완성됐어요</h3>
              <p>직접 서명과 전자서명(본인 인증)이 모두 완료됐어요.</p>
            </div>
            <div className="aq-complete-summary">
              <div><small>서명자</small><strong>{name || certName || doc.signerName || '-'}</strong></div>
              <div><small>인증 수단</small><strong>{certProvider || '-'}</strong></div>
              <div><small>완성 시각</small><strong>{localStamp(doc.localSignatureAt || stampNow())}</strong></div>
            </div>
          </section>
        )}
      </div>
      <div className="aq-sign-foot">
        {phase === 'sign' && readOnly ? (
          <button type="button" className="button secondary aq-sign-full" id="aqSignReturn" onClick={onBack}>
            문서로 돌아가기
          </button>
        ) : phase === 'sign' ? (
          <>
            <button type="button" className={`button${signing ? ' is-busy' : ''}`} id="aqSignSave" onClick={save} disabled={signing}>
              {MOCK ? '서명 저장 후 본인 인증' : signing ? '서명하는 중' : '서명하고 계약 체결'}
            </button>
            <button type="button" className="button secondary" id="aqSignReturn" onClick={onBack}>
              문서로 돌아가기
            </button>
          </>
        ) : phase === 'cert' ? (
          renderCertFoot()
        ) : (
          <>
            <button type="button" className="button" onClick={() => (onDone ?? onBack)()}>
              계약서 완성 확인
            </button>
            <button type="button" className="button secondary" onClick={() => { setPhase('sign'); }}>
              서명 다시 하기
            </button>
          </>
        )}
      </div>
    </Modal>
  );
}
