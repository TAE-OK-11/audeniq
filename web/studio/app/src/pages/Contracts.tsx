// 계약서 — 라이브 view-contracts / renderContracts(오버라이드) 대응
import { OrgContractEnd } from '../components/OrgContractEnd';
import { useEffect, useState } from 'react';
import { useNavigate, useSearchParams } from '../lib/router';
import { useDocs } from '../store/docs';
import { DocEmpty } from '../components/DocCard';
import { ContractBundle, bundleStage, type BundleStage } from '../components/ContractBundle';
import { api } from '../api/client';
import { useAsync } from '../hooks/useAsync';
import { DocumentModal } from '../components/DocumentModal';
import { SignatureModal } from '../components/SignatureModal';
import { refreshDocs } from '../store/portalSync';
import { MOCK } from '../lib/mode';

export function Contracts() {
  const docs = useDocs();
  const [openId, setOpenId] = useState<string | null>(null);
  const [signing, setSigning] = useState(false);

  const [params, setParams] = useSearchParams();
  const nav = useNavigate();
  // 담당자 승인으로 계약서가 ‘서명 필요’로 바뀌었을 수 있다 — 화면을 열 때 새로 받는다
  useEffect(() => { if (!MOCK) void refreshDocs(); }, []);
  const openDoc = openId ? docs.find(d => d.id === openId) ?? null : null;
  // 발매마다 신청서 + 계약서를 한 묶음으로 — 서명할 것, 검토·보완 중, 체결 완료 순, 같은 묶음 안에서는 최신순
  const ORDER: Record<BundleStage, number> = { 'to-sign': 0, needs: 1, review: 2, waiting: 3, signed: 4 };
  // 내 발매 목록에 있는 발매의 계약서만 — 삭제한 발매 등 목록에 없는 발매의 계약서는 보이지 않게
  const { data: releases } = useAsync(() => api.listReleases(), []);
  const mine = releases ? new Set(releases.map(r => r.id)) : null;
  const agreements = docs
    .filter(c => c.kind === 'agreements' && (!mine || !c.releaseId || mine.has(c.releaseId)))
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
      <div className="ui-page-head">
        <div>
          <p className="eyebrow">계약·권리</p>
          <h1>계약서</h1>
          <p>발매마다 배급 신청서와 계약서를 확인하고 서명해요.</p>
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

      {/* 진행 순서는 처음 한 번 읽으면 되는 설명 — 서명할 계약서보다 위에 있어 매번 지나쳐야 했다 */}
      <section className="aq-doc-path-wrap" aria-labelledby="docPathTitle">
        <h2 id="docPathTitle">계약은 이렇게 진행돼요</h2>
        <div className="studio-doc-path">
          <div>
            <span className="studio-path-n">1단계</span>
            <strong>배급 신청서 서명</strong>
            <small>발매를 접수할 때</small>
          </div>
          <div>
            <span className="studio-path-n">2단계</span>
            <strong>담당자 검토</strong>
            <small>수수료·배급 조건 확정</small>
          </div>
          <div>
            <span className="studio-path-n">3단계</span>
            <strong>배급 계약서 서명</strong>
            <small>권리 확인 체크 후 배급 시작</small>
          </div>
        </div>
      </section>

      <OrgContractEnd />

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
