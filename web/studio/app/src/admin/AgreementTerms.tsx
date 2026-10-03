// 배급 계약 조건 — 담당자가 수수료·배급 형태·특별 요율·특약을 입력하면 서버가 계약서(AUD-DIST 2.0) 본문을 만든다.
// 조건을 입력해야 승인할 수 있고, 승인 후 아티스트가 확인 항목에 체크하고 서명한다.
import { useState } from 'react';
import { staffApi, type AgreementTermsInput, type StaffDocument } from './api';
import { errorMessage } from '../api/errors';
import { useToast } from '../components/Toast';
import { Section, Seg } from './ui';
import { CheckIcon } from '../components/Check';

const pct = (bps: number) => `${(bps / 100).toLocaleString('ko-KR', { maximumFractionDigits: 2 })}%`;

export function AgreementTermsSection({ releaseId, doc, editable, onSaved }: {
  releaseId: string;
  doc: StaffDocument;
  /** 이 발매를 맡은 담당자이고 계약서가 검토 중일 때 */
  editable: boolean;
  onSaved: () => void;
}) {
  const toast = useToast();
  const t = doc.agreement_terms;
  const [exclusivity, setExclusivity] = useState<AgreementTermsInput['exclusivity']>(t?.exclusivity ?? 'NON_EXCLUSIVE');
  const [fee, setFee] = useState(t ? String(t.fee_bps / 100) : '');
  const [rateNote, setRateNote] = useState(t?.rate_note ?? '');
  const [territoryNote, setTerritoryNote] = useState(t?.territory_note ?? '');
  const [minPayout, setMinPayout] = useState(t?.min_payout_note ?? '');
  const [special, setSpecial] = useState(t?.special_terms ?? '');
  const [busy, setBusy] = useState(false);
  const feeBps = Math.round(Number(fee) * 100);
  const feeOk = fee.trim() !== '' && Number.isFinite(feeBps) && feeBps >= 0 && feeBps <= 10000;

  const save = async () => {
    if (!feeOk || busy) return;
    setBusy(true);
    try {
      await staffApi.agreementTerms(releaseId, { exclusivity, fee_bps: feeBps, rate_note: rateNote, territory_note: territoryNote, min_payout_note: minPayout, special_terms: special });
      toast('계약 조건을 저장했어요. 계약서 본문이 새로 만들어졌어요.', 'success');
      onSaved();
    } catch (e) { toast(errorMessage(e)); }
    finally { setBusy(false); }
  };

  const signed = doc.status === 'SIGNED';
  return (
    <Section title="배급 계약 조건" meta={t ? `${t.form} · ${exclusivity === 'EXCLUSIVE' ? '독점' : '비독점'} · 회사 ${pct(t.fee_bps)}` : '입력 전'}>
      {!editable ? (
        t ? (
          <dl className="adm-kv">
            <div><dt>배급 형태</dt><dd>{t.exclusivity === 'EXCLUSIVE' ? '독점' : '비독점'}</dd></div>
            <div><dt>배급수수료</dt><dd>회사 {pct(t.fee_bps)} / 이용자 {pct(t.user_bps)}</dd></div>
            {t.rate_note && <div><dt>특별 요율</dt><dd>{t.rate_note}</dd></div>}
            {t.territory_note && <div><dt>지역 조건</dt><dd>{t.territory_note}</dd></div>}
            {t.min_payout_note && <div><dt>최소지급액</dt><dd>{t.min_payout_note}</dd></div>}
            <div><dt>개별 특약</dt><dd>{t.special_terms || '없음'}</dd></div>
          </dl>
        ) : <p className="small muted">이 발매를 맡은 담당자가 승인 전에 수수료와 배급 형태를 입력해요.</p>
      ) : (
        <div className="adm-terms-form">
          <div className="adm-field"><span className="adm-terms-label">배급 형태</span>
            <Seg label="배급 형태" value={exclusivity} onChange={setExclusivity} options={[['NON_EXCLUSIVE', '비독점 (기본)'], ['EXCLUSIVE', '독점']] as const} /></div>
          <div className="adm-field">
            <label htmlFor="atFee">AUDENIQ 배급수수료 (%)</label>
            <div className="adm-terms-fee">
              <input id="atFee" className="adm-input" inputMode="decimal" value={fee} placeholder="예: 8" onChange={e => setFee(e.target.value.replace(/[^\d.]/g, ''))} aria-invalid={fee !== '' && !feeOk} />
              <span>{feeOk ? `이용자 ${pct(10000 - feeBps)}` : '0~100 사이'}</span>
            </div>
          </div>
          <div className="adm-field"><label htmlFor="atRate">특별 요율 메모 (선택)</label>
            <textarea id="atRate" className="adm-textarea" rows={2} maxLength={500} value={rateNote} onChange={e => setRateNote(e.target.value)} placeholder="레이블 특별요율, 프로모션 요율(기간), Merlin·직계약 등 경로별 요율" /></div>
          <div className="adm-field"><label htmlFor="atTerritory">배급 지역 제외·조건 (선택)</label>
            <input id="atTerritory" className="adm-input" maxLength={300} value={territoryNote} onChange={e => setTerritoryNote(e.target.value)} placeholder="예: 일본 제외" /></div>
          <div className="adm-field"><label htmlFor="atMin">최소지급액 특약 (선택)</label>
            <input id="atMin" className="adm-input" maxLength={300} value={minPayout} onChange={e => setMinPayout(e.target.value)} placeholder="비우면 약관의 기본 기준" /></div>
          <div className="adm-field"><label htmlFor="atSpecial">개별 특약 (선택)</label>
            <textarea id="atSpecial" className="adm-textarea" rows={3} maxLength={2000} value={special} onChange={e => setSpecial(e.target.value)} placeholder="특정 DSP 포함·제외, 전담 지원, 별도 SLA 등 이 고객과만 합의한 내용이 있을 때만" /></div>
          <button type="button" className="adm-btn primary" disabled={!feeOk || busy} onClick={() => void save()}>{busy ? '저장하는 중…' : t ? '계약 조건 다시 저장' : '계약 조건 저장'}</button>
          {!t && <p className="small muted">저장해야 승인할 수 있어요.</p>}
        </div>
      )}
      {doc.body && t && (
        <details className="adm-terms-body">
          <summary>계약서 본문 보기{signed ? ' (서명됨)' : ''}</summary>
          <pre>{doc.body}</pre>
        </details>
      )}
      {doc.confirmations && (
        <div className="adm-terms-checks">
          <b>아티스트 확인 항목</b>
          <ul>{doc.confirmations.items.map(i => <li key={i.id} className={i.checked ? 'is-on' : ''}><span aria-hidden="true">{i.checked && <CheckIcon size={11} />}</span>{i.text}{!i.checked && ' (체크 안 함)'}</li>)}</ul>
        </div>
      )}
    </Section>
  );
}
