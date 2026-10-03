// 계약서 — 라이브 view-contracts / renderContracts(오버라이드) 대응
import { useEffect, useState } from 'react';
import { useNavigate, useSearchParams } from '../lib/router';
import { useDocs } from '../store/docs';
import { DocEmpty } from '../components/DocCard';
import { ContractBundle, bundleStage, type BundleStage } from '../components/ContractBundle';
import { DocumentModal } from '../components/DocumentModal';
import { SignatureModal } from '../components/SignatureModal';

export function Contracts() {
  const docs = useDocs();
  const [openId, setOpenId] = useState<string | null>(null);
  const [signing, setSigning] = useState(false);

  const [params, setParams] = useSearchParams();
  const nav = useNavigate();
  const openDoc = openId ? docs.find(d => d.id === openId) ?? null : null;
  // 발매마다 신청서 + 계약서를 한 묶음으로 — 서명할 것, 검토·보완 중, 체결 완료 순, 같은 묶음 안에서는 최신순
  const ORDER: Record<BundleStage, number> = { 'to-sign': 0, needs: 1, review: 2, waiting: 3, signed: 4 };
  const agreements = docs
    .filter(c => c.kind === 'agreements')
    .sort((a, b) => ORDER[bundleStage(a)] - ORDER[bundleStage(b)]
      || String(b.created || '').localeCompare(String(a.created || '')));
  // 발매 상세에서 ‘확인하고 서명’으로 들어오면 바로 연다 (?doc=)
  const linked = params.get('doc');
  useEffect(() => {
    if (linked && docs.some(d => d.id === linked)) { setOpenId(linked); setParams({}, { replace: true }); }
  }, [linked, docs]); // eslint-disable-line react-hooks/exhaustive-deps

  const openSignature = () => {
    if (!openDoc) return;
    // 라이브 openSignatureFlow(c, readOnly): 미승인 문서는 절차 확인 모드
    setSigning(true);
  };

  return (
    <div id="view-contracts" className="view">
      <div className="view-title">
        <div>
          <p className="eyebrow">AUDENIQ AGREEMENTS</p>
          <h1>계약서</h1>
          <p>발매마다 배급 신청서와 배급 계약서를 한곳에서 확인하고 서명하세요.</p>
        </div>
      </div>

      <div className="studio-doc-path">
        <div>
          <span className="studio-path-n">01</span>
          <strong>배급 신청서 서명</strong>
          <small>발매를 접수할 때</small>
        </div>
        <div>
          <span className="studio-path-n">02</span>
          <strong>담당자 검토</strong>
          <small>수수료·배급 조건 확정</small>
        </div>
        <div>
          <span className="studio-path-n">03</span>
          <strong>배급 계약서 서명</strong>
          <small>권리 확인 체크 후 배급 시작</small>
        </div>
      </div>

      <div id="contractList">
        {agreements.length ? (
          <div className="aq-bundle-list aq-stagger">
            {agreements.map(c => (
              <ContractBundle key={c.id} releaseId={c.releaseId ?? ''} releaseTitle={c.releaseTitle} agreement={c} onOpenAgreement={setOpenId} />
            ))}
          </div>
        ) : (
          <DocEmpty
            title="아직 계약 서류가 없어요."
            desc="발매를 접수하면 배급 신청서와 배급 계약서가 여기에 함께 정리돼요."
          />
        )}
      </div>

      <div className="notice" style={{ marginTop: 25 }}>
        배급 신청서는 접수할 때, 배급 계약서는 담당자 검토가 끝난 뒤 서명해요. 계약서에 서명하면 플랫폼으로 배급이 시작돼요.
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
          onDone={() => { const done = openDoc; setSigning(false); setOpenId(null); if (done) nav(`/contracts/${encodeURIComponent(done.id)}`); }}
          onSaved={() => {}}
        />
      )}
    </div>
  );
}
