// 계약서 화면의 저작권 관리 계약 — 배급처럼 신청서(신청할 때 서명)와 계약서(담당자 승인 뒤 서명)가 따로다.
// 배급 계약과 같은 '계약 묶음'(ContractBundle의 aq-bundle) 모양: 머리(이름·상태) + 번호 붙은 줄(신청서 → 계약서).
// 곡 화면의 계약 관리(ContractManage)에서 기간·연장·종료 요청을 다룬다
import { useEffect, useRef, useState } from 'react';
import { useNavigate, useSearchParams } from '../../lib/router';
import { pubApi, type PublishingContract } from '../../api/publishing';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { CONTRACT_LABEL, CONTRACT_TONE, daysLeft, rightName, termLabel, territoryName } from '../../lib/publishing';
import { localStamp, niceDate } from '../../lib/format';
import { useToast } from '../Toast';
import { useConfirm } from '../Confirm';
import { Modal } from '../Modal';
import { CheckIcon } from '../Check';
import { Badge, Meter, Segmented } from '../../ui';

import '../../styles/split/ReleaseDetail.css';
import '../../styles/split/Publishing.css';


function RenewModal({ c, onClose, onDone }: { c: PublishingContract; onClose: () => void; onDone: (c: PublishingContract) => void }) {
  const toast = useToast();
  const { data: policy } = useAsync(() => pubApi.policy(), [], { key: 'pub-policy' });
  const [months, setMonths] = useState<string>('');
  const [busy, setBusy] = useState(false);
  useEffect(() => { if (policy && !months) setMonths(String(policy.renewalOptions[1] ?? policy.renewalOptions[0])); }, [policy]); // eslint-disable-line react-hooks/exhaustive-deps
  const end = (() => {
    if (!months) return '';
    const d = new Date(`${c.endDate}T00:00:00`);
    d.setMonth(d.getMonth() + Number(months));
    return d.toISOString().slice(0, 10);
  })();
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

/** 곡 화면 '등록·계약' 탭의 계약 관리 — 기간·남은 날·연장·종료 요청 (계약서 화면은 서류만 본다) */
export function ContractManage({ contract, canEdit }: { contract: PublishingContract; canEdit: boolean }) {
  const toast = useToast();
  const confirm = useConfirm();
  const [c, setC] = useState(contract);
  const [renewing, setRenewing] = useState(false);
  useEffect(() => setC(contract), [contract]);
  const left = daysLeft(c.endDate);
  const ended = c.status === 'ended';
  const total = Math.max(1, daysLeft(c.endDate, new Date(`${c.startDate}T00:00:00+09:00`)));
  const terminate = async () => {
    const ok = await confirm({
      title: '저작권 관리 계약 종료를 요청할까요?',
      message: '담당자가 관리 단체 절차를 확인한 뒤 종료해요. 종료 후에는 새 곡을 관리에 넣을 수 없고, 그동안의 등록·정산 기록은 남아요.',
      confirmLabel: '종료 요청', danger: true,
    });
    if (!ok) return;
    try { setC(await pubApi.requestTermination(c.id)); toast('종료를 요청했어요.', 'success'); }
    catch (e) { toast(errorMessage(e), 'error'); }
  };
  return (
    <>
      <dl className="ui-kv is-2">
        <div className="aq-kv-code"><dt>계약 기간</dt><dd>{c.startDate} ~ {c.endDate}</dd></div>
        <div><dt>남은 기간</dt><dd>{ended ? '종료' : `${Math.max(0, left)}일`}</dd></div>
        <div><dt>수수료</dt><dd>{c.commissionRate}%</dd></div>
        <div className="aq-kv-code"><dt>관리 범위</dt><dd>{c.territories.map(territoryName).join(', ')} · {c.rights.map(rightName).join(', ')}</dd></div>
      </dl>
      {!ended && <Meter total={total} parts={[{ value: total - Math.max(0, left), tone: left <= 45 ? 'amber' : 'primary' }]} label={`계약 기간 ${Math.max(0, left)}일 남음`} />}
      {c.renewal && <p className="ui-field-help">{termLabel(c.renewal.months)} 연장을 요청했어요 · {localStamp(c.renewal.requestedAt)}</p>}
      {c.terminationRequestedAt && <p className="ui-field-help is-warn">종료를 요청했어요 · {localStamp(c.terminationRequestedAt)}</p>}
      {canEdit && !ended && !c.terminationRequestedAt && (
        <div className="row-actions">
          {!c.renewal && <button type="button" className={`ui-btn ${c.status === 'expiring' ? 'is-primary' : 'is-soft'} is-sm`} onClick={() => setRenewing(true)}>연장하기</button>}
          <button type="button" className="ui-btn is-text is-sm aq-danger-text" onClick={() => void terminate()}>계약 종료 요청</button>
        </div>
      )}
      {renewing && <RenewModal c={c} onClose={() => setRenewing(false)} onDone={setC} />}
    </>
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
