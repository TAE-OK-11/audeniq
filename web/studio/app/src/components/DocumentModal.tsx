// 문서 상세 모달 — 라이브 openDocument(오버라이드) 대응
import { FilePicker } from './FilePicker';
import { useEffect, useRef, useState } from 'react';
import { Modal } from './Modal';
import { useNavigate } from '../lib/router';
import { MOCK } from '../lib/mode';
import * as portal from '../api/portal';
import { errorMessage } from '../api/errors';
import { refreshDocs } from '../store/portalSync';
import { useToast } from './Toast';
import { docBadgeTone, updateDoc, type DocRecord } from '../store/docs';
import { fileSize, localStamp } from '../lib/format';
import { stampNow } from '../lib/date';
import { docState } from '../store/docs';
import { Glyph } from './Glyph';
import { Badge } from '../ui';

const statusPill = docState;

interface Stage { status: string; time: string; detail: string }

function buildStages(c: DocRecord): Stage[] {
  const stages: Stage[] = [
    { status: '문서 생성', time: c.created, detail: '제출한 정보를 문서로 정리했어요.' },
    ...c.reviewHistory.slice().sort((a, b) => String(a.time || '').localeCompare(String(b.time || '')))
      .map(h => ({ status: h.status, time: h.time, detail: h.detail })),
  ];
  if (c.fileName) stages.push({ status: '원본 첨부', time: c.uploadedAt || c.created, detail: c.fileName });
  if (c.checkedAt) stages.push({ status: '내용 확인', time: c.checkedAt, detail: `v${c.version || '1.0'} · 내용 확인 기록` });
  if (c.reviewStatus === 'approved' && !stages.some(x => x.status === '검토 완료')) {
    stages.push({ status: '검토 완료', time: c.approvedAt || '', detail: '검토 결과를 반영했어요.' });
  }
  return stages;
}

function FilePreview({ doc }: { doc: DocRecord }) {
  const [body, setBody] = useState<React.ReactNode>('첨부 서류를 확인하고 있어요.');
  useEffect(() => {
    const blob = doc.fileBlob;
    if (!blob) { setBody('원본 파일은 첨부한 기기의 현재 세션에서만 미리 볼 수 있어요. 파일명과 제출 기록은 보관돼요.'); return; }
    let url = '';
    let cancelled = false;
    (async () => {
      try {
        url = URL.createObjectURL(blob);
        if (cancelled) { URL.revokeObjectURL(url); return; }
        if (blob.type.startsWith('image/')) {
          setBody(<img className="doc-image" alt={`${doc.title} 첨부 이미지`} src={url} />);
        } else if (blob.type === 'application/pdf' || /\.pdf$/i.test(doc.fileName)) {
          setBody(<iframe className="doc-pdf" title="첨부 서류" src={url} />);
        } else if (blob.type === 'text/plain') {
          setBody(<>{(await blob.text()).slice(0, 200000)}</>);
        } else {
          setBody('파일을 첨부했어요. 원본 다운로드로 확인할 수 있어요.');
        }
      } catch {
        if (!cancelled) setBody('첨부한 원본을 읽을 수 없어요.');
      }
    })();
    return () => { cancelled = true; if (url) URL.revokeObjectURL(url); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [doc.id, doc.fileBlob]);
  return <div id="aqDocPreview" className="aq-document-snapshot">{body}</div>;
}

export function DocumentModal({
  doc,
  onClose,
  onOpenSignature,
}: {
  doc: DocRecord;
  onClose: () => void;
  onOpenSignature: () => void;
}) {
  const toast = useToast();
  const nav = useNavigate();
  const [sending, setSending] = useState(false);
  const [confirmed, setConfirmed] = useState(!!doc.checkedAt);
  const [pendingFile, setPendingFile] = useState<File | null>(null);
  const downloadUrlRef = useRef<string>('');

  useEffect(() => {
    setConfirmed(!!doc.checkedAt);
    setPendingFile(null);
  }, [doc.id]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => () => {
    if (downloadUrlRef.current) URL.revokeObjectURL(downloadUrlRef.current);
  }, []);

  const versions = doc.consentHistory.filter(x => x.action === '내용 확인');
  const stages = buildStages(doc);

  const confirmSave = async () => {
    if (!confirmed) { toast('문서 내용을 확인해 주세요.'); return; }
    if (!MOCK) {
      try {
        if (!doc.checkedAt) await portal.checkDocument(doc.id);
        await refreshDocs();
        onClose();
        toast('문서 확인 기록을 저장했어요.');
      } catch (err) {
        toast(errorMessage(err, '확인 기록을 저장하지 못했어요.'), 'error');
      }
      return;
    }
    if (!doc.checkedAt) {
      const at = stampNow();
      updateDoc(doc.id, {
        checked: true,
        checkedAt: at,
        consentHistory: [...doc.consentHistory, { time: at, action: '내용 확인', version: doc.version || '1.0' }],
      });
    }
    onClose();
    toast('문서 확인 기록을 저장했어요.');
  };

  const submitReview = async () => {
    if (!MOCK) {
      // 실서버: 확인 기록 → (권리 서류) 원본 업로드 후 검토 요청. 계약서는 담당자 검토가 자동으로 시작된다.
      if (!doc.checkedAt && !confirmed) { toast('문서 내용을 확인했다고 체크해 주세요.'); return; }
      if (doc.kind === 'rights' && !pendingFile && !doc.fileName) { toast('요청된 증빙 서류를 첨부해 주세요.'); return; }
      // 보완 요청을 받은 서류는 예전 파일이 남아 있어도 새 파일을 올려야 다시 검토된다
      if (doc.kind === 'rights' && doc.reviewStatus === 'needs' && !pendingFile) { toast('보완한 서류를 새로 첨부해 주세요.', 'error'); return; }
      if (pendingFile && !/\.(pdf|jpe?g|png)$/i.test(pendingFile.name)) { toast('서류는 PDF, JPG, PNG 파일로 올려 주세요.'); return; }
      if (['review', 'prepared', 'approved'].includes(doc.reviewStatus) && !pendingFile) { toast('이미 검토 요청된 문서예요.', 'info'); return; }
      setSending(true);
      try {
        // 현재 문서 버전을 서버에서 받아 제출한다 (담당자가 그사이 요청을 바꿨을 수 있다)
        const rv = await portal.checkDocument(doc.id);
        if (pendingFile && doc.kind === 'rights') await portal.submitProof(doc.id, pendingFile, rv);
        await refreshDocs();
        onClose();
        toast(doc.kind === 'rights' ? '서류를 제출했어요. 검토가 끝나면 알려 드릴게요.' : '문서 확인 기록을 저장했어요.');
      } catch (err) {
        toast(errorMessage(err, '서류를 제출하지 못했어요. 잠시 후 다시 시도해 주세요.'), 'error');
      } finally {
        setSending(false);
      }
      return;
    }
    let base: Partial<DocRecord> = {};
    let history = doc.reviewHistory;
    let fileName = doc.fileName;
    // 체크박스만 체크하고 바로 검토 요청을 누른 경우 확인 기록을 함께 남긴다
    if (!doc.checkedAt && confirmed) {
      const at = stampNow();
      base = {
        checked: true,
        checkedAt: at,
        consentHistory: [...doc.consentHistory, { time: at, action: '내용 확인', version: doc.version || '1.0' }],
      };
    }
    if (pendingFile) {
      const okType = ['application/pdf', 'image/jpeg', 'image/png', 'image/webp', 'text/plain'].includes(pendingFile.type)
        || /\.(pdf|txt)$/i.test(pendingFile.name);
      if (!okType) { toast('PDF, 이미지 또는 TXT 형식의 서류를 첨부해 주세요.'); return; }
      const at = stampNow();
      fileName = pendingFile.name;
      history = [...history, { status: '서류 등록', time: at, detail: pendingFile.name + ' 원본 첨부' }];
      base = { ...base, fileName: pendingFile.name, fileBlob: pendingFile, fileType: pendingFile.type, uploadedAt: at };
    }
    if (!doc.checkedAt && !confirmed) { toast('문서 내용을 확인했다고 체크해 주세요.'); return; }
    if (['review', 'prepared', 'approved'].includes(doc.reviewStatus) && !pendingFile) {
      toast('이미 검토 요청된 문서예요.', 'info');
      return;
    }
    if (doc.kind === 'rights' && !fileName) { toast('요청된 증빙 서류를 첨부해 주세요.'); return; }
    const at = stampNow();
    updateDoc(doc.id, {
      ...base,
      reviewStatus: 'prepared',
      reviewHistory: [...history, { status: '검토 요청 대기', time: at, detail: '접수할 준비를 마쳤어요.' }],
    });
    onClose();
    toast('서류를 제출 대기 상태로 보관했어요.');
  };

  const download = () => {
    const blob = pendingFile || doc.fileBlob;
    if (!blob) { toast('첨부한 원본을 찾을 수 없어요.'); return; }
    if (downloadUrlRef.current) URL.revokeObjectURL(downloadUrlRef.current);
    const url = URL.createObjectURL(blob);
    downloadUrlRef.current = url;
    const a = document.createElement('a');
    a.href = url;
    a.download = doc.fileName || 'AUDENIQ-document';
    a.click();
    window.setTimeout(() => {
      if (downloadUrlRef.current === url) {
        URL.revokeObjectURL(url);
        downloadUrlRef.current = '';
      }
    }, 30000);
  };

  const toSign = doc.kind === 'agreements' && doc.reviewStatus === 'approved' && !doc.localSignatureAt;
  // 원본을 내고 검토를 요청할 수 있는 서류 (전자 문서는 서명으로 접수된다)
  const canSubmit = !doc.electronic && (doc.reviewStatus === 'awaiting_documents' || doc.reviewStatus === 'needs');
  // 첨부 칸은 실제로 낼 수 있을 때만 — 검토 중·승인된 서류에 내보낼 버튼 없는 첨부 칸을 두지 않는다
  const needsFile = canSubmit && (doc.reviewStatus === 'awaiting_documents' || doc.kind === 'rights');
  const tone = docBadgeTone(doc);
  // 맨 위 '지금 할 일' 한 줄 — 어디까지 왔고 무엇을 하면 되는지 먼저
  const next = toSign ? { icon: 'pencil' as const, title: '서명만 남았어요', desc: '내용을 확인하고 서명하면 바로 배급을 시작해요.' }
    : doc.reviewStatus === 'needs' ? { icon: 'alert' as const, title: '보완이 필요해요', desc: doc.reviewNote || (doc.electronic ? '보완 내용을 반영한 전자 문서를 새로 작성해 주세요.' : '보완을 요청한 서류를 첨부해 주세요.') }
      : doc.reviewStatus === 'awaiting_documents' ? { icon: 'clip' as const, title: doc.kind === 'rights' ? '서류를 첨부해 주세요' : '내용을 확인해 주세요', desc: doc.kind === 'rights' ? '원본을 첨부하고 검토 요청을 누르면 담당자가 확인해요.' : '확인하면 담당자 검토가 시작돼요.' }
        : doc.reviewStatus === 'review' || doc.reviewStatus === 'prepared' ? { icon: 'clock' as const, title: '담당자가 검토하고 있어요', desc: '검토가 끝나면 알림으로 알려 드려요.' }
          : doc.localSignatureAt ? { icon: 'check' as const, title: doc.kind === 'agreements' ? '계약이 체결됐어요' : '서명을 마쳤어요', desc: `${doc.signerName || '서명자'} · ${localStamp(doc.localSignatureAt)}` }
            : doc.reviewStatus === 'approved' ? { icon: 'check' as const, title: '승인됐어요', desc: '따로 할 일은 없어요.' } : null;

  return (
    <Modal title={doc.title} onClose={onClose}>
      <p className="aq-meta-line">
        <Badge tone={tone} className="aq-doc-state">{statusPill(doc)}</Badge>
        {doc.kind === 'agreements' ? '계약·신청서' : '권리 증빙'} · v{doc.version || '1.0'}
      </p>

      {next && (
        <div className={`aq-dm-next is-${tone}`}>
          <span className="aq-dm-next-ico" aria-hidden="true"><Glyph name={next.icon} size={18} /></span>
          <span className="min-0"><strong>{next.title}</strong><small>{next.desc}</small></span>
        </div>
      )}

      <section className="aq-dm-sec" aria-label="문서 정보">
        <h4>문서 정보</h4>
        <dl className="aq-dm-kv">
          <div><dt>관련 발매</dt><dd>{doc.releaseTitle || '공통'}</dd></div>
          <div><dt>작성일</dt><dd>{localStamp(doc.created)}</dd></div>
          {doc.fileName && <div><dt>첨부 원본</dt><dd className="break">{doc.fileName}</dd></div>}
        </dl>
        {/* 본문이 있을 때만 — '원본을 확인해 주세요' 같은 빈 안내 칸은 두지 않는다 */}
        {doc.content && <div className="aq-document-snapshot">{doc.content}</div>}
        {doc.fileName && <FilePreview doc={doc} />}
        <div className="aq-dm-links">
          {doc.kind === 'agreements' && doc.releaseId && (
            <button type="button" className="aq-dm-link aq-doc-open-app" onClick={() => nav(`/releases/${encodeURIComponent(doc.releaseId!)}/application`)}>
              <Glyph name="doc" size={17} /><span>배급 신청서 보기</span><Glyph name="chevron-right" size={16} />
            </button>
          )}
          {doc.electronic && (
            <button type="button" className="aq-dm-link aq-doc-open-app" onClick={() => { onClose(); nav(`/rights/${encodeURIComponent(doc.id)}`); }}>
              <Glyph name="doc" size={17} /><span>계약서 보기 · PDF 저장</span><Glyph name="chevron-right" size={16} />
            </button>
          )}
          {doc.fileName && (
            <button id="aqDocDownload" type="button" className="aq-dm-link" onClick={download}>
              <Glyph name="download" size={17} /><span>원본 열기</span><Glyph name="chevron-right" size={16} />
            </button>
          )}
        </div>
      </section>

      {needsFile && (
        <section className="aq-dm-sec" aria-label="서류 첨부">
          <h4>{doc.reviewStatus === 'needs' ? '보완 서류 첨부' : '요청된 서류 첨부'}</h4>
          <FilePicker
            id="aqEvidenceFile" fileName={pendingFile?.name}
            accept=".pdf,.txt,image/png,image/jpeg,image/webp,application/pdf,text/plain"
            onChange={e => setPendingFile(e.target.files?.[0] || null)}
          />
          <p className="ui-field-help">
            {pendingFile ? `선택한 파일 · ${pendingFile.name} (${fileSize(pendingFile.size)}) — ‘검토 요청’을 누르면 제출돼요.` : '서류를 선택하면 원본과 발매 정보가 함께 보관돼요.'}
          </p>
        </section>
      )}

      {doc.localSignatureData && (
        <section className="aq-dm-sec aq-sign-record" aria-label="서명 기록">
          <h4>{doc.electronic ? '권리자 서명' : '직접 서명 기록'}</h4>
          <img src={doc.localSignatureData} alt="직접 입력한 서명" />
          <small>{doc.signerName || '서명자'} · {localStamp(doc.localSignatureAt)}</small>
          <small>{doc.electronic ? `서명자 구분: ${doc.electronic.signer_role} · 문서 번호: ${doc.electronic.document_no}` : '본인 인증과 법적 전자서명은 따로 진행해요.'}</small>
        </section>
      )}

      <section className="aq-dm-sec" aria-label="처리 과정">
        <h4>처리 과정</h4>
        <ol className="aq-dm-steps">
          {stages.map((h, i) => (
            <li key={i} className={i === stages.length - 1 ? 'is-now' : ''}>
              <strong>{h.status || '확인'}</strong>
              <small>{h.time ? localStamp(h.time) : '진행 예정'}</small>
              {h.detail && <p>{h.detail}</p>}
            </li>
          ))}
        </ol>
      </section>

      {/* 서명할 계약서는 서명이 곧 확인 — 따로 '읽고 확인했어요'를 받지 않는다 */}
      {!toSign && (
        <section className="aq-dm-sec" aria-label="확인 기록">
          <h4>확인 기록</h4>
          {versions.map((h, i) => (
            <p key={i} className="aq-dm-audit"><Glyph name="check" size={14} />v{h.version || '1.0'} 확인 · {localStamp(h.time)}</p>
          ))}
          {!versions.length && doc.checkedAt && <p className="aq-dm-audit"><Glyph name="check" size={14} />v{doc.version || '1.0'} 확인 · {localStamp(doc.checkedAt)}</p>}
          {!doc.checkedAt && (
            <label className="aq-dm-consent ui-check is-plain">
              <input id="aqConfirmDoc" type="checkbox" checked={confirmed} onChange={e => setConfirmed(e.target.checked)} />
              <span>문서 내용을 읽고 확인했어요.<small>확인 시간과 문서 버전이 기록돼요. 계약 서명과는 별도예요.</small></span>
            </label>
          )}
        </section>
      )}

      <div className="doc-actions aq-sticky-foot aq-dm-foot">
        {toSign ? (
          <button type="button" id="aqSignIntent" className="ui-btn is-primary" onClick={onOpenSignature}>서명하기</button>
        ) : (
          <>
            {canSubmit && <button id="aqDocSubmit" type="button" className="ui-btn is-primary" onClick={submitReview} disabled={sending}>{sending ? '제출하는 중' : '검토 요청'}</button>}
            {doc.kind === 'agreements' && (
              <button type="button" id="aqSignIntent" className="ui-btn is-soft" onClick={onOpenSignature}>
                {doc.reviewStatus === 'approved' ? '서명 다시 하기' : '검토 진행 상황 보기'}
              </button>
            )}
            {!doc.checkedAt
              ? <button id="aqDocConfirm" type="button" className={`ui-btn ${canSubmit || doc.kind === 'agreements' ? 'is-soft' : 'is-primary'}`} onClick={confirmSave}>확인하고 저장하기</button>
              : !canSubmit && doc.kind !== 'agreements' && <button type="button" className="ui-btn is-primary" onClick={onClose}>닫기</button>}
          </>
        )}
      </div>
    </Modal>
  );
}
