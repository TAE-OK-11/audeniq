// 곡 화면 '저작권·계약' 탭의 카드들 — 한 카드에 몰아 두지 않고 할 말마다 한 장씩:
// 모든 카드가 같은 틀: 작은 제목 → 한 문장 → 이름·값 줄(또는 누르는 줄) → 맨 아래 작은 글자 동작.
//  · RightsInfoCard: 저작권 등록 — 어디에 등록됐는지 · ISWC · 곡 코드 · 단체 작품 코드 · 등록일 · 내 IPI
//  · RoyaltyInCard: 들어온 저작권료 — 큰 숫자 하나 (누르면 성과 탭)
//  · SocietiesCard: 관리 단체 — 몇 곳에서 받는지 · 단체마다 상태와 받은 금액
//  · ApplicationCard: 저작권 관리 신청 — 상태 한 문장 · 번호 · 제출일 · 신청서 · 철회
//  · ContractCard: 계약 — 언제까지 맡겼는지 한 문장, 남은 날, 나라·권리·연장·계약서 줄, 맨 아래 계약 끝내기
import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { Link } from '../../lib/router';
import { pubApi, type ApplicationStatus, type PublishingContract, type Royalty, type Work } from '../../api/publishing';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { REG_SENTENCE, daysLeft, rightName, societyName, termLabel, territoryName } from '../../lib/publishing';
import { localStamp } from '../../lib/format';
import { Money } from '../Money';
import { Glyph } from '../Glyph';
import { Modal } from '../Modal';
import { useToast } from '../Toast';
import { useConfirm } from '../Confirm';
import { RenewModal } from './PublishingContracts';
import { Meter } from '../../ui';
import '../../styles/split/Upload.css';
import '../../styles/split/Publishing.css';

const Empty = ({ children }: { children: string }) => <span className="aq-empty-val">{children}</span>;
const day = (at?: string | null) => (at ? localStamp(at).replace(/ 오[전후].*$/, '') : '');

/** 이름 왼쪽 · 값 오른쪽 한 줄 — 카드 안 정보는 모두 이 모양 (표처럼 두 칸으로 나누지 않는다) */
function Row({ k, v }: { k: string; v: ReactNode }) {
  return <li className="aq-pbl-kv"><span>{k}</span><b>{v}</b></li>;
}

/** 저작권 등록 — 어디에 등록됐는지 한 문장, 번호들은 줄로 */
export function RightsInfoCard({ w }: { w: Work }) {
  const done = [...w.registrationLog].reverse().find(l => l.status === 'registered');
  const where = [...w.registrationLog].reverse().find(l => l.society)?.society;
  const me = w.contributors.find(c => c.isMe);
  const title = w.registration === 'registered' ? (where ? `${societyName(where)}에 등록됐어요` : '관리 단체에 등록됐어요')
    : w.registration === 'none' ? '아직 관리 단체에 등록하지 않았어요' : '관리 단체에 등록하고 있어요';
  return (
    <section className="aq-relinfo-card aq-pbl-ccard" aria-labelledby="rcInfo">
      <p className="aq-pbl-ccard-cap" id="rcInfo">저작권 등록</p>
      <h2 className="aq-pbl-ccard-title">{title}</h2>
      {w.registration !== 'registered' && w.registration !== 'none' && <p className="aq-pbl-ccard-lead">{REG_SENTENCE[w.registration]}</p>}
      <ul className="aq-pbl-kvs">
        <Row k="ISWC" v={w.iswc ?? <Empty>발급 전</Empty>} />
        <Row k="AUDENIQ 곡 코드" v={w.internalCode} />
        {w.societyCode && <Row k="관리 단체 작품 코드" v={w.societyCode} />}
        {done && <Row k="등록일" v={day(done.at)} />}
        {me && <Row k="내 IPI" v={me.ipi ?? <Empty>입력 전</Empty>} />}
      </ul>
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

/** 맡긴 관리 단체 — 몇 곳에서 받고 있는지 한 문장, 단체마다 한 줄(상태 · 받은 금액) */
export function SocietiesCard({ w, royalties }: { w: Work; royalties: readonly Royalty[] }) {
  const rows = useMemo(() => {
    const m = new Map<string, { done: boolean; got: number }>();
    for (const l of w.registrationLog) {
      if (!l.society) continue;
      const cur = m.get(l.society) ?? { done: false, got: 0 };
      cur.done ||= l.status === 'registered';
      m.set(l.society, cur);
    }
    for (const r of royalties) {
      if (r.status === 'estimated') continue;
      const cur = m.get(r.source) ?? { done: true, got: 0 };
      cur.got += r.net;
      m.set(r.source, cur);
    }
    return [...m.entries()].sort((a, b) => b[1].got - a[1].got);
  }, [w.registrationLog, royalties]);
  const paying = rows.filter(([, v]) => v.got > 0).length;
  return (
    <section className="aq-relinfo-card aq-pbl-ccard" aria-labelledby="rcSoc">
      <p className="aq-pbl-ccard-cap" id="rcSoc">관리 단체</p>
      <h2 className="aq-pbl-ccard-title">{!rows.length ? '아직 맡긴 관리 단체가 없어요' : paying ? `${paying}곳에서 저작권료를 받고 있어요` : `${rows.length}곳에 맡겼어요`}</h2>
      {!rows.length && <p className="aq-pbl-ccard-lead">등록이 끝나면 어느 단체에 맡겼는지와 단체별로 받은 저작권료가 여기에 보여요.</p>}
      {rows.length > 0 && (
        <ul className="aq-pbl-kvs">
          {rows.map(([s, v]) => (
            <li key={s} className="aq-pbl-kv">
              <span>{societyName(s)}<small>{v.done ? '등록 완료' : '등록 진행 중'}</small></span>
              <b>{v.got > 0 ? <Money value={v.got} /> : <Empty>아직 없음</Empty>}</b>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

const APP_TITLE: Record<ApplicationStatus, string> = {
  draft: '신청서를 쓰고 있어요', submitted: '신청서를 보냈어요', in_review: '담당자가 신청을 보고 있어요', needs_fix: '고칠 곳이 있어요',
  approved: '신청이 승인됐어요', rejected: '신청이 반려됐어요', withdrawn: '신청을 철회했어요',
};

/** 저작권 관리 신청 — 상태 한 문장, 번호·제출일 줄, 신청서 보기. 철회는 맨 아래 작은 글자 */
export function ApplicationCard({ app, submittedAt, signTo, canWithdraw, onWithdraw, applyTo }: {
  app: { id: string; number: string; status: ApplicationStatus } | null; submittedAt?: string | null;
  /** 승인돼 계약서 서명이 남았으면 그 계약서 */
  signTo?: string | null; canWithdraw: boolean; onWithdraw: () => void; applyTo?: string | null;
}) {
  if (!app || app.status === 'withdrawn') {
    return (
      <section className="aq-relinfo-card aq-pbl-ccard" aria-labelledby="rcApp">
        <p className="aq-pbl-ccard-cap" id="rcApp">저작권 관리 신청</p>
        <h2 className="aq-pbl-ccard-title">{app ? '신청을 철회했어요' : '아직 신청하지 않았어요'}</h2>
        <p className="aq-pbl-ccard-lead">맡기지 않아도 곡 정보는 계속 관리할 수 있어요. 맡기면 방송·스트리밍 저작권료까지 받아 드려요.</p>
        {applyTo && (
          <ul className="aq-pbl-kvs">
            <li><Link to={applyTo} className="aq-pbl-kv is-link"><span>저작권 관리 신청하기</span><b className="aq-pbl-ccard-go">시작<Glyph name="chevron-right" size={15} /></b></Link></li>
          </ul>
        )}
      </section>
    );
  }
  return (
    <section className="aq-relinfo-card aq-pbl-ccard" aria-labelledby="rcApp">
      <p className="aq-pbl-ccard-cap" id="rcApp">저작권 관리 신청</p>
      <h2 className="aq-pbl-ccard-title">{APP_TITLE[app.status]}</h2>
      {signTo && <p className="aq-pbl-ccard-lead">계약서에 서명하면 계약이 맺어지고 관리 단체 등록이 시작돼요.</p>}
      <ul className="aq-pbl-kvs">
        {signTo && <li><Link to={signTo} className="aq-pbl-kv is-link is-strong"><span>계약서 확인하고 서명</span><b className="aq-pbl-ccard-go">서명<Glyph name="chevron-right" size={15} /></b></Link></li>}
        <Row k="신청 번호" v={app.number} />
        {submittedAt && <Row k="제출" v={day(submittedAt)} />}
        <li><Link to={`/publishing/applications/${encodeURIComponent(app.id)}`} className="aq-pbl-kv is-link"><span>신청서</span><b className="aq-pbl-ccard-go">보기<Glyph name="chevron-right" size={15} /></b></Link></li>
      </ul>
      {canWithdraw && <button type="button" className="ui-btn is-text is-sm aq-pbl-end" onClick={onWithdraw}>신청 철회</button>}
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
    try { onDone(await pubApi.requestScope(c.id, terr, rights)); toast('바꿔 달라고 요청했어요. 담당자가 확인하면 바뀌어요.', 'success'); onClose(); }
    catch (e) { toast(errorMessage(e), 'error'); } finally { setBusy(false); }
  };
  return (
    <Modal title="관리하는 나라·권리 바꾸기" onClose={onClose}>
      <div className="aq-pbl-scope">
        <p className="ui-field-help">맡길 나라와 권리를 골라 주세요. 수수료({c.commissionRate}%)와 계약 기간은 그대로예요. 담당자가 관리 단체에 맞춘 뒤 바뀌어요.</p>
        {policy && <>
          <h3 className="aq-track-sheet-h">관리하는 나라</h3>
          <div className="ui-chips is-wrap" role="group" aria-label="관리 국가">
            {policy.territories.map(t => <button key={t.code} type="button" className="ui-chip" aria-pressed={terr.includes(t.code)} onClick={() => flip(terr, setTerr, t.code)}>{t.label}</button>)}
          </div>
          <h3 className="aq-track-sheet-h">관리하는 권리</h3>
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
  renewed: '계약이 연장됐어요.', renewal_rejected: '연장 요청을 처리하지 못했어요.', ended: '계약이 끝났어요.',
  termination_rejected: '계약 끝내기 요청을 처리하지 못했어요.', scope_changed: '관리하는 나라·권리가 바뀌었어요.', scope_rejected: '나라·권리 변경 요청을 처리하지 못했어요.',
};

/** "2027-10-31" → "2027년 10월 31일" */
const longDay = (d: string) => (/^\d{4}-\d{2}-\d{2}$/.test(d) ? `${Number(d.slice(0, 4))}년 ${Number(d.slice(5, 7))}월 ${Number(d.slice(8, 10))}일` : d);
/** "대한민국 외 3곳" — 줄에는 짧게, 누르면 전부 */
const more = (list: string[], unit: string) => (list.length > 1 ? `${list[0]} 외 ${list.length - 1}${unit}` : list[0] ?? '');

/** 계약 — 토스처럼 한 문장으로 먼저 말하고(언제까지 무엇을 맡겼는지), 할 수 있는 일은 설명이 붙은 줄로.
 *  번호·시작일 같은 서류 정보는 계약서 줄 안에, 계약 끝내기는 맨 아래 작은 글자로 */
export function ContractCard({ contract, canEdit }: { contract: PublishingContract; canEdit: boolean }) {
  const toast = useToast();
  const confirm = useConfirm();
  const [c, setC] = useState(contract);
  const [modal, setModal] = useState<'renew' | 'scope' | null>(null);
  useEffect(() => setC(contract), [contract]);
  const ended = c.status === 'ended';
  const left = Math.max(0, daysLeft(c.endDate));
  const total = Math.max(1, daysLeft(c.endDate, new Date(`${c.startDate}T00:00:00+09:00`)));
  const soon = !ended && left <= 45;
  const open = canEdit && !ended && !c.terminationRequestedAt;
  const terminate = async () => {
    const ok = await confirm({
      title: '저작권 관리 계약을 끝낼까요?',
      message: '담당자가 관리 단체 절차를 확인한 뒤 끝내요. 끝나면 새 저작권료를 받아 드릴 수 없고, 그동안의 기록은 남아요.',
      confirmLabel: '끝내기 요청', danger: true,
    });
    if (!ok) return;
    try { setC(await pubApi.requestTermination(c.id)); toast('계약을 끝내 달라고 요청했어요.', 'success'); }
    catch (e) { toast(errorMessage(e), 'error'); }
  };
  const terr = c.territories.map(territoryName);
  const rights = c.rights.map(rightName);
  // 지금 기다리는 요청 — 한 줄로만
  const waiting = c.terminationRequestedAt ? '계약을 끝내 달라고 요청했어요. 담당자가 확인하고 있어요.'
    : c.renewal ? `${termLabel(c.renewal.months)} 연장을 요청했어요. 담당자가 확인하면 새 날짜로 바뀌어요.`
    : c.scopeRequest ? '관리하는 나라·권리를 바꿔 달라고 요청했어요. 담당자가 확인하고 있어요.' : null;
  return (
    <section className="aq-relinfo-card aq-pbl-ccard" aria-labelledby="rcContract">
      <p className="aq-pbl-ccard-cap" id="rcContract">저작권 관리 계약</p>
      {ended ? (
        <h2 className="aq-pbl-ccard-title">계약이 끝났어요</h2>
      ) : (
        <h2 className="aq-pbl-ccard-title"><b className="aq-nowrap">{longDay(c.endDate)}</b>까지<br />AUDENIQ이 저작권을 관리해요</h2>
      )}
      {!ended && (
        <div className={`aq-pbl-ccard-term${soon ? ' is-soon' : ''}`}>
          <Meter total={total} parts={[{ value: left, tone: soon ? 'amber' : 'primary' }]} label={`계약 기간 ${left}일 남음`} />
          <span>{left}일 남았어요</span>
        </div>
      )}
      <p className="aq-pbl-ccard-lead">저작권료가 들어오면 수수료 {c.commissionRate}%만 빼고 정산 잔액에 넣어 드려요.</p>

      {waiting && <p className="aq-pbl-ccard-wait"><Glyph name="clock" size={15} />{waiting}</p>}
      {!waiting && c.requestDecision && (
        <p className={`aq-pbl-ccard-wait${c.requestDecision.outcome.endsWith('rejected') ? ' is-warn' : ''}`}>
          <Glyph name="info" size={15} />{DECISION[c.requestDecision.outcome]}{c.requestDecision.note && ` ${c.requestDecision.note}`}
        </p>
      )}
      <ul className="ui-list is-inset aq-pbl-ccard-rows">
        {/* 곧 끝나면 연장이 맨 위 줄 — 큰 버튼 대신 강조한 한 줄 (카드 가운데 큰 버튼은 무겁고 균형이 깨졌다) */}
        {open && soon && !c.renewal && (
          <li>
            <button type="button" className="ui-row is-soon" onClick={() => setModal('renew')}>
              <span className="min-0"><span className="ui-row-title">계약 연장</span><span className="ui-row-meta"><span>{left}일 뒤 끝나요 · 지금 늘려 두면 관리가 끊기지 않아요</span></span></span>
              <span className="aq-pbl-ccard-go">연장하기<Glyph name="chevron-right" size={15} /></span>
            </button>
          </li>
        )}
        <li>
          <button type="button" className="ui-row" disabled={!open || !!c.scopeRequest} onClick={() => setModal('scope')}>
            <span className="min-0"><span className="ui-row-title">관리하는 나라</span><span className="ui-row-meta"><span>{more(terr, '곳')}</span></span></span>
            {open && !c.scopeRequest && <span className="aq-pbl-ccard-go">바꾸기<Glyph name="chevron-right" size={15} /></span>}
          </button>
        </li>
        <li>
          <button type="button" className="ui-row" disabled={!open || !!c.scopeRequest} onClick={() => setModal('scope')}>
            <span className="min-0"><span className="ui-row-title">관리하는 권리</span><span className="ui-row-meta"><span>{more(rights, '개')}</span></span></span>
            {open && !c.scopeRequest && <span className="aq-pbl-ccard-go">바꾸기<Glyph name="chevron-right" size={15} /></span>}
          </button>
        </li>
        {open && !soon && !c.renewal && (
          <li>
            <button type="button" className="ui-row" onClick={() => setModal('renew')}>
              <span className="min-0"><span className="ui-row-title">계약 연장</span><span className="ui-row-meta"><span>끝나기 전에 미리 늘려 둘 수 있어요</span></span></span>
              <span className="aq-pbl-ccard-go">연장하기<Glyph name="chevron-right" size={15} /></span>
            </button>
          </li>
        )}
        <li>
          <Link to={`/contracts/publishing/${encodeURIComponent(c.id)}`} className="ui-row">
            <span className="min-0"><span className="ui-row-title">계약서</span><span className="ui-row-meta"><span>{c.number}</span>{c.startDate && <span>{longDay(c.startDate)} 시작</span>}</span></span>
            <span className="aq-pbl-ccard-go">보기<Glyph name="chevron-right" size={15} /></span>
          </Link>
        </li>
      </ul>
      {open && <button type="button" className="ui-btn is-text is-sm aq-pbl-end" onClick={() => void terminate()}>계약 끝내기</button>}
      {modal === 'renew' && <RenewModal c={c} onClose={() => setModal(null)} onDone={setC} />}
      {modal === 'scope' && <ScopeModal c={c} onClose={() => setModal(null)} onDone={setC} />}
    </section>
  );
}
