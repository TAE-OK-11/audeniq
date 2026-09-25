// 계약서 — 라이브 view-contracts / renderContracts(오버라이드) 대응
import { useState } from 'react';
import { useDocs } from '../store/docs';
import { DocCard, DocEmpty } from '../components/DocCard';
import { DocumentModal } from '../components/DocumentModal';
import { SignatureModal } from '../components/SignatureModal';

export function Contracts() {
  const docs = useDocs();
  const [openId, setOpenId] = useState<string | null>(null);
  const [signing, setSigning] = useState(false);

  const openDoc = openId ? docs.find(d => d.id === openId) ?? null : null;
  const agreements = docs
    .filter(c => c.kind === 'agreements')
    .slice()
    .sort((a, b) => String(b.created || '').localeCompare(String(a.created || '')));

  const openSignature = () => {
    if (!openDoc) return;
    // 라이브 openSignatureFlow(c, readOnly): 미승인 문서는 절차 확인 모드
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
          <DocEmpty
            title="서명할 계약서가 없어요."
            desc="발매를 신청하면 입력 내용이 정리된 계약서가 자동으로 준비돼요."
          />
        )}
      </div>

      <div className="notice" style={{ marginTop: 25 }}>
        발매 신청 정보는 계약서에 자동으로 정리돼요. 내용을 확인한 뒤 서명만 진행하면 돼요.
      </div>

      {openDoc && !signing && (
        <DocumentModal
          doc={openDoc}
          onClose={() => setOpenId(null)}
          onOpenSignature={openSignature}
        />
      )}

      {openDoc && signing && (
        <SignatureModal
          doc={openDoc}
          onBack={() => setSigning(false)}
          onSaved={() => setSigning(false)}
        />
      )}
    </>
  );
}
