// 곡 화면 '저작권·계약' 탭의 카드들 — 한 카드에 몰아 두지 않고 할 말마다 한 장씩:
//  · RightsInfoCard: 저작권 등록 정보 (상태 · ISWC · 곡 코드 · 단체 작품 코드 · 등록일 · 내 IPI)
//  · RoyaltyInCard: 들어온 저작권료 — 큰 숫자 하나 (누르면 성과 탭)
//  · SocietiesCard: 위임된 관리 단체 — 단체마다 등록 상태와 받은 금액
//  · ContractCard: 계약 — 기간·남은 날·조건, 관리 국가·권리, 연장 · 관리 범위 바꾸기 · 계약서 · 종료 요청
import { useEffect, useMemo, useState } from 'react';
import { Link } from '../../lib/router';
import { pubApi, type PublishingContract, type Royalty, type Work } from '../../api/publishing';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { REG_LABEL, REG_SENTENCE, daysLeft, rightName, societyName, termLabel, territoryName } from '../../lib/publishing';
import { localStamp } from '../../lib/format';
import { Money } from '../Money';
import { Glyph } from '../Glyph';
import { Modal } from '../Modal';
import { useToast } from '../Toast';
import { useConfirm } from '../Confirm';
import { RegBadge } from './parts';
import { RenewModal } from './PublishingContracts';
import { Badge, KeyValue, Meter } from '../../ui';
import '../../styles/split/Upload.css';
import '../../styles/split/Publishing.css';

const Empty = ({ children }: { children: string }) => <span className="aq-empty-val">{children}</span>;
const day = (at?: string | null) => (at ? localStamp(at).replace(/ 오[전후].*$/, '') : '');

export function RightsInfoCard({ w }: { w: Work }) {
  const done = [...w.registrationLog].reverse().find(l => l.status === 'registered');
  const me = w.contributors.find(c => c.isMe);
  return (
    <section className="aq-relinfo-card aq-pbl-rcard" aria-labelledby="rcInfo">
      <div className="ui-section-head"><h2 className="subhead" id="rcInfo">저작권 등록 정보</h2><RegBadge status={w.registration} /></div>
      {w.registration !== 'registered' && <p className="ui-field-help">{REG_SENTENCE[w.registration]}</p>}
      <KeyValue cols={2} items={[
        ['ISWC', w.iswc ?? <Empty>발급 전</Empty>],
        ['AUDENIQ 곡 코드', w.internalCode],
        w.societyCode ? ['관리 단체 작품 코드', w.societyCode] : null,
        ['등록일', done ? day(done.at) : <Empty>{REG_LABEL[w.registration]}</Empty>],
        me ? ['내 IPI', me.ipi ?? <Empty>입력 전</Empty>] : null,
        me?.society ? ['내 관리 단체', societyName(me.society)] : null,
      ]} />
    </section>
  );
}

/** 들어온 저작권료 — 확정·지급만 큰 숫자로 */
export function RoyaltyInCard({ royalties, to }: { royalties: readonly Royalty[]; to: string }) {
  const got = royalties.filter(r => r.status !== 'estimated').reduce((n, r) => n + r.net, 0);
  if (!got) return null;
  return (
    <Link to={to} className="ui-card aq-pbl-income" aria-label="들어온 저작권료 · 성과 보기">
      <span className="aq-pbl-income-cap">들어온 저작권료</span>
      <strong className="aq-pbl-income-num"><Money value={got} /></strong>
      <Glyph name="chevron-right" size={18} className="aq-pbl-income-go" />
    </Link>
  );
}

/** 위임된 관리 단체 — 등록 기록·내 소속·저작권료를 보낸 단체를 모아 단체마다 한 줄 */
export function SocietiesCard({ w, royalties }: { w: Work; royalties: readonly Royalty[] }) {
  const rows = useMemo(() => {
    const m = new Map<string, { state: string; at?: string; got: number }>();
    for (const l of w.registrationLog) {
      if (!l.society) continue;
      const cur = m.get(l.society) ?? { state: '', got: 0 };
      cur.state = l.status === 'registered' ? '등록 완료' : '등록 진행 중';
      cur.at = l.at;
      m.set(l.society, cur);
    }
    for (const r of royalties) {
      if (r.status === 'estimated') continue;
      const cur = m.get(r.source) ?? { state: '등록 완료', got: 0 };
      cur.got += r.net;
      m.set(r.source, cur);
    }
    return [...m.entries()];
  }, [w.registrationLog, royalties]);
  return (
    <section className="aq-relinfo-card aq-pbl-rcard" aria-labelledby="rcSoc">
      <h2 className="subhead" id="rcSoc">위임된 관리 단체</h2>
      {rows.length ? (
        <ul className="ui-list is-inset aq-pbl-soc">
          {rows.map(([s, v]) => (
            <li key={s} className="ui-row is-flat">
              <span className="min-0">
                <span className="ui-row-title">{societyName(s)}</span>
                <span className="ui-row-meta"><span>{v.state || '등록 진행 중'}</span>{v.at && <span>{day(v.at)}</span>}</span>
              </span>
              {v.got > 0 ? <Money value={v.got} /> : <span className="muted small">아직 없음</span>}
            </li>
          ))}
        </ul>
      ) : <p className="ui-field-help">관리 단체에 등록되면 어느 단체에 맡겼는지와 단체별로 들어온 저작권료가 여기에 보여요.</p>}
    </section>
  );
}

/** 관리 국가·권리 바꾸기 — 지금 범위에서 골라 바꾸고 담당자가 관리 단체 위임을 맞춘 뒤 적용한다 */
function ScopeModal({ c, onClose, onDone }: { c: PublishingContract; onClose: () => void; onDone: (c: PublishingContract) => void }) {
  const toast = useToast();
  const { data: policy } = useAsync(() => pubApi.policy(), [], { key: 'pub-policy' });
  const [terr, setTerr] = useState<string[]>(c.territories);
  const [rights, setRights] = useState<string[]>(c.rights);
  const [busy, setBusy] = useState(false);
  const flip = (list: string[], set: (v: string[]) => void, v: string) => set(list.includes(v) ? list.filter(x => x !== v) : [...list, v]);
  const same = (a: string[], b: string[]) => a.length === b.length && a.every(x => b.includes(x));
  const unchanged = same(terr, c.territories) && same(rights, c.rights);
  const go = async () => {
    setBusy(true);
    try { onDone(await pubApi.requestScope(c.id, terr, rights)); toast('관리 범위 변경을 요청했어요. 담당자가 관리 단체 위임을 맞춘 뒤 적용해요.', 'success'); onClose(); }
    catch (e) { toast(errorMessage(e), 'error'); } finally { setBusy(false); }
  };
  return (
    <Modal title="관리 범위 바꾸기" onClose={onClose}>
      <div className="aq-pbl-scope">
        <p className="ui-field-help">관리할 국가와 권리를 바꿀 수 있어요. 수수료({c.commissionRate}%)와 계약 기간은 그대로예요.</p>
        {policy && <>
          <h3 className="aq-track-sheet-h">관리 국가</h3>
          <div className="ui-chips is-wrap" role="group" aria-label="관리 국가">
            {policy.territories.map(t => <button key={t.code} type="button" className="ui-chip" aria-pressed={terr.includes(t.code)} onClick={() => flip(terr, setTerr, t.code)}>{t.label}</button>)}
          </div>
          <h3 className="aq-track-sheet-h">관리할 권리</h3>
          <div className="aq-agree-box">
            {policy.rights.map(r => (
              <label key={r.key} className="ui-check is-plain">
                <input type="checkbox" checked={rights.includes(r.key)} onChange={() => flip(rights, setRights, r.key)} />
                <span><strong>{r.label}</strong><br /><small className="muted">{r.desc}</small></span>
              </label>
            ))}
          </div>
        </>}
        <div className="aq-confirm-actions">
          <button type="button" className="ui-btn is-soft" onClick={onClose}>닫기</button>
          <button type="button" className="ui-btn is-primary" disabled={busy || unchanged || !terr.length || !rights.length} onClick={() => void go()}>{busy ? '요청 중' : '변경 요청'}</button>
        </div>
      </div>
    </Modal>
  );
}

const DECISION: Record<NonNullable<PublishingContract['requestDecision']>['outcome'], string> = {
  renewed: '연장 요청이 처리됐어요.', renewal_rejected: '연장 요청이 반려됐어요.', ended: '계약이 종료됐어요.',
  termination_rejected: '종료 요청이 반려됐어요.', scope_changed: '관리 범위 변경이 적용됐어요.', scope_rejected: '관리 범위 변경 요청이 반려됐어요.',
};

export function ContractCard({ contract, canEdit }: { contract: PublishingContract; canEdit: boolean }) {
  const toast = useToast();
  const confirm = useConfirm();
  const [c, setC] = useState(contract);
  const [modal, setModal] = useState<'renew' | 'scope' | null>(null);
  useEffect(() => setC(contract), [contract]);
  const ended = c.status === 'ended';
  const left = Math.max(0, daysLeft(c.endDate));
  const total = Math.max(1, daysLeft(c.endDate, new Date(`${c.startDate}T00:00:00+09:00`)));
  const open = canEdit && !ended && !c.terminationRequestedAt;
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
    <section className="aq-relinfo-card aq-pbl-rcard" aria-labelledby="rcContract">
      <div className="ui-section-head"><h2 className="subhead" id="rcContract">계약</h2><Badge tone={ended ? 'gray' : c.status === 'expiring' || c.terminationRequestedAt ? 'amber' : 'green'}>{ended ? '종료' : c.terminationRequestedAt ? '종료 요청' : c.status === 'expiring' ? '곧 끝남' : '관리 중'}</Badge></div>
      {!ended && (
        <div className="aq-pbl-term">
          <p><b>{left}일</b> 남았어요 <span className="muted">· {c.endDate}까지</span></p>
          {/* 남은 기간을 채운 막대 — 첫날 빈 막대가 고장처럼 보였다 */}
          <Meter total={total} parts={[{ value: left, tone: left <= 45 ? 'amber' : 'primary' }]} label={`계약 기간 ${left}일 남음`} />
        </div>
      )}
      <KeyValue cols={2} items={[
        ['계약 번호', c.number],
        ['시작일', c.startDate],
        ['관리 수수료', `받은 저작권료의 ${c.commissionRate}%`],
        ['최초 기간', termLabel(c.termMonths)],
      ]} />
      <div className="aq-pbl-scope-view">
        <div><span>관리 국가</span><p>{c.territories.map(t => <Badge key={t} tone="gray">{territoryName(t)}</Badge>)}</p></div>
        <div><span>관리할 권리</span><p>{c.rights.map(r => <Badge key={r} tone="gray">{rightName(r)}</Badge>)}</p></div>
      </div>
      {/* 담당자를 기다리는 요청과 지난 요청의 답 — 한 줄씩 */}
      {c.renewal && <p className="ui-field-help aq-pbl-req"><Glyph name="clock" size={14} />{termLabel(c.renewal.months)} 연장을 요청했어요 · {day(c.renewal.requestedAt)}</p>}
      {c.scopeRequest && <p className="ui-field-help aq-pbl-req"><Glyph name="clock" size={14} />관리 범위 변경을 요청했어요 — {c.scopeRequest.territories.map(territoryName).join(', ')} · {c.scopeRequest.rights.map(rightName).join(', ')}</p>}
      {c.terminationRequestedAt && <p className="ui-field-help aq-pbl-req is-warn"><Glyph name="clock" size={14} />종료를 요청했어요 · {day(c.terminationRequestedAt)}</p>}
      {!c.renewal && !c.scopeRequest && !c.terminationRequestedAt && c.requestDecision && (
        <p className={`ui-field-help${c.requestDecision.outcome.endsWith('rejected') ? ' is-warn' : ''}`}>
          {DECISION[c.requestDecision.outcome]}{c.requestDecision.note && ` — ${c.requestDecision.note}`}
        </p>
      )}
      <div className="aq-pbl-contract-btns">
        {open && !c.renewal && <button type="button" className={`ui-btn ${c.status === 'expiring' ? 'is-primary' : 'is-soft'}`} onClick={() => setModal('renew')}>계약 연장</button>}
        {open && !c.scopeRequest && <button type="button" className="ui-btn is-soft" onClick={() => setModal('scope')}>관리 범위 바꾸기</button>}
        <Link to={`/contracts/publishing/${encodeURIComponent(c.id)}`} className="ui-btn is-soft">계약서 보기</Link>
      </div>
      {open && <button type="button" className="ui-btn is-text is-sm aq-danger-text aq-pbl-end" onClick={() => void terminate()}>계약 종료 요청</button>}
      {modal === 'renew' && <RenewModal c={c} onClose={() => setModal(null)} onDone={setC} />}
      {modal === 'scope' && <ScopeModal c={c} onClose={() => setModal(null)} onDone={setC} />}
    </section>
  );
}
