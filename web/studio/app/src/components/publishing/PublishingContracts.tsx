// 계약서 화면의 저작권 관리 계약 — 배급처럼 신청서(신청할 때 서명)와 계약서(담당자 승인 뒤 서명)가 따로다.
// 배급 계약과 같은 '계약 묶음'(ContractBundle의 aq-bundle) 모양: 머리(이름·상태) + 번호 붙은 줄(신청서 → 계약서).
// 곡 화면의 계약 카드(WorkRightsCards › ContractCard)에서 기간·연장·관리 범위·종료 요청을 다룬다
import { useEffect, useRef, useState } from 'react';
import { useNavigate, useSearchParams } from '../../lib/router';
import { pubApi, type PublishingContract } from '../../api/publishing';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { CONTRACT_LABEL, CONTRACT_TONE, addMonths, termLabel } from '../../lib/publishing';
import { niceDate } from '../../lib/format';
import { useToast } from '../Toast';
import { Modal } from '../Modal';
import { CheckIcon } from '../Check';
import { Badge, Segmented } from '../../ui';

import '../../styles/split/ReleaseDetail.css';
import '../../styles/split/Publishing.css';


export function RenewModal({ c, onClose, onDone }: { c: PublishingContract; onClose: () => void; onDone: (c: PublishingContract) => void }) {
  const toast = useToast();
  const { data: policy } = useAsync(() => pubApi.policy(), [], { key: 'pub-policy' });
  const [months, setMonths] = useState<string>('');
  const [busy, setBusy] = useState(false);
  useEffect(() => { if (policy && !months) setMonths(String(policy.renewalOptions[1] ?? policy.renewalOptions[0])); }, [policy]); // eslint-disable-line react-hooks/exhaustive-deps
  const end = months ? addMonths(c.endDate, Number(months)) : '';
  const go = async () => {
    setBusy(true);
    try { onDone(await pubApi.renewContract(c.id, Number(months))); toast('연장을 요청했어요. 담당자가 확인하면 새 기간이 적용돼요.', 'success'); onClose(); }
    catch (e) { toast(errorMessage(e), 'error'); } finally { setBusy(false); }
  };
  return (
    <Modal title="계약 연장" onClose={onClose}>
      <p className="ui-field-help">관리가 끊기지 않도록 끝나기 전에 연장해요. 수수료({c.commissionRate}%)와 관리 범위는 그대로예요.</p>
      {policy && <Segmented fill label="연장 기간" value={months} onChange={setMonths} options={policy.renewalOptions.map(m => ({ value: String(m), label: termLabel(m) }))} />}
      {end && <dl className="ui-kv aq-statement-detail"><div><dt>새 종료일</dt><dd>{end}</dd></div></dl>}
      <div className="aq-confirm-actions">
        <button type="button" className="ui-btn is-soft" onClick={onClose}>닫기</button>
        <button type="button" className="ui-btn is-primary" disabled={busy || !months} onClick={() => void go()}>{busy ? '요청 중' : '연장 요청'}</button>
      </div>
    </Modal>
  );
}

/** 계약서 목록의 저작권 관리 계약 — 발매에 실리지 않은 곡의 계약만 따로 묶는다(발매 곡은 그 발매 계약 묶음 안에).
 *  배급 계약처럼 담당자가 승인해 계약서가 생긴 뒤에만 보이고, 신청 중·보완 중인 건은 곡 화면에서 다룬다.
 *  all: 모든 계약 (발매 곡까지) */
export function PublishingContracts({ all = false }: { all?: boolean }) {
  const nav = useNavigate();
  const [params] = useSearchParams();
  const focus = params.get('pub');
  const { data: contracts } = useAsync(() => pubApi.listContracts(), [], { key: 'pub-contracts' });
  const focused = useRef<HTMLElement | null>(null);
  useEffect(() => { if (focus && contracts) focused.current?.scrollIntoView({ block: 'center', behavior: 'smooth' }); }, [focus, contracts]);
  const list = (contracts ?? []).filter(c => all || c.id === focus || !c.releaseIds?.length);
  if (!list.length) return null;

  return (
    <>
      {list.map(c => {
        const signed = c.status !== 'pending_sign';
        return (
          <article key={c.id} ref={c.id === focus ? el => { focused.current = el; } : undefined}
            className={`aq-bundle aq-pbl-contract is-${signed ? 'signed' : 'to-sign'}`} aria-label={`저작권 관리 계약 ${c.number}`}>
            <header className="aq-bundle-head">
              <div className="min-0">
                <strong>{c.works.map(w => w.title).join(', ')}</strong>
                <span>{c.signedAt ? `${niceDate(c.signedAt)} 서명` : '서명 전'} · {c.number}</span>
              </div>
              <Badge tone={CONTRACT_TONE[c.status]} className="aq-bundle-state">{CONTRACT_LABEL[c.status]}</Badge>
            </header>
            <ol className="aq-bundle-docs">
              <li className="is-done">
                <span className="aq-bundle-no" aria-hidden="true"><CheckIcon size={14} /></span>
                <div className="min-0"><strong>저작권 관리 신청서</strong><span>신청할 때 서명했어요</span></div>
                {c.applicationId && <button type="button" className="ui-btn is-soft is-sm" onClick={() => nav(`/publishing/applications/${encodeURIComponent(c.applicationId!)}`)}>신청서 보기</button>}
              </li>
              <li className={signed ? 'is-done' : 'is-current'}>
                <span className="aq-bundle-no" aria-hidden="true">{signed ? <CheckIcon size={14} /> : 2}</span>
                <div className="min-0"><strong>저작권 관리 계약서</strong><span>{signed ? `${c.startDate} ~ ${c.endDate} · 수수료 ${c.commissionRate}%` : '담당자 승인 완료, 서명 필요'}</span></div>
                <button type="button" className={signed ? 'ui-btn is-soft is-sm' : 'ui-btn is-primary aq-bundle-sign'} onClick={() => nav(`/contracts/publishing/${encodeURIComponent(c.id)}`)}>
                  {signed ? '계약서 보기' : '확인하고 서명'}
                </button>
              </li>
            </ol>
          </article>
        );
      })}
    </>
  );
}
