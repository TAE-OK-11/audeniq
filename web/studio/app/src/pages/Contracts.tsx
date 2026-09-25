import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Modal } from '../components/Modal';
import { useToast } from '../components/Toast';

interface DocRecord {
  id: string;
  kind: 'agreements' | 'rights';
  title: string;
  releaseTitle: string;
  version: string;
  created: string;
  fileName: string;
  content: string;
  checkedAt: string;
  reviewStatus: string;
  reviewNote: string;
  signerName: string;
  localSignatureAt: string;
  consentHistory: { time: string; action: string; version: string }[];
  reviewHistory: { status: string; time: string; detail: string }[];
}

const AGREEMENT_CONTENT = [
  'AUDENIQ 디지털 음원 배급 신청·계약서',
  '',
  '1. 신청인 및 발매 정보',
  '아티스트: 서린',
  '발매명: 첫 번째 싱글',
  '발매 유형: 싱글',
  '발매 희망일: 2026-10-01',
  '레이블 표기: AUDENIQ',
  '수록곡: 1. 첫 번째 싱글 / 2. 첫 번째 싱글 (Inst.)',
  '',
  '2. 권리자 및 배급 범위',
  '마스터 권리자: 서린',
  '℗ 표기: 2026 서린',
  '© 표기: 2026 서린',
  '배급 지역: 전 세계',
  '배급 플랫폼: Spotify, Apple Music, YouTube Music',
  '추가 확인 항목: 일반 발매',
  '',
  '3. 신청인의 확인',
  '신청인은 제출한 음원, 가사, 커버아트, 크레딧 및 메타데이터를 배급할 적법한 권한을 보유하고 있으며, 제3자의 권리가 포함된 경우 필요한 허락을 확보했음을 확인합니다.',
  '',
  '4. AUDENIQ 배급 계약',
  '신청인은 위 발매 정보를 기준으로 AUDENIQ에 디지털 음원 배급을 신청하고, 계약서에 안내된 배급 범위·정산·수정·테이크다운 및 권리 보증 조항에 동의합니다. 보완이 필요한 경우 AUDENIQ는 관련 자료를 요청할 수 있습니다.',
  '',
  '위 신청 정보는 자동으로 작성됐습니다. 내용을 확인한 뒤 신청인 서명만 진행해 주세요.',
].join('\n');

const INITIAL_DOCS: DocRecord[] = [
  {
    id: 'doc1', kind: 'agreements',
    title: '첫 번째 싱글 · AUDENIQ 디지털 음원 배급 신청·계약서',
    releaseTitle: '첫 번째 싱글', version: '1.0', created: '2026-09-20',
    fileName: '', content: AGREEMENT_CONTENT, checkedAt: '2026-09-20 14:32',
    reviewStatus: 'approved', reviewNote: '', signerName: '',
    localSignatureAt: '',
    consentHistory: [{ time: '2026-09-20 14:32', action: '내용 확인', version: '1.0' }],
    reviewHistory: [{ status: '검토 완료', time: '2026-09-21 10:05', detail: 'AUDENIQ 담당자 검토 완료' }],
  },
  {
    id: 'doc2', kind: 'agreements',
    title: '여름 EP · AUDENIQ 디지털 음원 배급 신청·계약서',
    releaseTitle: '여름 EP', version: '1.0', created: '2026-09-22',
    fileName: '', content: '', checkedAt: '',
    reviewStatus: 'review', reviewNote: '', signerName: '',
    localSignatureAt: '', consentHistory: [], reviewHistory: [],
  },
  {
    id: 'doc3', kind: 'rights',
    title: '피처링 이용 허락서',
    releaseTitle: '여름 EP', version: '1.0', created: '2026-09-23',
    fileName: 'feature_agreement.pdf', content: '', checkedAt: '',
    reviewStatus: 'needs', reviewNote: '서명란에 서명자 이름이 빠져 있어요. 보완 후 다시 제출해 주세요.',
    signerName: '', localSignatureAt: '', consentHistory: [], reviewHistory: [],
  },
];

function aqDocumentState(c: DocRecord): string {
  if (c.reviewStatus === 'approved') return '검토 완료';
  if (c.reviewStatus === 'needs') return '보완 요청';
  if (c.reviewStatus === 'review' || c.reviewStatus === 'prepared') return '검토 중';
  if (c.localSignatureAt) return '서명 완료';
  return c.kind === 'rights' ? '서류 제출 필요' : '서명 필요';
}

function DocCard({ c, onOpen }: { c: DocRecord; onOpen: (id: string) => void }) {
  const state = aqDocumentState(c);
  const tone = c.reviewStatus === 'needs' ? 'is-needs'
    : c.reviewStatus === 'approved' ? 'is-approved'
    : (c.reviewStatus === 'review' || c.reviewStatus === 'prepared') ? 'is-review' : '';
  const btnLabel = c.kind === 'agreements'
    ? (c.localSignatureAt ? '계약서 보기' : '확인하고 서명')
    : (c.reviewStatus === 'needs' ? '보완하기' : '자세히 보기');
  return (
    <article className={`aq-doc-card ${tone}`}>
      <span className="document-icon" aria-hidden="true">{c.kind === 'agreements' ? '✓' : '▤'}</span>
      <div className="aq-doc-copy">
        <button type="button" className="row-name" onClick={() => onOpen(c.id)}>{c.title}</button>
        <span className="row-sub">{c.releaseTitle || '공통 문서'} · {c.created}</span>
        <div className="aq-doc-state-line">
          <span className="aq-doc-state-pill">{state}</span>
          {c.fileName && <span>{c.fileName}</span>}
          {c.reviewNote && <span>{c.reviewNote}</span>}
        </div>
      </div>
      <button className="button secondary" type="button" onClick={() => onOpen(c.id)}>{btnLabel}</button>
    </article>
  );
}

function SignaturePad({ doc, onSave, onBack }: { doc: DocRecord; onSave: (name: string, data: string) => void; onBack: () => void }) {
  const toast = useToast();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [tab, setTab] = useState<'draw' | 'cert'>('draw');
  const [name, setName] = useState(doc.signerName || '');
  const [ack, setAck] = useState(false);
  const [strokes, setStrokes] = useState(0);
  const drawing = useRef(false);
  const last = useRef<{ x: number; y: number } | null>(null);
  const readOnly = doc.reviewStatus !== 'approved';

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const rect = canvas.getBoundingClientRect();
    const ratio = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.max(1, Math.round(rect.width * ratio));
    canvas.height = Math.max(1, Math.round(rect.height * ratio));
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.lineCap = 'round'; ctx.lineJoin = 'round';
    ctx.strokeStyle = '#212D49'; ctx.lineWidth = 2.7;
  }, [tab]);

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
    ctx.beginPath(); ctx.moveTo(p.x, p.y); ctx.lineTo(p.x + 0.12, p.y + 0.12); ctx.stroke();
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
    onSave(name.trim(), data);
  };

  return (
    <>
      <p className="aq-sign-intro">계약서 내용을 확인하고 서명 방법을 선택해 주세요.</p>
      <div className="aq-sign-doc">
        <small>{readOnly ? '서명 절차를 확인할 문서' : '서명할 문서'} · v{doc.version || '1.0'}</small>
        <strong>{doc.title}</strong>
        <div className="help">검토 완료 · {doc.releaseTitle || '공통 문서'}</div>
      </div>
      <div className="aq-sign-tabs" role="tablist" aria-label="서명 방법">
        <button type="button" role="tab" aria-selected={tab === 'draw'} onClick={() => setTab('draw')}>직접 서명</button>
        <button type="button" role="tab" aria-selected={tab === 'cert'} onClick={() => setTab('cert')}>민간인증서</button>
      </div>
      {tab === 'draw' ? (
        <section role="tabpanel">
          <div className="field">
            <label htmlFor="aqSignerName">서명자 이름</label>
            <input
              type="text" maxLength={100} id="aqSignerName" value={name}
              onChange={e => setName(e.target.value)} autoComplete="name"
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
              {strokes === 0 && <span className="aq-sign-placeholder">여기에 손가락으로 서명해 주세요.</span>}
            </div>
          </div>
          <div className="aq-sign-tools">
            <button type="button" className="link-btn" onClick={clear}>다시 그리기</button>
          </div>
          <label className="aq-sign-check">
            <input type="checkbox" checked={ack} onChange={e => setAck(e.target.checked)} />
            <span>서명할 문서의 내용을 확인했어요.<small>이 단계에서는 서명 이미지를 입력·보관하며, 법적 전자서명과 본인 인증은 별도 연동이 필요해요.</small></span>
          </label>
          {readOnly && <div className="notice">문서 검토가 완료되면 서명을 저장할 수 있어요.</div>}
          <div className="aq-sign-foot">
            <button type="button" className="button" onClick={save} disabled={readOnly}>서명 입력 저장</button>
            <button type="button" className="button secondary" onClick={onBack}>문서로 돌아가기</button>
          </div>
        </section>
      ) : (
        <section role="tabpanel">
          <p className="aq-sign-intro">본인 명의의 인증서를 선택해 계약서 서명을 진행할 수 있어요.</p>
          <div className="aq-sign-provider-list">
            {['PASS', '카카오 인증서', '네이버 인증서', '토스 인증서'].map(x => (
              <button key={x} type="button" className="aq-sign-provider" onClick={() => toast(x + ' 인증 연결을 준비하고 있어요.')}>
                <span>{x}</span><small>인증 연동 준비 중</small>
              </button>
            ))}
          </div>
          <div className="notice">인증 사업자 연결 및 계약 원문에 대한 서명 검증이 준비되면 이 화면에서 진행할 수 있어요. 아직 인증 요청이 전송되지는 않아요.</div>
          <div className="aq-sign-foot">
            <button type="button" className="button secondary" onClick={onBack}>문서로 돌아가기</button>
          </div>
        </section>
      )}
    </>
  );
}

export function Contracts() {
  const navigate = useNavigate();
  const toast = useToast();
  const [docs, setDocs] = useState<DocRecord[]>(INITIAL_DOCS);
  const [openId, setOpenId] = useState<string | null>(null);
  const [signing, setSigning] = useState(false);
  const [confirmed, setConfirmed] = useState(false);

  const openDoc = openId ? docs.find(d => d.id === openId) ?? null : null;
  const agreements = docs.filter(c => c.kind === 'agreements');
  const rights = docs.filter(c => c.kind === 'rights');

  const rightsNeeded = rights.filter(c => !['review', 'prepared', 'approved'].includes(c.reviewStatus)).length;
  const rightsReview = rights.filter(c => ['review', 'prepared'].includes(c.reviewStatus)).length;
  const rightsFix = rights.filter(c => c.reviewStatus === 'needs').length;

  useEffect(() => {
    if (openDoc) setConfirmed(!!openDoc.checkedAt);
  }, [openId]); // eslint-disable-line react-hooks/exhaustive-deps

  const confirmDoc = () => {
    if (!openDoc) return;
    if (!confirmed) { toast('문서 내용을 확인해 주세요.'); return; }
    const stamp = new Date().toLocaleString('ko-KR', { year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });
    setDocs(ds => ds.map(d => d.id === openDoc.id ? {
      ...d,
      checkedAt: d.checkedAt || stamp,
      consentHistory: d.checkedAt ? d.consentHistory : [...d.consentHistory, { time: stamp, action: '내용 확인', version: d.version || '1.0' }],
    } : d));
    setOpenId(null);
    toast('문서 확인 기록을 저장했어요.');
  };

  const saveSignature = (name: string, _data: string) => {
    if (!openDoc) return;
    const stamp = new Date().toLocaleString('ko-KR', { year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });
    setDocs(ds => ds.map(d => d.id === openDoc.id ? {
      ...d,
      signerName: name,
      localSignatureAt: stamp,
      reviewHistory: [...d.reviewHistory, { status: '직접 서명 입력 보관', time: stamp, detail: '서명 이미지 보관 · 본인 인증 및 법적 전자서명 대기' }],
    } : d));
    setSigning(false);
    toast('서명 입력을 보관했어요.');
  };

  const openSignature = () => {
    if (!openDoc) return;
    if (openDoc.reviewStatus !== 'approved') {
      toast('검토가 완료된 문서에서 서명을 진행할 수 있어요.');
      return;
    }
    setSigning(true);
  };

  return (
    <>
      <div className="view-title">
        <div>
          <p className="eyebrow">AUDENIQ AGREEMENTS</p>
          <h1>계약서</h1>
          <p>AUDENIQ과 체결하는 배급 계약과 발매 신청서를 확인하고 서명하세요.</p>
        </div>
      </div>

      <div className="studio-doc-path">
        <div>
          <span className="studio-path-n">01</span>
          <strong>신청 정보 확인</strong>
          <small>발매·권리자 정보 자동 정리</small>
        </div>
        <div>
          <span className="studio-path-n">02</span>
          <strong>계약 내용 확인</strong>
          <small>배급 범위와 필수 조항 확인</small>
        </div>
        <div>
          <span className="studio-path-n">03</span>
          <strong>서명·접수</strong>
          <small>서명 후 AUDENIQ에 즉시 접수</small>
        </div>
      </div>

      <div id="contractList">
        {agreements.length ? (
          <div className="aq-doc-grid">
            {agreements.map(c => <DocCard key={c.id} c={c} onOpen={setOpenId} />)}
          </div>
        ) : (
          <div className="empty-page">
            <p>서명할 계약서가 없어요.</p>
            <span>발매를 신청하면 입력 내용이 정리된 계약서가 자동으로 준비돼요.</span>
          </div>
        )}
      </div>

      <div className="section-top" style={{ marginTop: 40 }}>
        <h2>권리·보완 서류</h2>
        <button type="button" className="link-btn" onClick={() => navigate('/rights')}>전체 보기 ↗</button>
      </div>
      <div className="aq-rights-summary">
        <div><span>제출할 서류</span><strong>{rightsNeeded}</strong></div>
        <div><span>검토 중</span><strong>{rightsReview}</strong></div>
        <div><span>보완 요청</span><strong>{rightsFix}</strong></div>
      </div>
      <div id="rightsList">
        {rights.length ? (
          <div className="aq-doc-grid">
            {rights.map(c => <DocCard key={c.id} c={c} onOpen={setOpenId} />)}
          </div>
        ) : (
          <div className="empty-page">
            <p>제출할 권리 서류가 없어요.</p>
            <span>보완 자료가 필요하면 요청 내용과 제출 항목이 여기에 표시돼요.</span>
          </div>
        )}
      </div>

      <div className="notice" style={{ marginTop: 25 }}>
        발매 신청 정보는 계약서에 자동으로 정리돼요. 내용을 확인한 뒤 서명만 진행하면 돼요.
      </div>

      {openDoc && !signing && (
        <Modal title={openDoc.title} onClose={() => setOpenId(null)}>
          <div className="doc-meta">
            <span className="doc-pill">{openDoc.kind === 'agreements' ? '계약서' : '권리 증빙'}</span>
            <span className="doc-pill">{aqDocumentState(openDoc)}</span>
          </div>
          <div className="information">
            <div><dt>관련 발매</dt><dd>{openDoc.releaseTitle || '공통'}</dd></div>
            <div><dt>등록일</dt><dd>{openDoc.created}</dd></div>
            <div><dt>문서 버전</dt><dd>{openDoc.version || '1.0'}</dd></div>
            <div><dt>첨부 파일</dt><dd>{openDoc.fileName || '없음'}</dd></div>
          </div>
          <h3 className="doc-section-title">문서 내용</h3>
          {openDoc.content ? (
            <div className="doc-content" style={{ whiteSpace: 'pre-wrap' }}>{openDoc.content}</div>
          ) : (
            <div className="doc-loading">첨부 파일을 확인하고 있어요.</div>
          )}
          <h3 className="doc-section-title">확인 및 동의 내역</h3>
          {openDoc.consentHistory.length ? (
            <div className="doc-audit">
              {openDoc.consentHistory.map((h, i) => (
                <div key={i}><strong>{h.action}</strong><span>{h.time}</span><span>문서 버전: {h.version}</span></div>
              ))}
            </div>
          ) : (
            <p className="muted small">아직 내용 확인 기록이 없어요.</p>
          )}
          {openDoc.reviewHistory.length > 0 && (
            <>
              <h3 className="doc-section-title">진행 기록</h3>
              <div className="process-timeline">
                {openDoc.reviewHistory.map((h, i) => (
                  <div key={i} className="timeline-row">
                    <strong>{h.status}</strong>
                    <span className="row-sub">{h.time} · {h.detail}</span>
                  </div>
                ))}
              </div>
            </>
          )}
          {openDoc.reviewStatus === 'needs' && (
            <>
              <div className="notice error" style={{ marginTop: 16 }}>{openDoc.reviewNote || '보완을 요청한 서류를 첨부해 주세요.'}</div>
              <div className="field" style={{ marginTop: 16 }}>
                <label htmlFor="aqEvidenceFile">요청된 서류 첨부</label>
                <input
                  type="file" id="aqEvidenceFile"
                  accept=".pdf,.txt,image/png,image/jpeg,image/webp,application/pdf,text/plain"
                  onChange={e => {
                    const f = e.target.files?.[0];
                    if (f) {
                      setDocs(ds => ds.map(d => d.id === openDoc.id ? { ...d, fileName: f.name } : d));
                      toast('서류를 첨부했어요.');
                    }
                  }}
                />
                <p className="help">서류를 선택하면 원본과 발매 정보가 함께 보관돼요.</p>
              </div>
            </>
          )}
          <label className="check-line doc-consent">
            <input type="checkbox" checked={confirmed} onChange={e => setConfirmed(e.target.checked)} />
            <span>
              위 문서의 내용을 읽고 확인했어요.
              <small>확인 시간을 기록합니다. 법적 전자서명이나 AUDENIQ의 계약 승인·권리 심사를 대신하지 않아요.</small>
            </span>
          </label>
          <div className="doc-connection">
            현재 서류 접수 시스템과 연결되지 않았어요. 아래의 제출 준비는 AUDENIQ 담당자에게 전달되지 않아요.
          </div>
          {openDoc.kind === 'agreements' && (
            <div className="aq-doc-extra" style={{ marginTop: 16 }}>
              <button type="button" className="button" onClick={openSignature}>
                {openDoc.reviewStatus === 'approved' ? '서명 진행하기' : '서명 절차 확인'}
              </button>
            </div>
          )}
          <div className="doc-actions" style={{ marginTop: 16 }}>
            <button type="button" className="button" onClick={confirmDoc}>확인 및 저장</button>
            <button type="button" className="button secondary" onClick={() => toast('검토 요청을 준비했어요. (테스트 모드)')}>검토 요청</button>
            <button
              type="button" className="button ghost"
              onClick={() => {
                if (!window.confirm('문서를 삭제할까요? 등록된 첨부 원본과 확인 내역이 함께 삭제돼요.')) return;
                setDocs(ds => ds.filter(d => d.id !== openDoc.id));
                setOpenId(null);
                toast('문서를 삭제했어요.');
              }}
            >
              문서 삭제
            </button>
          </div>
        </Modal>
      )}

      {openDoc && signing && (
        <Modal title="계약서 서명" onClose={() => setSigning(false)}>
          <SignaturePad doc={openDoc} onSave={saveSignature} onBack={() => setSigning(false)} />
        </Modal>
      )}
    </>
  );
}
