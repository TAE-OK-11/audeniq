// 정산·지급 — hero + 탭 구조의 단순화된 레이아웃
import { Link, useSearchParams } from '../lib/router';
import { pubApi } from '../api/publishing';
import { useAsync } from '../hooks/useAsync';
import { ROYALTY_KIND_LABEL, pct } from '../lib/publishing';
import { SplitsReceived } from '../components/SplitsReceived';
import { useEffect, useState } from 'react';
import { Modal, useModalClose } from '../components/Modal';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { BankLogo } from '../components/BankLogo';
import { PaymentSetupModal } from '../components/PaymentSetupModal';
import { isPaymentRegistered, usePayment } from '../store/payment';
import { balance, financeListsLoaded, revenueTypeOf, payoutFee, payoutsStore, statementsNextStore, statementsStore, useBalance, type Payout, type Statement } from '../store/settlement';
import { SkeletonRows } from '../components/Skeleton';
import { MOCK } from '../lib/mode';
import * as portal from '../api/portal';
import { errorMessage } from '../api/errors';
import { refreshFinance } from '../store/portalSync';

const PAYOUT_STATUS: Record<Payout['status'], [string, Tone]> = {
  recorded: ['전송 전', 'blue'],
  requested: ['요청 접수', 'violet'],
  processing: ['지급 처리 중', 'violet'],
  sent: ['지급 완료', 'green'],
  failed: ['지급 실패', 'amber'],
  cancelled: ['요청 취소', 'gray'],
};

/** 지급이 지금 몇 번째 단계인지 — 요청 접수 → 처리 중 → 입금 완료 (실패·취소는 단계 밖) */
const PAYOUT_STEPS = ['요청 접수', '처리 중', '입금 완료'] as const;
const payoutStep = (st: Payout['status']) => (st === 'requested' ? 0 : st === 'processing' ? 1 : st === 'sent' ? 2 : -1);

import { money, niceDate } from '../lib/format';
import { todayStr } from '../lib/date';
import { uid } from '../lib/store';
import { CheckIcon } from '../components/Check';
import { Glyph } from '../components/Glyph';
import { Money } from '../components/Money';
import { Segmented } from '../components/Segmented';
import { Badge, ChipGroup, type Tone } from '../ui';

/** 정산은 플랫폼별·월별로 나뉘어 들어오지 않는다 — 운영팀이 정산금(크레딧)을 넣으면 잔액에 쌓이고, 아티스트가 원할 때 지급을 요청한다.
 *  그래서 내역은 통장처럼 날짜별 적립·차감 줄로 보여 준다 */
const entryTitle = (s: Statement) => s.note || (s.amount < 0 ? '정산금 차감' : '정산금 적립');
const entryKind = (s: Statement) => (s.amount < 0 ? '차감' : revenueTypeOf(s) === 'publishing' ? '저작권료 적립' : '적립');
/** 'YYYY-MM-DD' → '9월 29일' (올해가 아니면 연도까지) */
function dayLabel(d: string, now = new Date()): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(d);
  if (!m) return d || '날짜 없음';
  const md = `${+m[2]}월 ${+m[3]}일`;
  return +m[1] === now.getFullYear() ? md : `${m[1]}년 ${md}`;
}
/** +50,000원 / −50,000원 — 들어온 돈과 빠진 돈을 부호로 */
function SignedMoney({ value }: { value: number }) {
  return <span className={`settle-entry-amount${value < 0 ? ' is-minus' : ' is-plus'}`}>{value < 0 ? '−' : '+'}<Money value={Math.abs(value)} /></span>;
}

/** 체험 모드: 운영팀이 정산금을 넣는 상황을 흉내 낸다 */
function StatementForm({ onSave }: { onSave: (s: { amount: number; note: string }) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [amount, setAmount] = useState('');
  const [note, setNote] = useState('');
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const amt = Number(amount);
    if (!Number.isInteger(amt) || amt <= 0) { toast('금액을 1원 이상 입력해 주세요.'); return; }
    onSave({ amount: amt, note: note.trim() });
    close();
  };
  return (
    <form onSubmit={submit}>
      <p className="small muted">실제로는 운영팀이 정산금을 넣어 드려요. 체험에서는 직접 넣어 지급 요청까지 해 볼 수 있어요.</p>
      <div className="ui-field">
        <label htmlFor="stAmount">금액 (원)</label>
        <span className="aq-affix" data-unit="원">
          <input id="stAmount" inputMode="numeric" required placeholder="예: 50,000"
            value={amount ? Number(amount).toLocaleString('ko-KR') : ''} onChange={e => setAmount(e.target.value.replace(/\D/g, '').replace(/^0+/, '').slice(0, 15))} />
        </span>
      </div>
      <div className="ui-field">
        <label htmlFor="stNote">내용<span className="aq-optional">선택</span></label>
        <input id="stNote" maxLength={200} value={note} onChange={e => setNote(e.target.value)} placeholder="예: 9월 정산금" />
      </div>
      <div className="aq-sticky-foot">
        <button className="ui-btn is-primary studio-submit-wide" type="submit">정산금 넣기</button>
      </div>
    </form>
  );
}

/** 체험 모드: 기록한 정산 내역을 그대로 CSV로 (실서버는 서버가 전체 기간을 만든다) */
function saveStatementsCsv(rows: Statement[]) {
  const cell = (v: string | number) => {
    const t = String(v ?? '');
    const safe = /^[=+\-@]/.test(t) && typeof v === 'string' ? `'${t}` : t;
    return `"${safe.replace(/"/g, '""')}"`;
  };
  const csv = '\uFEFF' + ['period,platform,amount,note,created', ...rows.map(r => [r.period, r.platform, r.amount, r.note, r.created].map(cell).join(','))].join('\r\n');
  const url = URL.createObjectURL(new Blob([csv], { type: 'text/csv;charset=utf-8' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = 'statements.csv';
  document.body.appendChild(a);
  a.click();
  a.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export function Settlement() {
  const toast = useToast();
  const confirm = useConfirm();
  const payment = usePayment();
  const statements = statementsStore.use();
  const payouts = payoutsStore.use();
  const statementsNext = statementsNextStore.use();
  // 내역 목록은 이 화면을 열 때 받는다 (로그인할 때는 잔액 요약만) — 받기 전에는 '없어요' 대신 자리표시
  const listsLoaded = financeListsLoaded.use();
  const listsPending = !MOCK && !listsLoaded;
  useEffect(() => { if (!MOCK) void refreshFinance(); }, []);
  const [loadingMore, setLoadingMore] = useState(false);
  const [exporting, setExporting] = useState(false);
  const loadMoreStatements = async () => {
    if (!statementsNext || loadingMore) return;
    setLoadingMore(true);
    try {
      const page = await portal.fetchStatementsPage(statementsNext);
      const seen = new Set(statementsStore.get().map(x => x.id));
      statementsStore.set(old => [...old, ...page.items.filter(x => !seen.has(x.id))]);
      statementsNextStore.set(page.next);
    } catch (e) {
      toast(errorMessage(e));
    } finally {
      setLoadingMore(false);
    }
  };
  const exportStatements = async () => {
    if (MOCK) {
      if (!orderedStatements.length) { toast('내보낼 정산 내역이 없어요.'); return; }
      saveStatementsCsv(orderedStatements);
      return;
    }
    setExporting(true);
    try {
      await portal.downloadStatementsCsv();
    } catch (e) {
      toast(errorMessage(e));
    } finally {
      setExporting(false);
    }
  };
  const [tab, setTab] = useState<'statements' | 'payouts'>('statements');
  // 수익 유형 — 배급 정산과 저작권료는 원장이 따로다. 같은 화면에서 거르기만 하고 더하지 않는다 (?type=)
  const [params, setParams] = useSearchParams();
  const revType = (['distribution', 'publishing'] as const).find(t => t === params.get('type')) ?? 'all';
  const setRevType = (v: 'all' | 'distribution' | 'publishing') => { const p = new URLSearchParams(params); if (v === 'all') p.delete('type'); else p.set('type', v); setParams(p, { replace: true }); };
  const { data: royalties } = useAsync(() => pubApi.listRoyalties(), [], { key: 'royalties:all' });
  const hasPub = statements.some(st => revenueTypeOf(st) === 'publishing') || !!royalties?.length;
  const estimated = (royalties ?? []).filter(r => r.status === 'estimated').reduce((n, r) => n + r.net, 0);
  const [showPayout, setShowPayout] = useState(false);
  const [showStatement, setShowStatement] = useState(false);
  const [showPaymentSetup, setShowPaymentSetup] = useState(false);
  const [amount, setAmount] = useState('');
  const [payoutNote, setPayoutNote] = useState('');

  const paymentRegistered = isPaymentRegistered(payment);

  const { total, used, left, minimum, held } = useBalance();
  const [requesting, setRequesting] = useState(false);

  const openPayoutModal = () => {
    if (!isPaymentRegistered(payment)) {
      // 버튼 이름이 이미 '계좌 등록하기'라 '먼저 등록해 주세요' 알림은 같은 말을 되풀이하며 등록 화면의 선택지를 가렸다
      setShowPaymentSetup(true);
      return;
    }
    if (!left) { toast('지급을 요청할 수 있는 잔액이 없어요.'); return; }
    if (left < minimum) { toast(`${money(minimum)} 이상부터 지급을 요청할 수 있어요.`); return; }
    setAmount('');
    setPayoutNote('');
    setShowPayout(true);
  };

  // 줄을 누르면 상세를 열고, 삭제는 상세 안에서만 — 금액 옆 ×를 잘못 누르는 일을 막는다
  const [detail, setDetail] = useState<{ kind: 'statement' | 'payout'; id: string } | null>(null);
  const detailStatement = detail?.kind === 'statement' ? statements.find(x => x.id === detail.id) : undefined;
  const detailRoyalty = detailStatement?.royaltyId ? royalties?.find(r => r.id === detailStatement.royaltyId) : undefined;
  const detailPayout = detail?.kind === 'payout' ? payouts.find(x => x.id === detail.id) : undefined;

  const deleteStatement = async (id: string) => {
    const s = statements.find(x => x.id === id);
    if (!s) return;
    const remaining = balance(statements.filter(x => x.id !== id), payouts);
    if (remaining.total < remaining.used) {
      toast('지급 요청 합계보다 정산액이 적어져서 삭제할 수 없어요. 요청 기록을 먼저 정리해 주세요.');
      return;
    }
    if (!(await confirm({ title: '정산 내역을 삭제할까요?', message: `${niceDate(s.created)} · ${entryTitle(s)} · ${money(s.amount)}`, confirmLabel: '삭제', danger: true }))) return;
    statementsStore.set(ss => ss.filter(x => x.id !== id));
    toast('정산 내역을 삭제했어요.');
  };

  const [cancelling, setCancelling] = useState(false);
  const cancelPayout = async (p: Payout) => {
    if (!(await confirm({ title: '지급 요청을 취소할까요?', message: `${niceDate(p.created)} · ${money(p.amount)} — 취소하면 요청 금액이 다시 요청 가능 금액으로 돌아와요.`, confirmLabel: '요청 취소', danger: true }))) return;
    setCancelling(true);
    try {
      await portal.cancelPayout(p.id);
      await refreshFinance();
      setDetail(null);
      toast('지급 요청을 취소했어요.');
    } catch (err) {
      toast(errorMessage(err, '지급 요청을 취소하지 못했어요. 잠시 후 다시 시도해 주세요.'), 'error');
    } finally {
      setCancelling(false);
    }
  };

  const deletePayout = async (id: string) => {
    const p = payouts.find(x => x.id === id);
    if (!p) return;
    if (!(await confirm({ title: '지급 요청 기록을 삭제할까요?', message: `${niceDate(p.created)} · ${money(p.amount)}`, confirmLabel: '삭제', danger: true }))) return;
    payoutsStore.set(ps => ps.filter(x => x.id !== id));
    toast('지급 요청 기록을 삭제했어요.');
  };

  const submitPayout = async (e: React.FormEvent) => {
    e.preventDefault();
    const amt = Number(amount);
    if (!Number.isInteger(amt) || amt <= 0 || amt > left) {
      toast('요청 가능 금액 안에서 입력해 주세요.');
      return;
    }
    if (amt < minimum) { toast(`${money(minimum)} 이상부터 요청할 수 있어요.`); return; }
    if (!MOCK && amt - payoutFee(amt) <= 0) { toast(`수수료 ${money(payoutFee(amt))}을 빼면 받을 금액이 없어요. 금액을 늘려 주세요.`); return; }
    if (!MOCK) {
      if (requesting) return;
      setRequesting(true);
      try {
        await portal.requestPayout(amt);
        await refreshFinance();
        setShowPayout(false);
        setTab('payouts');
        // 얼마를 어디로 — '박승원 님에게 50,000원을 보냈어요'처럼 금액과 계좌를 문장에 (수수료를 뺀 실제 받는 금액)
        toast(`${money(amt - payoutFee(amt))} 지급을 요청했어요. 확인이 끝나면 ${payment ? `${payment.bank} ${payment.last4} 계좌로` : '등록한 계좌로'} 보내 드려요.`);
      } catch (err) {
        toast(errorMessage(err, '지급을 요청하지 못했어요. 잠시 후 다시 시도해 주세요.'), 'error');
      } finally {
        setRequesting(false);
      }
      return;
    }
    payoutsStore.set(ps => [...ps, { id: uid('p'), amount: amt, note: payoutNote.trim(), created: todayStr(), status: 'recorded' }]);
    setShowPayout(false);
    setTab('payouts');
    toast('지급 요청 내역을 저장했어요.');
  };

  // 최근 것부터 — 같은 날은 들어온 순서 그대로(서버는 이미 최신순)
  const day = (s: Statement) => s.created.slice(0, 10);
  const orderedStatements = statements.filter(st => revType === 'all' || revenueTypeOf(st) === revType).sort((a, b) => day(b).localeCompare(day(a)));
  const statementDays = [...orderedStatements.reduce((m, s) => m.set(day(s), [...(m.get(day(s)) ?? []), s]), new Map<string, Statement[]>())];

  return (
    <div id="view-settlement" className="view">
      <div className="ui-page-head">
        <div>
          <p className="eyebrow">수익·성과</p>
          <h1>정산·지급</h1>
          <p>쌓인 정산금은 원할 때 지급을 요청해 받을 수 있어요. <Link to="/terms/settlement" className="aq-inline-link">정산 약관 보기</Link></p>
        </div>
      </div>

      <section className="ui-card settle-hero" aria-labelledby="settleHeroHeading">
        <h2 id="settleHeroHeading">받을 수 있는 금액</h2>
        <strong className="settle-hero-amount"><Money value={left} animate /></strong>
        {/* 쌓인 정산금이 지금 어디에 있는지 — 받을 수 있음 / 지급 처리 중 / 보류를 한 막대로 */}
        {total > 0 ? (
          <div className="settle-split" aria-label={`쌓인 정산금 ${money(total)} 중 받을 수 있음 ${money(left)}, 지급 처리 중 ${money(used)}${held > 0 ? `, 보류 ${money(held)}` : ''}`}>
            {/* 막대가 무엇을 나눈 것인지 글자로 — '받을 수 있음'과 '지급 요청함'이 비율처럼 읽혔다 */}
            <p className="settle-split-cap" aria-hidden="true">쌓인 정산금 <b><Money value={total} /></b> 중</p>
            <div className="settle-split-bar" aria-hidden="true">
              <i className="is-left" style={{ flexGrow: left }} />
              <i className="is-used" style={{ flexGrow: used }} />
              {held > 0 && <i className="is-held" style={{ flexGrow: held }} />}
            </div>
            <ul className="settle-split-legend">
              <li><i className="is-left" />받을 수 있음 <b><Money value={left} /></b></li>
              <li><i className="is-used" />{MOCK ? '지급 요청함' : '지급 처리 중'} <b><Money value={used} /></b></li>
              {held > 0 && <li><i className="is-held" />권리 확인 중 보류 <b><Money value={held} /></b></li>}
            </ul>
            {held > 0 && (
              <p className="settle-held-note">보류 금액은 권리 확인이 끝나면 받을 수 있는 금액으로 옮겨져요. 요청받은 서류가 있으면 <Link to="/rights" className="aq-inline-link">권리·보완 서류</Link>에서 내 주세요.</p>
            )}
          </div>
        ) : (
          <p className="settle-hero-sub">아직 쌓인 정산금이 없어요. 들어오면 바로 받을 수 있어요.</p>
        )}
        {/* 정산금은 정해진 날에 들어오지 않고 운영팀이 넣는 대로 쌓인다 — '다음 정산 ○월 중순' 같은 약속은 하지 않는다 */}
        {left > 0 && paymentRegistered && (
          <dl className="settle-facts">
            <div><dt>지금 모두 받으면</dt><dd>수수료 {payoutFee(left) ? money(payoutFee(left)) : '무료'} · 실제 받는 금액 <b>{money(Math.max(0, left - payoutFee(left)))}</b></dd></div>
          </dl>
        )}
        {minimum > 1 && left < minimum && (
          <div className="settle-min" role="note">
            <div className="settle-min-bar" aria-hidden="true"><i style={{ width: `${Math.min(100, Math.round((left / minimum) * 100))}%` }} /></div>
            <p>{money(minimum - left)} 더 쌓이면 받을 수 있어요 (최소 {money(minimum)}).</p>
          </div>
        )}
        {/* 계좌 줄 전체를 누를 수 있게 — 오른쪽 작은 '등록·변경' 글자(44×38px)만 눌렸다.
            계좌가 없을 때는 이 줄을 빼고 아래 '계좌 등록하기' 버튼 하나만 — 같은 일을 하는 자리가 두 개였다 */}
        {paymentRegistered && payment && (
          <button type="button" className="settle-hero-account is-button" onClick={() => setShowPaymentSetup(true)}
            aria-label={`받을 계좌 변경 — ${payment.bank} ${payment.last4}`}>
            <BankLogo name={payment.bank} />
            <span className="min-0">
              <strong>{payment.bank}</strong>
              <span>•••• {payment.last4} · {payment.recipient}</span>
            </span>
            <span className="settle-hero-link" aria-hidden="true">바꾸기</span>
          </button>
        )}
        <div className="settle-hero-actions">
          {MOCK && <button type="button" className="ui-btn is-soft is-lg" onClick={() => setShowStatement(true)}>정산금 넣기</button>}
          <button type="button" className="ui-btn is-primary is-lg" onClick={openPayoutModal}>{paymentRegistered ? '수익 받기' : '계좌 등록하기'}</button>
        </div>
      </section>
      <SplitsReceived />

      <Segmented
        tabs className="aq-seg-tabs" label="정산 내역 구분" value={tab} onChange={setTab}
        options={[{ value: 'statements', label: '적립 내역' }, { value: 'payouts', label: '지급 요청 기록' }] as const}
      />

      {tab === 'statements' ? (
        <div id="statementList" className="ui-tab-panel" key="st">
          {(orderedStatements.length > 0 || hasPub) && (
            <div className="aq-list-tools">
              {hasPub ? (
                <ChipGroup size="sm" label="수익 유형" value={revType} onChange={setRevType} hideZero={false}
                  options={[{ value: 'all', label: '전체' }, { value: 'distribution', label: '배급' }, { value: 'publishing', label: '저작권료' }]} />
              ) : <span>{statementsNext ? `최근 ${orderedStatements.length}건` : `전체 ${orderedStatements.length}건`}</span>}
              <button type="button" className="ui-btn is-soft is-sm" onClick={exportStatements} disabled={exporting}>
                <Glyph name="download" size={14} className="aq-glyph-lead" />{exporting ? '내려받는 중…' : '전체 기간 CSV'}
              </button>
            </div>
          )}
          {revType !== 'distribution' && estimated > 0 && (
            <div className="ui-callout is-neutral is-row">
              <span>확정 전인 예상 저작권료 <b>{money(estimated)}</b> — 관리 단체가 확정하면 여기에 적립돼요.</span>
              <Link to="/reports?type=publishing" className="ui-btn is-white is-sm">곡별 보기</Link>
            </div>
          )}
          {listsPending ? <SkeletonRows count={3} pattern="amount" /> : orderedStatements.length ? (
            <div className="aq-catalog-cards aq-stagger">
              {/* 통장처럼 날짜별로 — 들어온 정산금은 +, 운영팀이 거둬 간 금액은 − */}
              {statementDays.map(([d, rows]) => (
                <section key={d} className="settle-day" aria-label={dayLabel(d)}>
                  <h3 className="settle-day-head">{dayLabel(d)}</h3>
                  {rows.map(s => (
                    <button type="button" key={s.id} className="ui-row aq-statement-card settle-entry" onClick={() => setDetail({ kind: 'statement', id: s.id })}>
                      <span className={`aq-statement-icon${s.amount < 0 ? ' is-minus' : ''}`} aria-hidden="true"><Glyph name={s.amount < 0 ? 'arrow-up-right' : 'won'} size={17} /></span>
                      <div className="min-0">
                        <span className="ui-row-title">{entryTitle(s)}</span>
                        <span className="ui-row-meta">{[entryKind(s), s.platform, s.period && `${s.period}분`].filter(Boolean).map(t => <span key={t}>{t}</span>)}</span>
                      </div>
                      <div className="aq-statement-end"><SignedMoney value={s.amount} /></div>
                    </button>
                  ))}
                </section>
              ))}
              {statementsNext && (
                <button type="button" className={`ui-btn is-white aq-report-more${loadingMore ? ' is-busy' : ''}`} onClick={loadMoreStatements} disabled={loadingMore}>
                  {loadingMore ? '불러오는 중…' : '이전 내역 더 보기'}
                  <Glyph name="chevron-down" size={15} />
                </button>
              )}
            </div>
          ) : (
            <div className="ui-empty">
              <h2>아직 들어온 정산금이 없어요.</h2>
              {MOCK ? (<>
                <p>체험용 정산금을 넣어 지급 요청까지 해 볼 수 있어요.</p>
                <button type="button" className="ui-btn is-primary" onClick={() => setShowStatement(true)}>체험용 정산금 넣기</button>
              </>) : <p>운영팀이 정산금을 넣으면 여기에 쌓이고, 바로 지급을 요청할 수 있어요.</p>}
            </div>
          )}
        </div>
      ) : (
        <div id="payoutList" className="ui-tab-panel" key="po">
          {listsPending ? <SkeletonRows count={3} pattern="amount" /> : payouts.length ? (
            <div className="aq-catalog-cards aq-stagger">
              {(MOCK ? [...payouts].reverse() : payouts).map(p => (
                <button type="button" key={p.id} className="ui-row aq-statement-card" onClick={() => setDetail({ kind: 'payout', id: p.id })}>
                  <span className="aq-statement-icon" aria-hidden="true"><Glyph name="arrow-up-right" size={17} /></span>
                  <div className="min-0">
                    <span className="ui-row-title"><Money value={p.amount} /> · 지급 요청</span>
                    <span className="ui-row-meta"><span>{niceDate(p.created)}</span><span>{p.note || (MOCK ? '현재 작업 공간에만 기록됨' : '등록한 계좌로 지급')}</span></span>
                    {payoutStep(p.status) >= 0 && (
                      <ol className="settle-steps" aria-label={`지급 진행: ${PAYOUT_STEPS[Math.max(0, payoutStep(p.status))]}`}>
                        {PAYOUT_STEPS.map((label, i) => <li key={label} className={i <= payoutStep(p.status) ? 'is-done' : ''}>{label}</li>)}
                      </ol>
                    )}
                    {p.status === 'failed' && <span className="settle-fail">계좌 정보를 확인한 뒤 다시 요청해 주세요.</span>}
                  </div>
                  <div className="aq-statement-end">
                    <Badge tone={PAYOUT_STATUS[p.status]?.[1] ?? 'violet'}>{PAYOUT_STATUS[p.status]?.[0] ?? p.status}</Badge>
                    <Glyph name="chevron-right" size={16} />
                  </div>
                </button>
              ))}
            </div>
          ) : (
            <div className="ui-empty">
              <h2>지급 요청 기록이 없어요.</h2>
              <p>‘수익 받기’로 지급을 요청하면 여기에 쌓여요.</p>
            </div>
          )}
        </div>
      )}

      {detailStatement && (
        <Modal title={entryTitle(detailStatement)} onClose={() => setDetail(null)}>
          <dl className="ui-kv aq-statement-detail">
            <div><dt>금액</dt><dd><SignedMoney value={detailStatement.amount} /></dd></div>
            <div><dt>구분</dt><dd>{detailStatement.amount < 0 ? '차감 — 잔액에서 빠졌어요' : '적립 — 잔액에 더해졌어요'}</dd></div>
            <div><dt>날짜</dt><dd>{niceDate(detailStatement.created)}</dd></div>
            {detailStatement.platform && <div><dt>플랫폼</dt><dd>{detailStatement.platform}</dd></div>}
            {detailStatement.period && <div><dt>정산 기간</dt><dd>{detailStatement.period}</dd></div>}
            {detailRoyalty && <>
              <div><dt>곡</dt><dd><Link to={`/works/${detailRoyalty.workId}?tab=revenue`} className="aq-inline-link">{detailRoyalty.workTitle}</Link></dd></div>
              <div><dt>수익 유형</dt><dd>저작권료 · {ROYALTY_KIND_LABEL[detailRoyalty.kind]}</dd></div>
              <div><dt>수금액</dt><dd><Money value={detailRoyalty.collected} currency={detailRoyalty.currency} /></dd></div>
              <div><dt>관리 지분 · 수수료</dt><dd>{pct(detailRoyalty.share)} · <Money value={detailRoyalty.fee} currency={detailRoyalty.currency} /></dd></div>
              <div><dt>국가</dt><dd>{detailRoyalty.country}</dd></div>
            </>}
          </dl>
          {MOCK && (
            <div className="row-actions aq-statement-detail-actions">
              <button type="button" className="ui-btn is-text aq-danger-text" onClick={async () => { const id = detailStatement.id; setDetail(null); await deleteStatement(id); }}>이 내역 삭제</button>
            </div>
          )}
        </Modal>
      )}

      {detailPayout && (
        <Modal title={`${money(detailPayout.amount)} 지급 요청`} onClose={() => setDetail(null)}>
          <dl className="ui-kv aq-statement-detail">
            <div><dt>요청 금액</dt><dd><Money value={detailPayout.amount} /></dd></div>
            {detailPayout.fee != null && <div><dt>출금 수수료</dt><dd>{detailPayout.fee ? <Money value={detailPayout.fee} /> : '무료'}</dd></div>}
            {detailPayout.net != null && <div><dt>실수령액</dt><dd><Money value={detailPayout.net} /></dd></div>}
            <div><dt>상태</dt><dd><Badge tone={PAYOUT_STATUS[detailPayout.status]?.[1] ?? 'violet'}>{PAYOUT_STATUS[detailPayout.status]?.[0] ?? detailPayout.status}</Badge></dd></div>
            <div><dt>요청일</dt><dd>{niceDate(detailPayout.created)}</dd></div>
            {detailPayout.note && <div><dt>메모</dt><dd>{detailPayout.note}</dd></div>}
          </dl>
          {MOCK && (
            <div className="row-actions aq-statement-detail-actions">
              <button type="button" className="ui-btn is-text aq-danger-text" onClick={async () => { const id = detailPayout.id; setDetail(null); await deletePayout(id); }}>요청 기록 삭제</button>
            </div>
          )}
          {!MOCK && detailPayout.cancellable && (
            <div className="row-actions aq-statement-detail-actions">
              <button type="button" className={`ui-btn is-text aq-danger-text${cancelling ? ' is-busy' : ''}`} disabled={cancelling} onClick={() => void cancelPayout(detailPayout)}>요청 취소</button>
            </div>
          )}
        </Modal>
      )}

      {showPayout && (
        <Modal title="수익 지급 요청" onClose={() => setShowPayout(false)}>
          <div className="studio-payout-form">
            <p className="eyebrow">지급 요청 가능 금액</p>
            <strong className="studio-payout-number"><Money value={left} mark /></strong>
            <p className="small muted">원하는 금액을 입력하고 받을 계좌를 확인해 주세요.</p>
            <form id="payoutForm" onSubmit={submitPayout}>
              <div className="ui-field">
                <label htmlFor="pAmount">받을 금액 (원)</label>
                <div className="studio-amount-field">
                  <input
                    // type=number는 마우스 휠·방향키로 금액이 몰래 바뀌고 쉼표 표시가 안 돼 숫자 전용 텍스트로 받는다
                    id="pAmount" inputMode="numeric" type="text" autoComplete="off" enterKeyHint="done"
                    required placeholder={`예: ${left.toLocaleString('ko-KR')}`}
                    value={amount ? Number(amount).toLocaleString('ko-KR') : ''}
                    onChange={e => setAmount(e.target.value.replace(/\D/g, '').replace(/^0+/, '').slice(0, 15))}
                  />
                  <span className="aq-amount-unit" aria-hidden="true">원</span>
                  <button type="button" id="payoutAll" className="ui-btn is-white is-sm" onClick={() => setAmount(String(Math.floor(left)))}>전액</button>
                </div>
                <p className="ui-field-help">
                  {!MOCK && Number(amount) > 0
                    ? `출금 수수료 ${payoutFee(Number(amount)) ? money(payoutFee(Number(amount))) : '무료'} · 실수령 ${money(Math.max(0, Number(amount) - payoutFee(Number(amount))))}`
                    : '요청할 수 있는 금액보다 많아요.'}
                </p>
              </div>
              <div className="studio-receive-account">
                <BankLogo name={payment?.bank || ''} />
                <div>
                  <strong>{payment?.bank || '은행 등록 전'}</strong>
                  <p className="small muted">
                    {payment?.recipient || '받는 분 등록 전'} · {payment?.last4 ? `•••• ${payment.last4}` : '계좌 등록 전'}
                  </p>
                </div>
                <span className="studio-account-check" aria-label="받을 계좌"><CheckIcon size={12} /></span>
              </div>
              {MOCK && <div className="ui-field">
                <label htmlFor="pNote">메모<span className="aq-optional">선택</span></label>
                <input
                  id="pNote" maxLength={200} placeholder="필요한 내용을 남겨 주세요"
                  value={payoutNote} onChange={e => setPayoutNote(e.target.value)}
                />
              </div>}
              <div className="doc-connection">
                {MOCK
                  ? '신청 내용은 현재 작업 공간에 저장돼요. 지급 서비스가 연결되기 전에는 실제로 송금되지 않아요.'
                  : `담당자가 확인한 뒤 등록한 계좌로 보내 드려요. 30,000원 이하는 수수료 500원(부가가치세 포함), 초과는 무료예요.${minimum > 0 ? ` ${money(minimum)} 이상부터 요청할 수 있어요.` : ' 2027년 12월 31일까지는 최소 출금금액 없이 요청할 수 있어요.'} 담당자가 승인하기 전까지는 취소할 수 있어요.`}
              </div>
              <div className="aq-sticky-foot">
                <button className={`ui-btn is-primary studio-submit-wide${requesting ? ' is-busy' : ''}`} type="submit" disabled={requesting}>
                  {MOCK ? '지급 요청 내용 저장' : requesting ? '요청하는 중' : '지급 요청하기'}
                </button>
              </div>
            </form>
          </div>
        </Modal>
      )}

      {showStatement && (
        <Modal title="체험용 정산금 넣기" onClose={() => setShowStatement(false)}>
          <StatementForm
            onSave={st => {
              statementsStore.set(ss => [...ss, { id: uid('st'), period: '', platform: '', ...st, created: todayStr() }]);
              setTab('statements');
              toast(`정산금 ${money(st.amount)}을 넣었어요.`);
            }}
          />
        </Modal>
      )}

      {showPaymentSetup && <PaymentSetupModal onClose={() => setShowPaymentSetup(false)} />}
    </div>
  );
}
