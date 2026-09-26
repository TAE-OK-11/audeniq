// 문서 상세 모달 — 라이브 openDocument(오버라이드) 대응
import { useEffect, useRef, useState } from 'react';
import { Modal } from './Modal';
import { useToast } from './Toast';
import { updateDoc, type DocRecord } from '../store/docs';
import { localStamp } from '../lib/format';

function stampNow(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

function statusPill(c: DocRecord): string {
  if (c.reviewStatus === 'awaiting_documents') return '서류 접수 대기';
  if (c.reviewStatus === 'approved') return '검토 완료';
  return '접수 대기';
}

interface Stage { status: string; time: string; detail: string }

function buildStages(c: DocRecord): Stage[] {
  const stages: Stage[] = [
    { status: '문서 생성', time: c.created, detail: '제출 정보가 문서로 정리됐어요.' },
    ...c.reviewHistory.slice().sort((a, b) => String(a.time || '').localeCompare(String(b.time || '')))
      .map(h => ({ status: h.status, time: h.time, detail: h.detail })),
  ];
  if (c.fileName) stages.push({ status: '원본 첨부', time: c.uploadedAt || c.created, detail: c.fileName });
  if (c.checkedAt) stages.push({ status: '내용 확인', time: c.checkedAt, detail: `v${c.version || '1.0'} · 내용 확인 기록` });
  if (c.reviewStatus === 'approved' && !stages.some(x => x.status === '검토 완료')) {
    stages.push({ status: '검토 완료', time: c.approvedAt || '', detail: '검토 결과가 반영됐어요.' });
  }
  return stages;
}

function FilePreview({ doc }: { doc: DocRecord }) {
  const [body, setBody] = useState<React.ReactNode>('첨부 서류를 확인하고 있어요.');
  useEffect(() => {
    const blob = doc.fileBlob;
    if (!blob) { setBody('첨부 서류를 확인하고 있어요.'); return; }
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
  const [confirmed, setConfirmed] = useState(!!doc.checkedAt);
  const [pendingFile, setPendingFile] = useState<File | null>(null);
  const downloadUrlRef = useRef<string>('');
  const timersRef = useRef<number[]>([]);

  useEffect(() => {
    setConfirmed(!!doc.checkedAt);
    setPendingFile(null);
  }, [doc.id]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => () => {
    if (downloadUrlRef.current) URL.revokeObjectURL(downloadUrlRef.current);
    timersRef.current.forEach(t => window.clearTimeout(t));
    timersRef.current = [];
  }, []);

  const versions = doc.consentHistory.filter(x => x.action === '내용 확인');
  const stages = buildStages(doc);

  const confirmSave = () => {
    if (!confirmed) { toast('문서 내용을 확인해 주세요.'); return; }
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

  const submitReview = () => {
    let base: Partial<DocRecord> = {};
    let history = doc.reviewHistory;
    let fileName = doc.fileName;
    if (pendingFile) {
      const okType = ['application/pdf', 'image/jpeg', 'image/png', 'image/webp', 'text/plain'].includes(pendingFile.type)
        || /\.(pdf|txt)$/i.test(pendingFile.name);
      if (!okType) { toast('PDF, 이미지 또는 TXT 형식의 서류를 첨부해 주세요.'); return; }
      const at = stampNow();
      fileName = pendingFile.name;
      history = [...history, { status: '서류 등록', time: at, detail: pendingFile.name + ' 원본 첨부' }];
      base = { fileName: pendingFile.name, fileBlob: pendingFile, fileType: pendingFile.type, uploadedAt: at };
    }
    if (!doc.checkedAt) { toast('먼저 문서 내용을 확인하고 저장해 주세요.'); return; }
    if (doc.kind === 'rights' && !fileName) { toast('요청된 증빙 서류를 첨부해 주세요.'); return; }
    const at = stampNow();
    updateDoc(doc.id, {
      ...base,
      reviewStatus: 'prepared',
      reviewHistory: [...history, { status: '검토 요청 대기', time: at, detail: '접수 전송 준비가 완료됐어요.' }],
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
    const timer = window.setTimeout(() => {
      if (downloadUrlRef.current === url) {
        URL.revokeObjectURL(url);
        downloadUrlRef.current = '';
      }
    }, 30000);
    // unmount 시 타이머 정리 (누수 방지)
    timersRef.current.push(timer);
  };

  return (
    <Modal title={doc.title} onClose={onClose}>
      <div className="doc-meta">
        <span className="doc-pill">{doc.kind === 'agreements' ? '계약·신청서' : '권리 증빙'}</span>
        <span className="doc-pill">{statusPill(doc)}</span>
      </div>
      <dl className="information">
        <div><dt>관련 발매</dt><dd>{doc.releaseTitle || '공통'}</dd></div>
        <div><dt>작성일</dt><dd>{localStamp(doc.created)}</dd></div>
        <div><dt>버전</dt><dd>v{doc.version || '1.0'}</dd></div>
      </dl>

      <h3 className="doc-section-title">문서 내용</h3>
      <div className="aq-document-snapshot">{doc.content || '첨부된 문서의 원본을 확인해 주세요.'}</div>
      {doc.fileName && (
        <>
          <p className="small muted">첨부 파일 · {doc.fileName}</p>
          <FilePreview doc={doc} />
        </>
      )}

      <h3 className="doc-section-title">확인 및 동의 기록</h3>
      {versions.length ? versions.map((h, i) => (
        <div key={i} className="doc-audit">
          <strong>확인 완료 · v{h.version || '1.0'}</strong>
          <span>{localStamp(h.time)}</span>
        </div>
      )) : (
        <p className="muted small">아직 확인한 내역이 없어요.</p>
      )}

      <label className="doc-consent check-line">
        <input
          id="aqConfirmDoc" type="checkbox"
          checked={confirmed} onChange={e => setConfirmed(e.target.checked)}
        />
        <span>
          위 문서의 내용을 읽고 확인했어요.
          <small>확인 시간과 문서 버전이 기록돼요. 실제 계약 서명과는 별도로 처리돼요.</small>
        </span>
      </label>

      <div className="aq-process">
        <h3>처리 과정</h3>
        <ol>
          {stages.map((h, i) => (
            <li key={i}>
              <strong>{h.status || '확인'}</strong>
              <small>{h.time ? localStamp(h.time) : '진행 예정'}</small>
              {h.detail}
            </li>
          ))}
        </ol>
      </div>

      {doc.localSignatureData && (
        <div className="aq-sign-record">
          <strong>직접 서명 입력 기록</strong>
          <img src={doc.localSignatureData} alt="직접 입력한 서명" />
          <small>{doc.signerName || '서명자'} · {localStamp(doc.localSignatureAt)}</small>
          <small>본인 확인 및 법적 전자서명 처리는 별도 절차에서 진행돼요.</small>
        </div>
      )}

      {doc.reviewStatus === 'needs' && (
        <div className="notice error">{doc.reviewNote || '보완을 요청한 서류를 첨부해 주세요.'}</div>
      )}

      {(doc.reviewStatus === 'awaiting_documents' || doc.kind === 'rights') && (
        <div className="field">
          <label htmlFor="aqEvidenceFile">요청된 서류 첨부</label>
          <input
            type="file" id="aqEvidenceFile"
            accept=".pdf,.txt,image/png,image/jpeg,image/webp,application/pdf,text/plain"
            onChange={e => setPendingFile(e.target.files?.[0] || null)}
          />
          <p className="help">서류를 선택하면 원본과 발매 정보가 함께 보관돼요.</p>
        </div>
      )}

      {doc.kind === 'agreements' && (
        <div className="aq-doc-extra">
          <button type="button" id="aqSignIntent" className="button" onClick={onOpenSignature}>
            {doc.reviewStatus === 'approved' ? '서명 진행하기' : '서명 절차 확인'}
          </button>
        </div>
      )}

      <div className="doc-actions">
        <button id="aqDocConfirm" type="button" className="button" onClick={confirmSave}>확인 및 저장</button>
        <button id="aqDocSubmit" type="button" className="button secondary" onClick={submitReview}>검토 요청</button>
        {doc.fileName && (
          <button id="aqDocDownload" type="button" className="button secondary" onClick={download}>원본 열기</button>
        )}
      </div>
    </Modal>
  );
}
