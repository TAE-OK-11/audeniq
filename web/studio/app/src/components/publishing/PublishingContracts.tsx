// 계약서 화면의 저작권 관리 계약 — 계약에 든 곡, 기간, 관리 범위, 갱신(연장)·종료 요청·내려받기 (기획안 6.4).
// 배급 계약과 같은 '계약 묶음'(ContractBundle의 aq-bundle) 모양: 머리(이름·상태) + 번호 붙은 줄(신청 → 계약 → 연장).
// 연장 기간과 수수료는 서버 계약 정책에서 받는다. 신청 중인 건(심사 중·보완 요청)도 같은 묶음으로 보여 준다
import { useEffect, useRef, useState } from 'react';
import { useNavigate, useSearchParams } from '../../lib/router';
import { pubApi, type PublishingApplication, type PublishingContract } from '../../api/publishing';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { APP_LABEL, APP_TONE, CONTRACT_LABEL, CONTRACT_TONE, daysLeft, termLabel } from '../../lib/publishing';
import { localStamp, niceDate } from '../../lib/format';
import { useToast } from '../Toast';
import { useConfirm } from '../Confirm';
import { Modal } from '../Modal';
import { CheckIcon } from '../Check';
import { Badge, KeyValue, Meter, Segmented } from '../../ui';

import '../../styles/split/Publishing.css';

const RIGHT_NAME: Record<string, string> = { performance: '공연권', mechanical: '복제권', digital: '전송권', sync: '싱크' };
const TERR_NAME: Record<string, string> = { KR: '대한민국', JP: '일본', US: '미국', WORLD: '그 밖의 국가' };

function download(c: PublishingContract) {
  const text = [
    `AUDENIQ 저작권 관리 계약서 ${c.number}`, '',
    `기간: ${c.startDate} ~ ${c.endDate} (${termLabel(c.termMonths)})`,
    `수수료: 징수한 사용료의 ${c.commissionRate}%`,
    `관리 대상 국가: ${c.territories.map(t => TERR_NAME[t] ?? t).join(', ')}`,
    `관리할 권리: ${c.rights.map(r => RIGHT_NAME[r] ?? r).join(', ')}`,
    `대상 곡: ${c.works.map(w => w.title).join(', ')}`,
    c.signedAt ? `서명: ${c.signedAt}` : '서명 전',
  ].join('\n');
  const a = document.createElement('a');
  a.href = URL.createObjectURL(new Blob([text], { type: 'text/plain;charset=utf-8' }));
  a.download = `${c.number}.txt`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

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

function ApplicationBundle({ a }: { a: PublishingApplication }) {
  const nav = useNavigate();
  const fix = a.status === 'needs_fix';
  return (
    <article className={`aq-bundle aq-pbl-contract is-${fix ? 'needs' : 'review'}`} aria-label={`${a.workTitles.join(', ')} 저작권 관리 신청`}>
      <header className="aq-bundle-head">
        <div className="min-0">
          <strong>{a.workTitles.join(', ')}</strong>
          {a.submittedAt && <span>{niceDate(a.submittedAt)} 신청 · {a.number}</span>}
        </div>
        <Badge tone={APP_TONE[a.status]} className="aq-bundle-state">{APP_LABEL[a.status]}</Badge>
      </header>
      <ol className="aq-bundle-docs">
        <li className="is-done">
          <span className="aq-bundle-no" aria-hidden="true"><CheckIcon size={14} /></span>
          <div className="min-0"><strong>저작권 관리 신청서·전자계약</strong><span>신청할 때 서명했어요</span></div>
        </li>
        <li className={fix ? 'is-current' : ''}>
          <span className="aq-bundle-no" aria-hidden="true">2</span>
          <div className="min-0"><strong>담당자 심사</strong><span>{fix ? (a.fixReason || '보완할 내용을 확인해 주세요') : '권리 정보를 확인하고 있어요'}</span></div>
          <button type="button" className={fix ? 'ui-btn is-primary aq-bundle-sign' : 'ui-btn is-soft is-sm'} onClick={() => nav(`/works/${a.workIds[0]}?tab=register`)}>
            {fix ? '보완하기' : '진행 보기'}
          </button>
        </li>
      </ol>
    </article>
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
        <div><dt>계약 기간</dt><dd>{c.startDate} ~ {c.endDate}</dd></div>
        <div><dt>남은 기간</dt><dd>{ended ? '종료' : `${Math.max(0, left)}일`}</dd></div>
        <div><dt>수수료</dt><dd>{c.commissionRate}%</dd></div>
        <div><dt>관리 범위</dt><dd>{c.territories.map(t => TERR_NAME[t] ?? t).join(', ')} · {c.rights.map(r => RIGHT_NAME[r] ?? r).join(', ')}</dd></div>
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

/** 계약서 목록 안의 저작권 관리 계약 카드 — 배급 계약 묶음과 같은 목록에 섞여 들어간다. 서류만 본다 */
/** all: 퍼블리싱 메뉴의 '계약' 탭 — 발매한 곡의 계약까지 모두 */
export function PublishingContracts({ all = false }: { all?: boolean }) {
  const [params] = useSearchParams();
  const focus = params.get('pub');
  const { data: contracts } = useAsync(() => pubApi.listContracts(), [], { key: 'pub-contracts' });
  const { data: apps } = useAsync(() => pubApi.listApplications(), [], { key: 'pub-applications' });
  const { data: works } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  // 발매 곡의 신청은 위 발매 계약 묶음에 이미 보인다 — 여기서는 발매 전 곡의 신청만
  const releasedWork = new Set((works ?? []).filter(w => w.recordings.length).map(w => w.id));
  const focused = useRef<HTMLElement | null>(null);
  useEffect(() => { if (focus && contracts) focused.current?.scrollIntoView({ block: 'center', behavior: 'smooth' }); }, [focus, contracts]);

  // 발매한 곡의 서명 끝난 계약은 그 발매 계약 묶음의 '저작권 관리 계약' 줄에 있다 — 같은 계약을 두 번 보이지 않게.
  // 서명할 계약, 발매 전 곡이 든 계약, 그 줄에서 '계약 보기'로 찾아온 계약만 따로 보여 준다
  const list = (contracts ?? []).filter(c => all || c.status === 'pending_sign' || c.id === focus || c.works.some(w => !releasedWork.has(w.id)));
  const pending = (apps ?? []).filter(a => ['submitted', 'in_review', 'needs_fix'].includes(a.status) && (all || !a.workIds.every(id => releasedWork.has(id))));
  if (!list.length && !pending.length) return null;
  const app = (c: PublishingContract) => (apps ?? []).find(a => a.contractId === c.id);

  return (
    <>
      {pending.map(a => <ApplicationBundle key={a.id} a={a} />)}
      {list.map(c => {
        const signed = c.status !== 'pending_sign';
        const a = app(c);
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
            <KeyValue cols={2} className="aq-pbl-contract-summary" items={[
              ['계약 기간', `${c.startDate} ~ ${c.endDate}`],
              ['관리 수수료', `${c.commissionRate}%`],
            ]} />
            <ol className="aq-bundle-docs">
              <li className="is-done">
                <span className="aq-bundle-no" aria-hidden="true"><CheckIcon size={14} /></span>
                <div className="min-0"><strong>저작권 관리 신청서</strong><span>{a?.submittedAt ? `${niceDate(a.submittedAt)} 신청할 때 서명했어요` : '신청할 때 서명했어요'}</span></div>
              </li>
              <li className={signed ? 'is-done' : 'is-current'}>
                <span className="aq-bundle-no" aria-hidden="true">{signed ? <CheckIcon size={14} /> : 2}</span>
                <div className="min-0"><strong>저작권 관리 계약서</strong><span>{signed ? '서명 완료' : '서명 필요'}</span></div>
                <button type="button" className="ui-btn is-soft is-sm" onClick={() => download(c)}>계약서 받기</button>
              </li>
            </ol>
          </article>
        );
      })}
    </>
  );
}
