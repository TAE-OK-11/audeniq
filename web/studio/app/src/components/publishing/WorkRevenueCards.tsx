// 곡 화면 '성과' 탭 — 다른 탭과 같은 카드 틀(작은 제목 → 한 문장 → 이름·값 줄 → 맨 아래 할 일 줄).
//  · IncomeCards: 저작권료 · 녹음 수익을 큰 숫자 카드 한 장씩 (다른 돈이라 더하지 않는다)
//  · RoyaltyHistoryCard: 저작권료 내역 — 몇 건 들어왔는지 한 문장, 달마다 줄, 누르면 자세히, 아래에 CSV·분기 명세서
//  · RecordingIncomeCard: 녹음 수익 — 몇 곳에서 들어왔는지 한 문장, 위 몇 줄만 (더 보기)
import { useMemo, useState } from 'react';
import { Link } from '../../lib/router';
import type { Royalty, RoyaltyStatus } from '../../api/publishing';
import { ROYALTY_KIND_LABEL, ROYALTY_STATUS_LABEL, ROYALTY_STATUS_TONE, pct, societyName } from '../../lib/publishing';
import { Money } from '../Money';
import { Modal } from '../Modal';
import { Glyph } from '../Glyph';
import { saveRoyaltyCsv } from './Royalties';
import { Badge, ChipGroup } from '../../ui';
import '../../styles/split/Publishing.css';

const month = (p: string) => `${p.slice(0, 4)}년 ${Number(p.slice(5, 7))}월분`;

/** 큰 숫자 카드 두 장 — 저작권료(확정·지급) · 녹음 수익. 확정 전 예상·곡 수로 나눈 추정은 숫자 밑 한 줄 */
export function IncomeCards({ royalties, recorded, split, statsTo }: { royalties: readonly Royalty[]; recorded: number; split: boolean; statsTo: string }) {
  const got = royalties.filter(r => r.status !== 'estimated').reduce((n, r) => n + r.net, 0);
  const est = royalties.filter(r => r.status === 'estimated').reduce((n, r) => n + r.net, 0);
  return (
    <div className="aq-pbl-income-pair">
      <div className="ui-card aq-pbl-income">
        <span className="aq-pbl-income-cap">저작권료</span>
        <strong className="aq-pbl-income-num"><Money value={got} /></strong>
        <span className="aq-pbl-income-sub">{est > 0 ? <>확정 전 예상 <Money value={est} /></> : '작사·작곡 몫 · 관리 단체가 나눠 준 돈'}</span>
      </div>
      <Link to={statsTo} className="ui-card aq-pbl-income is-quiet" aria-label="녹음 수익 · 발매 성과 보기">
        <span className="aq-pbl-income-cap">녹음 수익</span>
        <strong className="aq-pbl-income-num"><Money value={recorded} /></strong>
        <span className="aq-pbl-income-sub">{split ? '앨범 수익을 곡 수로 나눈 추정' : '스트리밍·다운로드'}</span>
        <Glyph name="chevron-right" size={18} className="aq-pbl-income-go" />
      </Link>
    </div>
  );
}

export function RoyaltyHistoryCard({ rows, emptyText, csvLabel }: { rows: readonly Royalty[]; emptyText: string; csvLabel?: string }) {
  const [status, setStatus] = useState<'all' | RoyaltyStatus>('all');
  const [open, setOpen] = useState<Royalty | null>(null);
  const counts = useMemo(() => {
    const c: Record<string, number> = { all: rows.length };
    for (const r of rows) c[r.status] = (c[r.status] ?? 0) + 1;
    return c;
  }, [rows]);
  const shown = status === 'all' ? rows : rows.filter(r => r.status === status);
  const byMonth = useMemo(() => {
    const m = new Map<string, Royalty[]>();
    for (const r of shown) m.set(r.period, [...(m.get(r.period) ?? []), r]);
    return [...m.entries()].sort((a, b) => b[0].localeCompare(a[0]));
  }, [shown]);
  const kinds = new Set(rows.map(r => r.status)).size;
  return (
    <section className="aq-relinfo-card aq-pbl-ccard" aria-labelledby="wrHist">
      <p className="aq-pbl-ccard-cap" id="wrHist">저작권료 내역</p>
      <h2 className="aq-pbl-ccard-title">{rows.length ? `${new Set(rows.map(r => r.period)).size}개월 동안 ${rows.length}건이 들어왔어요` : '아직 들어온 저작권료가 없어요'}</h2>
      {!rows.length && <p className="aq-pbl-ccard-lead">{emptyText}</p>}
      {kinds > 1 && (
        <ChipGroup size="sm" label="지급 상태" value={status} onChange={setStatus}
          options={[{ value: 'all', label: '전체', count: counts.all }, ...(['confirmed', 'estimated', 'paid'] as const).map(s => ({ value: s, label: ROYALTY_STATUS_LABEL[s], count: counts[s] ?? 0 }))]} />
      )}
      {byMonth.map(([p, list]) => (
        <div key={p} className="aq-pbl-month">
          <p className="aq-pbl-month-head">{month(p)}</p>
          <ul className="aq-pbl-kvs">
            {list.map(r => (
              <li key={r.id}>
                <button type="button" className="aq-pbl-kv is-link" onClick={() => setOpen(r)}>
                  <span>{ROYALTY_KIND_LABEL[r.kind] ?? r.kind}<small>{r.source} · {r.country} · {ROYALTY_STATUS_LABEL[r.status]}</small></span>
                  <b className={r.status === 'estimated' ? 'aq-pbl-est' : 'aq-pbl-plus'}>+<Money value={r.net} currency={r.currency} /></b>
                </button>
              </li>
            ))}
          </ul>
        </div>
      ))}
      {rows.length > 0 && (
        <ul className="aq-pbl-kvs">
          <li><button type="button" className="aq-pbl-kv is-link" onClick={() => saveRoyaltyCsv(shown, csvLabel)}><span>명세서 CSV로 받기</span><b className="aq-pbl-ccard-go">받기<Glyph name="download" size={15} /></b></button></li>
          <li><Link to="/reports/publishing/statement" className="aq-pbl-kv is-link"><span>분기 명세서 (세금·회계용)</span><b className="aq-pbl-ccard-go">보기<Glyph name="chevron-right" size={15} /></b></Link></li>
        </ul>
      )}
      {open && (
        <Modal title={`${month(open.period)} · ${ROYALTY_KIND_LABEL[open.kind]}`} onClose={() => setOpen(null)}>
          <dl className="ui-kv aq-statement-detail">
            <div><dt>받을 금액</dt><dd><Money value={open.net} currency={open.currency} /></dd></div>
            <div><dt>상태</dt><dd><Badge tone={ROYALTY_STATUS_TONE[open.status]}>{ROYALTY_STATUS_LABEL[open.status]}</Badge></dd></div>
            <div><dt>징수액</dt><dd><Money value={open.collected} currency={open.currency} /></dd></div>
            <div><dt>관리 지분</dt><dd>{pct(open.share)}</dd></div>
            <div><dt>수수료</dt><dd><Money value={open.fee} currency={open.currency} /></dd></div>
            <div><dt>징수 단체</dt><dd>{societyName(open.source)} · {open.country}</dd></div>
          </dl>
        </Modal>
      )}
    </section>
  );
}

const SHOW = 5;
export function RecordingIncomeCard({ rows }: { rows: readonly { key: string; platform: string; period: string; plays: number; amount: number; note?: string }[] }) {
  const [all, setAll] = useState(false);
  const platforms = new Set(rows.map(r => r.platform)).size;
  const list = all ? rows : rows.slice(0, SHOW);
  return (
    <section className="aq-relinfo-card aq-pbl-ccard" aria-labelledby="wrRec">
      <p className="aq-pbl-ccard-cap" id="wrRec">녹음 수익</p>
      <h2 className="aq-pbl-ccard-title">플랫폼 {platforms}곳에서 들어왔어요</h2>
      <ul className="aq-pbl-kvs">
        {list.map(r => (
          <li key={r.key} className="aq-pbl-kv">
            <span>{r.platform}<small>{[r.period, `${r.plays.toLocaleString('ko-KR')}회`, r.note].filter(Boolean).join(' · ')}</small></span>
            <b><Money value={r.amount} /></b>
          </li>
        ))}
        {rows.length > SHOW && (
          <li><button type="button" className="aq-pbl-kv is-link" onClick={() => setAll(v => !v)}><span>{all ? '접기' : `${rows.length - SHOW}건 더 보기`}</span><b className="aq-pbl-ccard-go"><Glyph name="chevron-down" size={15} className={all ? 'is-flip' : undefined} /></b></button></li>
        )}
      </ul>
      <p className="aq-pbl-ccard-lead">저작권료와 녹음 수익은 같은 정산 잔액에 쌓여 한 번에 받아요. <Link to="/settlement" className="aq-inline-link">정산·지급 보기</Link></p>
    </section>
  );
}
