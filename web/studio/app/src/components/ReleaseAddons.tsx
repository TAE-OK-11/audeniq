// 발매의 부가서비스 — 신청서에서 고른 뮤직비디오 배급·신속 발매·AI 가사 싱크.
// AddonCheckout: 결제를 기다리는 것을 한 번에 결제 (신청 완료 화면·발매 상세 맨 위). 정산금이나 카드(설정한 PG).
// ReleaseAddons: 발매 상세의 '부가서비스' 칸 — 신청마다 진행 단계 카드·결과·취소, 직접 만든 싱크 가사(LRC) 올리기(무료).
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useSearchParams } from '../lib/router';
import { errorMessage } from '../api/errors';
import { money, niceDate } from '../lib/format';
import { PLATFORM_NAMES } from '../lib/platformNames';
import {
  LYRIC_SERVICE_LABEL, RATING_LABEL, SERVICE_LABEL, STATUS_LABEL, STATUS_TONE, TIER,
  cancelOrder, confirmFromReturn, fetchReleaseAddons, fetchTracks, lrcLines, orderStage, payWithBalance, payWithCard, putLrc, statusNote,
  type AddonOrder, type ReleaseAddons as State, type Service, type TrackRow,
} from '../lib/addons';
import { Badge, SkeletonList } from '../ui';
import { Glyph, type GlyphName } from './Glyph';
import { Modal, useModalClose } from './Modal';
import { StageCard } from './StageCard';
import { useToast } from './Toast';
import { useConfirm } from './Confirm';
import { CheckIcon } from './Check';

const ICON: Record<Service, GlyphName> = { MV_DISTRIBUTION: 'release', EXPRESS_RELEASE: 'clock', LYRIC_SYNC: 'music' };

/** 발매의 부가서비스 상태 — handleReturn이면 결제창에서 돌아온 주소(?pay=…)의 승인부터 마친다 (화면에 한 곳만) */
function useReleaseAddons(releaseId: string, handleReturn: boolean) {
  const toast = useToast();
  const [params, setParams] = useSearchParams();
  const [state, setState] = useState<State | null>(null);
  const load = useCallback(async () => {
    try { setState(await fetchReleaseAddons(releaseId)); } catch { setState(s => s ?? null); }
  }, [releaseId]);
  useEffect(() => {
    if (handleReturn && params.get('pay') && params.get('payment')) {
      const q = new URLSearchParams(params);
      const next = new URLSearchParams(params);
      for (const k of ['pay', 'payment', 'paymentKey', 'orderId', 'amount', 'paymentId', 'code', 'message', 'fail', 'pgCode', 'pgMessage', 'transactionType', 'txId']) next.delete(k);
      setParams(next, { replace: true });
      confirmFromReturn(q).then(
        s => { setState(s); toast('결제했어요. 담당자에게 접수됐어요.', 'success'); },
        e => { toast(errorMessage(e), 'error'); void load(); },
      );
      return;
    }
    void load();
  }, [releaseId]); // eslint-disable-line react-hooks/exhaustive-deps
  return { state, setState, load };
}

/** 결제를 기다리는 부가서비스를 한 번에 결제 — 없으면 아무것도 그리지 않는다 */
export function AddonCheckout({ releaseId, state, onPaid, title = '부가서비스 결제' }: {
  releaseId: string; state: State; onPaid: (s: State) => void; title?: string;
}) {
  const toast = useToast();
  const waiting = state.items.filter(o => o.status === 'AWAITING_PAYMENT');
  const total = waiting.reduce((n, o) => n + o.amount, 0);
  const enough = state.balance >= total;
  const [method, setMethod] = useState<'SETTLEMENT' | 'CARD'>(enough || !state.card ? 'SETTLEMENT' : 'CARD');
  const [busy, setBusy] = useState(false);
  useEffect(() => { if (!enough && state.card) setMethod('CARD'); }, [enough, state.card]);
  if (!waiting.length) return null;
  const pay = async () => {
    setBusy(true);
    try {
      const ids = waiting.map(o => o.id);
      if (method === 'SETTLEMENT') { onPaid(await payWithBalance(releaseId, ids)); toast('정산금에서 결제했어요. 담당자에게 접수됐어요.', 'success'); }
      else {
        const s = await payWithCard(releaseId, ids);
        if (s) { onPaid(s); toast('결제했어요. 담당자에게 접수됐어요.', 'success'); }
      }
    } catch (e) { toast(errorMessage(e), 'error'); } finally { setBusy(false); }
  };
  return (
    <section className="aq-addon-checkout" aria-labelledby="addonPayTitle">
      <div className="aq-addon-checkout-head">
        <h2 id="addonPayTitle">{title}</h2>
        <span>신청서에서 고른 부가서비스예요. 결제하면 바로 담당자에게 접수돼요.</span>
      </div>
      <ul className="aq-addon-lines">
        {waiting.map(o => (
          <li key={o.id}>
            <span className="aq-addon-line-icon" aria-hidden="true"><Glyph name={ICON[o.service]} size={16} /></span>
            <span className="min-0">
              <strong>{SERVICE_LABEL[o.service]}{o.tier ? ` · ${TIER[o.tier].label}` : ''}</strong>
              <small>{o.service === 'LYRIC_SYNC' ? `${o.quantity}곡 × ${money(o.unit_price)}` : o.title}</small>
            </span>
            <b>{money(o.amount)}</b>
          </li>
        ))}
        <li className="is-total"><span>합계</span><b>{money(total)}</b></li>
      </ul>
      <div className="ui-choices" role="radiogroup" aria-label="결제 방법">
        <button type="button" role="radio" aria-checked={method === 'SETTLEMENT'} disabled={!enough} className="ui-choice aq-addon-pay" onClick={() => setMethod('SETTLEMENT')}>
          <span>
            <b>정산금으로 결제</b>
            <small>{enough ? `출금할 수 있는 정산금 ${money(state.balance)}에서 바로 빠져요.` : `출금할 수 있는 정산금이 ${money(state.balance)}이라 모자라요.`}</small>
          </span>
          <span className="ui-choice-mark" aria-hidden="true">{method === 'SETTLEMENT' && <CheckIcon size={11} />}</span>
        </button>
        <button type="button" role="radio" aria-checked={method === 'CARD'} disabled={!state.card} className="ui-choice aq-addon-pay" onClick={() => setMethod('CARD')}>
          <span>
            <b>카드 결제</b>
            <small>{state.card ? `${state.card.label} 결제창에서 카드·간편결제로 결제해요.` : '카드 결제는 준비 중이에요. 정산금으로 결제하거나 나중에 결제해 주세요.'}</small>
          </span>
          <span className="ui-choice-mark" aria-hidden="true">{method === 'CARD' && <CheckIcon size={11} />}</span>
        </button>
      </div>
      <button type="button" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy || (method === 'SETTLEMENT' && !enough) || (method === 'CARD' && !state.card)} onClick={() => void pay()}>
        {money(total)} 결제하기
      </button>
      <p className="aq-addon-note">지금 결제하지 않아도 발매 상세에서 언제든 결제할 수 있어요. 담당자가 시작하기 전에 취소하면 결제한 방법으로 전액 환불돼요.</p>
    </section>
  );
}

/** 신청 완료 화면 — 결제할 것이 있을 때만 */
export function ApplicationCheckout({ releaseId }: { releaseId: string }) {
  const { state, setState } = useReleaseAddons(releaseId, false);
  if (!state) return null;
  return <AddonCheckout releaseId={releaseId} state={state} onPaid={setState} title="마지막으로, 부가서비스를 결제해 주세요" />;
}

/** 발매 상세 맨 위 — 결제를 기다리는 부가서비스가 있을 때만, 결제창에서 돌아온 승인도 여기서 */
export function PendingAddons({ releaseId }: { releaseId: string }) {
  const { state, setState } = useReleaseAddons(releaseId, true);
  if (!state) return null;
  return <AddonCheckout releaseId={releaseId} state={state} onPaid={setState} />;
}

/** 발매 상세의 '부가서비스' 칸 */
export function ReleaseAddons({ releaseId, editable }: { releaseId: string; editable: boolean }) {
  const { state, setState, load } = useReleaseAddons(releaseId, false);
  const [params] = useSearchParams();
  const [openId, setOpenId] = useState<string | null>(() => params.get('addon'));
  const [lrcOpen, setLrcOpen] = useState(false);
  if (!state) return <SkeletonList rows={1} />;
  const open = state.items.find(o => o.id === openId) ?? null;
  const shown = state.items.filter(o => o.status !== 'CANCELLED' || o.paid_at);
  return (
    <section className="aq-manage-group aq-release-addons" aria-labelledby="releaseAddonsTitle">
      <h3 className="aq-manage-group-title" id="releaseAddonsTitle">부가서비스</h3>
      {state.items.some(o => o.status === 'AWAITING_PAYMENT') && <p className="aq-addon-empty">결제를 기다리는 부가서비스가 있어요. 이 발매 맨 위에서 결제할 수 있어요.</p>}
      {shown.filter(o => o.status !== 'AWAITING_PAYMENT').length > 0 ? (
        <div className="aq-catalog-cards">
          {shown.filter(o => o.status !== 'AWAITING_PAYMENT').map(o => (
            <button key={o.id} type="button" className="ui-row aq-ticket-card" onClick={() => setOpenId(o.id)}>
              <span className="aq-ticket-icon" aria-hidden="true"><Glyph name={ICON[o.service]} size={18} /></span>
              <span className="min-0">
                <span className="ui-row-title">{SERVICE_LABEL[o.service]}{o.tier ? ` · ${TIER[o.tier].label}` : ''}</span>
                <span className="ui-row-meta">{[o.service === 'MV_DISTRIBUTION' ? o.title : o.service === 'LYRIC_SYNC' ? `${o.quantity}곡` : '', money(o.amount), niceDate(o.created_at)].filter(Boolean).join(' · ')}</span>
              </span>
              <Badge tone={STATUS_TONE[o.status]}>{STATUS_LABEL[o.status]}</Badge>
            </button>
          ))}
        </div>
      ) : !state.items.some(o => o.status === 'AWAITING_PAYMENT') && (
        <p className="aq-addon-empty">뮤직비디오 배급, 신속 발매, AI 가사 싱크는 발매 신청서의 ‘부가서비스’에서 고를 수 있어요.</p>
      )}
      {editable && (
        <button type="button" className="ui-row aq-ticket-card" onClick={() => setLrcOpen(true)}>
          <span className="aq-ticket-icon" aria-hidden="true"><Glyph name="music" size={18} /></span>
          <span className="min-0">
            <span className="ui-row-title">싱크 가사 직접 올리기</span>
            <span className="ui-row-meta">무료 · 만든 LRC 파일을 올리면 가사 서비스에 함께 등록해요</span>
          </span>
          <Glyph name="chevron-right" size={16} className="aq-set-chev" />
        </button>
      )}
      {open && (
        <Modal title={`${SERVICE_LABEL[open.service]}${open.tier ? ` · ${TIER[open.tier].label}` : ''}`} onClose={() => setOpenId(null)}>
          <OrderDetail o={open} releaseId={releaseId} onChanged={s => { setState(s); void load(); }} />
        </Modal>
      )}
      {lrcOpen && (
        <Modal title="싱크 가사 올리기 (무료)" onClose={() => setLrcOpen(false)}>
          <LrcForm releaseId={releaseId} />
        </Modal>
      )}
    </section>
  );
}

function OrderDetail({ o, releaseId, onChanged }: { o: AddonOrder; releaseId: string; onChanged: (s: State) => void }) {
  const toast = useToast();
  const confirm = useConfirm();
  const close = useModalClose();
  const st = orderStage(o);
  const d = o.detail as Record<string, string | string[]>;
  const facts: [string, string][] = [
    ['금액', `${money(o.amount)}${o.quantity > 1 ? ` (${o.quantity}곡 × ${money(o.unit_price)})` : ''}`],
    ['결제', o.pay_method ? `${o.pay_method === 'CARD' ? '카드' : '정산금'} · ${niceDate(o.paid_at ?? '')}${o.refunded_at ? ' · 환불됨' : ''}` : '결제 전'],
  ];
  if (o.tracks.length) facts.unshift(['곡', o.tracks.map(t => t.title).join(', ')]);
  if (o.service === 'MV_DISTRIBUTION') {
    facts.push(['영상', `${String(d.mv_title ?? '')} · ${String(d.duration ?? '')}`],
      ['파일', o.video ? o.video.name : String(d.video_url ?? '')],
      ['플랫폼', (Array.isArray(d.platforms) ? d.platforms : []).map(p => PLATFORM_NAMES[p] ?? p).join(', ')]);
  }
  const rating = o.result.rating || (typeof d.rating === 'string' ? d.rating : '');
  const ratingNo = o.result.rating_no || (typeof d.rating_no === 'string' ? d.rating_no : '');
  const links = Object.entries(o.result.links ?? {});
  const cancel = async () => {
    const ok = await confirm({
      title: '신청을 취소할까요?',
      message: o.paid_at ? `결제한 ${money(o.amount)}은 ${o.pay_method === 'CARD' ? '카드 결제 취소로' : '정산금으로'} 환불돼요.` : '아직 결제 전이라 바로 취소돼요.',
      confirmLabel: '신청 취소', danger: true,
    });
    if (!ok) return;
    try { onChanged(await cancelOrder(o.id, releaseId)); toast('신청을 취소했어요.'); close(); }
    catch (err) { toast(errorMessage(err), 'error'); }
  };
  return (
    <div className="aq-request-detail">
      <StageCard steps={st.steps} current={st.current} complete={st.complete} tone={st.stopped ? 'error' : undefined} note={statusNote(o)} label="신청 진행 단계" />
      {(rating || links.length > 0) && (
        <section className="aq-request-block">
          <h3>결과</h3>
          {rating && <p><span>영상물 등급</span>{RATING_LABEL[rating] ?? rating}{ratingNo ? ` · ${ratingNo}` : ''}</p>}
          {links.map(([p, u]) => <p key={p}><span>{PLATFORM_NAMES[p] ?? LYRIC_SERVICE_LABEL[p] ?? p}</span><a href={u} target="_blank" rel="noopener noreferrer">{u}</a></p>)}
        </section>
      )}
      <section className="aq-request-block">
        <h3>신청 내용</h3>
        {facts.filter(([, v]) => v).map(([k, v]) => <p key={k}><span>{k}</span>{v}</p>)}
      </section>
      {(o.status === 'AWAITING_PAYMENT' || o.status === 'SUBMITTED') && (
        <button type="button" className="ui-btn is-block aq-danger-text aq-request-cancel" onClick={() => void cancel()}>신청 취소</button>
      )}
    </div>
  );
}

function LrcForm({ releaseId }: { releaseId: string }) {
  const close = useModalClose();
  const toast = useToast();
  const [tracks, setTracks] = useState<TrackRow[] | null>(null);
  const [trackId, setTrackId] = useState('');
  const [lrc, setLrc] = useState('');
  const [busy, setBusy] = useState(false);
  useEffect(() => { fetchTracks(releaseId).then(t => { setTracks(t); setTrackId(t[0]?.id ?? ''); }, () => setTracks([])); }, [releaseId]);
  const lines = useMemo(() => (lrc.trim() ? lrcLines(lrc) : null), [lrc]);
  const readFile = async (f: File | undefined) => {
    if (!f) return;
    if (f.size > 200_000) { toast('LRC 파일이 너무 커요.', 'error'); return; }
    setLrc(await f.text());
  };
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!trackId) { toast('곡을 골라 주세요.', 'error'); return; }
    if (lines === null) { toast('[00:12.30]처럼 시간이 붙은 싱크 가사(LRC)를 넣어 주세요.', 'error'); return; }
    setBusy(true);
    try { const n = await putLrc(releaseId, trackId, lrc); toast(`싱크 가사 ${n}줄을 올렸어요.`, 'success'); close(); }
    catch (err) { toast(errorMessage(err), 'error'); } finally { setBusy(false); }
  };
  return (
    <form className="aq-addon-form" onSubmit={submit} noValidate>
      <p className="small muted">직접 만든 싱크 가사(LRC)를 올리면 발매와 함께 가사 서비스에 등록해요. 만들기 어렵다면 다음 신청서의 ‘AI 가사 싱크’(곡당 5,000원)를 골라 주세요.</p>
      <div className="ui-field">
        <label htmlFor="lrcTrack">곡</label>
        <select id="lrcTrack" value={trackId} onChange={e => setTrackId(e.target.value)} disabled={!tracks}>
          {(tracks ?? []).map(t => <option key={t.id} value={t.id}>{t.track_number}. {t.title}{t.lrc ? ' (싱크 가사 있음)' : ''}</option>)}
        </select>
      </div>
      <div className="ui-field">
        <div className="aq-al-section-head">
          <label htmlFor="lrcText" className="ui-field-label">싱크 가사 (LRC)</label>
          <label className="ui-btn is-sm aq-addon-file">
            <input type="file" accept=".lrc,text/plain" className="sr-only" onChange={e => void readFile(e.target.files?.[0])} />
            <Glyph name="download" size={14} />파일에서
          </label>
        </div>
        <textarea id="lrcText" rows={8} maxLength={60000} className="aq-addon-lrc" placeholder={'[00:12.30]첫 줄 가사\n[00:15.80]둘째 줄 가사'} value={lrc} onChange={e => setLrc(e.target.value)} />
        <small className={`ui-field-help${lrc.trim() && lines === null ? ' aq-help-error' : ''}`}>
          {!lrc.trim() ? '줄마다 [분:초.백분의1초] 시간이 있어야 해요.' : lines === null ? '시간이 없는 줄이 있거나 형식이 맞지 않아요.' : `시간이 붙은 줄 ${lines}개`}
        </small>
      </div>
      <div className="aq-sticky-foot">
        <button type="submit" className={`ui-btn is-primary studio-submit-wide${busy ? ' is-busy' : ''}`} disabled={busy}>올리기</button>
      </div>
    </form>
  );
}

