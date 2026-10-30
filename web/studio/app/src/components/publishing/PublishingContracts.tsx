// 계약서 화면의 퍼블리싱 계약 — 계약에 든 작품, 기간, 관리 범위, 갱신(연장)·종료 요청·내려받기 (기획안 6.4).
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
import { Badge, Meter, Segmented } from '../../ui';

const RIGHT_NAME: Record<string, string> = { performance: '공연권', mechanical: '복제권', digital: '전송권', sync: '싱크' };
const TERR_NAME: Record<string, string> = { KR: '대한민국', JP: '일본', US: '미국', WORLD: '그 밖의 국가' };

function download(c: PublishingContract) {
  const text = [
    `AUDENIQ 퍼블리싱 관리 계약서 ${c.number}`, '',
    `기간: ${c.startDate} ~ ${c.endDate} (${termLabel(c.termMonths)})`,
    `수수료: 징수한 사용료의 ${c.commissionRate}%`,
    `관리 대상 국가: ${c.territories.map(t => TERR_NAME[t] ?? t).join(', ')}`,
    `관리할 권리: ${c.rights.map(r => RIGHT_NAME[r] ?? r).join(', ')}`,
    `대상 작품: ${c.works.map(w => w.title).join(', ')}`,
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
    <article className={`aq-bundle is-${fix ? 'needs' : 'review'}`} aria-label={`${a.workTitles.join(', ')} 퍼블리싱 신청`}>
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
          <div className="min-0"><strong>퍼블리싱 신청서·전자계약</strong><span>신청할 때 서명했어요</span></div>
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

/** 계약서 목록 안의 퍼블리싱 계약 카드 — 배급 계약 묶음과 같은 목록(aq-bundle-list)에 섞여 들어간다 */
export function PublishingContracts() {
  const toast = useToast();
  const confirm = useConfirm();
  const [params] = useSearchParams();
  const focus = params.get('pub');
  const { data: contracts, reload } = useAsync(() => pubApi.listContracts(), [], { key: 'pub-contracts' });
  const { data: apps } = useAsync(() => pubApi.listApplications(), [], { key: 'pub-applications' });
  const { data: works } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  // 발매 곡의 신청은 위 발매 계약 묶음에 이미 보인다 — 여기서는 발매 전 작품의 신청만
  const releasedWork = new Set((works ?? []).filter(w => w.recordings.length).map(w => w.id));
  const [local, setLocal] = useState<Record<string, PublishingContract>>({});
  const [renewing, setRenewing] = useState<PublishingContract | null>(null);
  const focused = useRef<HTMLElement | null>(null);
  useEffect(() => { if (focus && contracts) focused.current?.scrollIntoView({ block: 'center', behavior: 'smooth' }); }, [focus, contracts]);

  const list = (contracts ?? []).map(c => local[c.id] ?? c);
  const pending = (apps ?? []).filter(a => ['submitted', 'in_review', 'needs_fix'].includes(a.status) && !a.workIds.every(id => releasedWork.has(id)));
  if (!list.length && !pending.length) return null;

  const terminate = async (c: PublishingContract) => {
    const ok = await confirm({
      title: '퍼블리싱 계약 종료를 요청할까요?',
      message: '담당자가 관리 단체 절차를 확인한 뒤 종료해요. 종료 후에는 새 작품을 관리에 넣을 수 없고, 그동안의 등록·정산 기록은 남아요.',
      confirmLabel: '종료 요청', danger: true,
    });
    if (!ok) return;
    try { const n = await pubApi.requestTermination(c.id); setLocal(x => ({ ...x, [c.id]: n })); toast('종료를 요청했어요.', 'success'); }
    catch (e) { toast(errorMessage(e), 'error'); }
  };

  return (
    <>
        {pending.map(a => <ApplicationBundle key={a.id} a={a} />)}
        {list.map(c => {
          const left = daysLeft(c.endDate);
          const ended = c.status === 'ended';
          const total = Math.max(1, daysLeft(c.endDate, new Date(`${c.startDate}T00:00:00+09:00`)));
          return (
            <article key={c.id} ref={c.id === focus ? el => { focused.current = el; } : undefined}
              className={`aq-bundle is-${c.status === 'pending_sign' ? 'to-sign' : ended ? 'signed' : 'signed'}`} aria-label={`퍼블리싱 관리 계약 ${c.number}`}>
              <header className="aq-bundle-head">
                <div className="min-0">
                  <strong>{c.works.map(w => w.title).join(', ')}</strong>
                  <span>퍼블리싱 관리 계약 · {c.number}</span>
                </div>
                <Badge tone={CONTRACT_TONE[c.status]} className="aq-bundle-state">{CONTRACT_LABEL[c.status]}</Badge>
              </header>
              <ol className="aq-bundle-docs">
                <li className="is-done">
                  <span className="aq-bundle-no" aria-hidden="true"><CheckIcon size={14} /></span>
                  <div className="min-0">
                    <strong>계약서 · 수수료 {c.commissionRate}%</strong>
                    <span>{c.territories.map(t => TERR_NAME[t] ?? t).join(', ')} · {c.rights.map(r => RIGHT_NAME[r] ?? r).join(', ')}</span>
                  </div>
                  <button type="button" className="ui-btn is-soft is-sm" onClick={() => download(c)}>계약서 받기</button>
                </li>
                <li className={c.status === 'expiring' && !c.renewal ? 'is-current' : c.renewal ? 'is-done' : ''}>
                  <span className="aq-bundle-no" aria-hidden="true">{c.renewal ? <CheckIcon size={14} /> : 2}</span>
                  <div className="min-0">
                    <strong>{c.startDate} ~ {c.endDate}</strong>
                    <span>{ended ? '계약이 끝났어요' : c.terminationRequestedAt ? `종료 요청 · ${localStamp(c.terminationRequestedAt)}`
                      : c.renewal ? `${termLabel(c.renewal.months)} 연장 요청함 · ${localStamp(c.renewal.requestedAt)}` : `${Math.max(0, left)}일 남았어요`}</span>
                    {!ended && <Meter total={total} parts={[{ value: total - Math.max(0, left), tone: left <= 45 ? 'amber' : 'primary' }]} label={`계약 기간 ${Math.max(0, left)}일 남음`} />}
                  </div>
                  {!ended && !c.terminationRequestedAt && !c.renewal && (
                    <button type="button" className={c.status === 'expiring' ? 'ui-btn is-primary aq-bundle-sign' : 'ui-btn is-soft is-sm'} onClick={() => setRenewing(c)}>연장하기</button>
                  )}
                </li>
              </ol>
              {!ended && !c.terminationRequestedAt && (
                <div className="row-actions" style={{ padding: "4px 14px 6px" }}>
                  <button type="button" className="ui-btn is-text is-sm aq-danger-text" onClick={() => void terminate(c)}>계약 종료 요청</button>
                </div>
              )}
            </article>
          );
        })}
      {renewing && <RenewModal c={renewing} onClose={() => setRenewing(null)} onDone={n => { setLocal(x => ({ ...x, [n.id]: n })); reload(); }} />}
    </>
  );
}
