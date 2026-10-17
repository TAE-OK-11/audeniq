// 정산·지급 — hero + 탭 구조의 단순화된 레이아웃
import { Link } from '../lib/router';
import { SplitsReceived } from '../components/SplitsReceived';
import { useState } from 'react';
import { Modal, useModalClose } from '../components/Modal';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { BankLogo } from '../components/BankLogo';
import { PaymentSetupModal } from '../components/PaymentSetupModal';
import { isPaymentRegistered, usePayment } from '../store/payment';
import { balance, payoutFee, payoutsStore, statementsNextStore, statementsStore, useBalance, type Payout, type Statement } from '../store/settlement';
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

/** 다음 정산 반영 예상 — 보통 매월 중순에 전월 정산이 반영된다 (정산 약관 제5조: 매월 1회). 날짜를 약속하지 않고 '중순'으로만 */
function nextSettlement(today = new Date()): { when: string; period: string } {
  const y = today.getFullYear(), m = today.getMonth(); // 0-based
  const target = today.getDate() < 15 ? new Date(y, m, 1) : new Date(y, m + 1, 1);
  const prev = new Date(target.getFullYear(), target.getMonth() - 1, 1);
  return { when: `${target.getMonth() + 1}월 중순`, period: `${prev.getMonth() + 1}월분` };
}
import { money, niceDate } from '../lib/format';
import { monthKey, todayStr } from '../lib/date';
import { uid } from '../lib/store';
import { CheckIcon } from '../components/Check';
import { Glyph } from '../components/Glyph';
import { Money } from '../components/Money';
import { Segmented } from '../components/Segmented';
import { Badge, type Tone } from '../ui';
import { MonthField } from '../components/Picker';

const PLATFORMS = ['Spotify', 'Apple Music', 'YouTube Music', '멜론', '지니', 'FLO', '벅스', 'Amazon Music', 'TIDAL', 'Deezer', '기타'];

/** 정산 내역 직접 기록 폼 */
function StatementForm({ onSave }: { onSave: (s: { period: string; platform: string; amount: number; note: string }) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [period, setPeriod] = useState(monthKey(-1));
  const [platform, setPlatform] = useState(PLATFORMS[0]);
  const [amount, setAmount] = useState('');
  const [note, setNote] = useState('');
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const amt = Number(amount.replace(/,/g, ''));
    if (!/^\d{4}-\d{2}$/.test(period)) { toast('정산 기간을 선택해 주세요.'); return; }
    if (!Number.isInteger(amt) || amt <= 0) { toast('정산 금액을 1원 이상 입력해 주세요.'); return; }
    onSave({ period, platform, amount: amt, note: note.trim() });
    close();
  };
  return (
    <form onSubmit={submit}>
      <p className="small muted">받은 정산서의 기간·플랫폼·금액을 기록하면 요청 가능 잔액에 반영돼요.</p>
      <div className="form-grid">
        <div className="ui-field">
          <label htmlFor="stPeriod">정산 기간</label>
          <MonthField id="stPeriod" required max={monthKey(0)} value={period} onChange={setPeriod} />
        </div>
        <div className="ui-field">
          <label htmlFor="stPlatform">플랫폼</label>
          <select id="stPlatform" value={platform} onChange={e => setPlatform(e.target.value)}>
            {PLATFORMS.map(p => <option key={p}>{p}</option>)}
          </select>
        </div>
      </div>
      <div className="ui-field">
        <label htmlFor="stAmount">정산 금액 (원)</label>
        <input id="stAmount" inputMode="numeric" required placeholder="예: 24406"
          value={amount} onChange={e => setAmount(e.target.value.replace(/[^\d]/g, ''))} />
        {amount && <p className="ui-field-help">{money(Number(amount))}</p>}
      </div>
      <div className="ui-field">
        <label htmlFor="stNote">메모<span className="aq-optional">선택</span></label>
        <input id="stNote" maxLength={200} value={note} onChange={e => setNote(e.target.value)} placeholder="정산서 번호 등" />
      </div>
      <div className="aq-sticky-foot">
        <button className="ui-btn is-primary studio-submit-wide" type="submit">정산 내역 기록하기</button>
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
      if (!statements.length) { toast('내보낼 정산 내역이 없어요.'); return; }
      saveStatementsCsv(statements);
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
      toast('수익을 받을 정보를 먼저 등록해 주세요.');
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
  const detailPayout = detail?.kind === 'payout' ? payouts.find(x => x.id === detail.id) : undefined;

  const deleteStatement = async (id: string) => {
    const s = statements.find(x => x.id === id);
    if (!s) return;
    const remaining = balance(statements.filter(x => x.id !== id), payouts);
    if (remaining.total < remaining.used) {
      toast('지급 요청 합계보다 정산액이 적어져서 삭제할 수 없어요. 요청 기록을 먼저 정리해 주세요.');
      return;
    }
    if (!(await confirm({ title: '정산 내역을 삭제할까요?', message: `${s.period} · ${s.platform} · ${money(s.amount)}`, confirmLabel: '삭제', danger: true }))) return;
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

  const orderedStatements = statements.slice().sort((a, b) => b.period.localeCompare(a.period));
  const statementMonths = [...orderedStatements.reduce((m, s) => m.set(s.period, [...(m.get(s.period) ?? []), s]), new Map<string, Statement[]>())];
  const monthTitle = (p: string) => (/^\d{4}-\d{2}/.test(p) ? `${p.slice(0, 4)}년 ${+p.slice(5, 7)}월 수익` : p);

  return (
    <div id="view-settlement" className="view">
      <div className="ui-page-head">
        <div>
          <p className="eyebrow">내 작업실</p>
          <h1>정산·지급</h1>
          <p>확정된 정산과 지급 요청 기록이에요. <Link to="/terms/settlement" className="aq-inline-link">정산 약관 보기</Link></p>
        </div>
      </div>

      <section className="ui-card settle-hero" aria-labelledby="settleHeroHeading">
        <h2 id="settleHeroHeading">받을 수 있는 금액</h2>
        <strong className="settle-hero-amount"><Money value={left} animate /></strong>
        {/* 확정 정산액이 지금 어디에 있는지 — 받을 수 있음 / 지급 처리 중 / 보류를 한 막대로 */}
        {total > 0 ? (
          <div className="settle-split" aria-label={`확정 정산 ${money(total)} 중 받을 수 있음 ${money(left)}, 지급 처리 중 ${money(used)}${held > 0 ? `, 보류 ${money(held)}` : ''}`}>
            {/* 막대가 무엇을 나눈 것인지 글자로 — '받을 수 있음'과 '지급 요청함'이 비율처럼 읽혔다 */}
            <p className="settle-split-cap" aria-hidden="true">확정 정산 <b><Money value={total} /></b> 중</p>
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
          <p className="settle-hero-sub">아직 확정된 정산이 없어요.</p>
        )}
        <dl className="settle-facts">
          <div><dt>다음 정산</dt><dd>{nextSettlement().when} · {nextSettlement().period} 반영 예정</dd></div>
          {left > 0 && paymentRegistered && (
            <div><dt>지금 모두 받으면</dt><dd>수수료 {payoutFee(left) ? money(payoutFee(left)) : '무료'} · 실제 받는 금액 <b>{money(Math.max(0, left - payoutFee(left)))}</b></dd></div>
          )}
        </dl>
        {minimum > 1 && left < minimum && (
          <div className="settle-min" role="note">
            <div className="settle-min-bar" aria-hidden="true"><i style={{ width: `${Math.min(100, Math.round((left / minimum) * 100))}%` }} /></div>
            <p>{money(minimum - left)} 더 쌓이면 받을 수 있어요 (최소 {money(minimum)}). 모자란 금액은 다음 정산으로 넘어가요.</p>
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
            <span className="settle-hero-link" aria-hidden="true">변경</span>
          </button>
        )}
        <div className="settle-hero-actions">
          {MOCK && <button type="button" className="ui-btn is-soft is-lg" onClick={() => setShowStatement(true)}>예시 정산 추가</button>}
          <button type="button" className="ui-btn is-primary is-lg" onClick={openPayoutModal}>{paymentRegistered ? '수익 받기' : '계좌 등록하기'}</button>
        </div>
      </section>
      <SplitsReceived />

      <Segmented
        tabs className="aq-seg-tabs" label="정산 내역 구분" value={tab} onChange={setTab}
        options={[{ value: 'statements', label: '정산 내역' }, { value: 'payouts', label: '지급 요청 기록' }] as const}
      />

      {tab === 'statements' ? (
        <div id="statementList" className="ui-tab-panel" key="st">
          {orderedStatements.length > 0 && (
            <div className="aq-list-tools">
              <span>{statementsNext ? `최근 ${orderedStatements.length}건` : `전체 ${orderedStatements.length}건`}</span>
              <button type="button" className="ui-btn is-soft is-sm" onClick={exportStatements} disabled={exporting}>
                <Glyph name="download" size={14} className="aq-glyph-lead" />{exporting ? '내려받는 중…' : '전체 기간 CSV'}
              </button>
            </div>
          )}
          {orderedStatements.length ? (
            <div className="aq-catalog-cards aq-stagger">
              {/* 달별로 묶어 합계를 먼저 보이고, 그 아래 플랫폼별 금액 — 같은 달 줄이 흩어져 있어 한 달에 얼마인지 더해 봐야 했다 */}
              {statementMonths.map(([period, rows]) => (
                <section key={period} className="settle-month" aria-label={`${period} 정산`}>
                  <header className="settle-month-head">
                    <span>{monthTitle(period)}</span>
                    <small>플랫폼 {rows.length}곳</small>
                    <strong><Money value={rows.reduce((n, r) => n + r.amount, 0)} /></strong>
                  </header>
                  {rows.map(s => (
                    <button type="button" key={s.id} className="ui-row aq-statement-card settle-month-row" onClick={() => setDetail({ kind: 'statement', id: s.id })}>
                      <div className="min-0">
                        <span className="ui-row-title">{s.platform}</span>
                        <span className="ui-row-meta"><span>{s.note || (MOCK ? '체험용으로 추가한 정산' : '플랫폼 정산')}</span><span>{niceDate(s.created)} 반영</span></span>
                      </div>
                      <div className="aq-statement-end">
                        <strong><Money value={s.amount} /></strong>
                        <Glyph name="chevron-right" size={16} />
                      </div>
                    </button>
                  ))}
                </section>
              ))}
              {statementsNext && (
                <button type="button" className="ui-btn is-white aq-report-more" onClick={loadMoreStatements} disabled={loadingMore}>
                  {loadingMore ? '불러오는 중…' : '이전 정산 내역 더 보기'}
                  <Glyph name="chevron-down" size={15} />
                </button>
              )}
            </div>
          ) : (
            <div className="ui-empty">
              <h2>정산 내역이 없어요.</h2>
              {MOCK ? (<>
                <p>정산서를 받은 뒤 금액과 기간을 직접 기록할 수 있어요.</p>
                <button type="button" className="ui-btn is-primary" onClick={() => setShowStatement(true)}>정산 내역 기록하기</button>
              </>) : <p>플랫폼 정산이 확정되면 이곳에 표시돼요. 보통 매월 중순에 전월 정산이 반영돼요.</p>}
            </div>
          )}
        </div>
      ) : (
        <div id="payoutList" className="ui-tab-panel" key="po">
          {payouts.length ? (
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
        <Modal title={`${detailStatement.period} · ${detailStatement.platform}`} onClose={() => setDetail(null)}>
          <dl className="ui-kv aq-statement-detail">
            <div><dt>정산 금액</dt><dd><Money value={detailStatement.amount} /></dd></div>
            <div><dt>정산 기간</dt><dd>{detailStatement.period}</dd></div>
            <div><dt>플랫폼</dt><dd>{detailStatement.platform}</dd></div>
            <div><dt>기록일</dt><dd>{niceDate(detailStatement.created)}</dd></div>
            {detailStatement.note && <div><dt>메모</dt><dd>{detailStatement.note}</dd></div>}
          </dl>
          {MOCK && (
            <div className="row-actions aq-statement-detail-actions">
              <button type="button" className="ui-btn is-text aq-danger-text" onClick={async () => { const id = detailStatement.id; setDetail(null); await deleteStatement(id); }}>정산 내역 삭제</button>
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
            <p className="small muted">원하는 금액을 입력하고 받으실 계좌를 확인해 주세요.</p>
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
                  <button type="button" id="payoutAll" className="ui-btn is-white is-sm" onClick={() => setAmount(String(Math.floor(left)))}>전액</button>
                </div>
                <p className="ui-field-help">
                  {!MOCK && Number(amount) > 0
                    ? `출금 수수료 ${payoutFee(Number(amount)) ? money(payoutFee(Number(amount))) : '무료'} · 실수령 ${money(Math.max(0, Number(amount) - payoutFee(Number(amount))))}`
                    : '요청 가능 금액을 초과할 수 없어요.'}
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
                <span className="studio-account-check" aria-label="선택된 수령 계좌"><CheckIcon size={12} /></span>
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
                  ? '신청 내용은 현재 작업 공간에 저장돼요. 지급 서비스가 연결되기 전에는 실제 송금이 진행되지 않아요.'
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
        <Modal title="정산 내역 기록" onClose={() => setShowStatement(false)}>
          <StatementForm
            onSave={st => {
              statementsStore.set(ss => [...ss, { id: uid('st'), ...st, created: todayStr() }]);
              setTab('statements');
              toast('정산 내역을 기록했어요.');
            }}
          />
        </Modal>
      )}

      {showPaymentSetup && <PaymentSetupModal onClose={() => setShowPaymentSetup(false)} />}
    </div>
  );
}
