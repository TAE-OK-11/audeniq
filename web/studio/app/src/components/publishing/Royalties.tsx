// 저작권료 — 정산 화면 '적립 내역'과 같은 모양: 위에 숫자 칸, 아래는 기간별로 묶은 금액 줄, 누르면 상세 창.
// 상세 창에 곡 제목 · 유형 · 기간 · 징수 기관 · 국가 · 통화 · 수금액 · 관리 지분 · 수수료 · 정산금 · 지급 상태 (기획안 8.2).
// 확정과 예상은 따로 더한다 — 예상은 아직 받을 수 있는 돈이 아니다
import { useMemo, useState } from 'react';
import type { Royalty, RoyaltyStatus } from '../../api/publishing';
import { ROYALTY_KIND_LABEL, ROYALTY_STATUS_LABEL, ROYALTY_STATUS_TONE, pct } from '../../lib/publishing';
import { Money } from '../Money';
import { Modal } from '../Modal';
import { Link } from '../../lib/router';
import { Glyph } from '../Glyph';
import { Badge, ChipGroup, EmptyState } from '../../ui';

const periodText = (p: string) => `${p.slice(0, 4)}년 ${Number(p.slice(5, 7))}월분`;

export function royaltyTotals(rows: readonly Royalty[]) {
  const t = { confirmed: 0, estimated: 0, paid: 0 };
  for (const r of rows) t[r.status] += r.net;
  return t;
}

function saveCsv(rows: readonly Royalty[]) {
  const head = ['곡', '수익 유형', '기간', '징수 기관', '국가', '통화', '수금액', '관리 지분(%)', '수수료', '정산금', '상태'];
  const body = rows.map(r => [r.workTitle, ROYALTY_KIND_LABEL[r.kind], r.period, r.source, r.country, r.currency, r.collected, r.share, r.fee, r.net, ROYALTY_STATUS_LABEL[r.status]]);
  const csv = '﻿' + [head, ...body].map(row => row.map(v => `"${String(v).replace(/"/g, '""')}"`).join(',')).join('\n');
  const a = document.createElement('a');
  a.href = URL.createObjectURL(new Blob([csv], { type: 'text/csv;charset=utf-8' }));
  a.download = `audeniq-publishing-royalties-${new Date().toISOString().slice(0, 10)}.csv`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

/** summary=false: 합계 카드 없이 내역만 (퍼블리싱 '저작권료' 탭 — 합계는 '분석' 탭에) */
export function RoyaltyTable({ rows, showWork = true, emptyText, summary = true }: { rows: readonly Royalty[]; showWork?: boolean; emptyText?: string; summary?: boolean }) {
  const [status, setStatus] = useState<'all' | RoyaltyStatus>('all');
  const [open, setOpen] = useState<Royalty | null>(null);
  const counts = useMemo(() => {
    const c: Record<string, number> = { all: rows.length };
    for (const r of rows) c[r.status] = (c[r.status] ?? 0) + 1;
    return c;
  }, [rows]);
  const shown = status === 'all' ? rows : rows.filter(r => r.status === status);
  const byPeriod = useMemo(() => {
    const m = new Map<string, Royalty[]>();
    for (const r of shown) m.set(r.period, [...(m.get(r.period) ?? []), r]);
    return [...m.entries()].sort((a, b) => b[0].localeCompare(a[0]));
  }, [shown]);
  const totals = royaltyTotals(rows);

  if (!rows.length) {
    return (
      <EmptyState compact icon={<Glyph name="receipt" size={22} />} title="아직 들어온 저작권료가 없어요">
        {emptyText ?? '관리 단체가 사용료를 나눠 주면 여기서 곡별로 보여 드려요. 보통 사용된 달부터 3~6개월 걸려요.'}
      </EmptyState>
    );
  }
  return (
    <>
      {/* 정산·지급의 '받을 수 있는 금액'과 같은 카드 — 큰 금액 하나 + 한 막대로 나눈 몫 + 범례 */}
      {summary && <section className="ui-card settle-hero" aria-label="저작권료 합계">
        <h2>확정된 저작권료</h2>
        <strong className="settle-hero-amount"><Money value={totals.confirmed + totals.paid} animate /></strong>
        <div className="settle-split">
          <div className="settle-split-bar" aria-hidden="true">
            <i className="is-left" style={{ flexGrow: totals.confirmed }} />
            <i className="is-used" style={{ flexGrow: totals.paid }} />
            {totals.estimated > 0 && <i className="is-held" style={{ flexGrow: totals.estimated }} />}
          </div>
          <ul className="settle-split-legend">
            <li><i className="is-left" />받을 수 있음 <b><Money value={totals.confirmed} /></b></li>
            <li><i className="is-used" />지급 완료 <b><Money value={totals.paid} /></b></li>
            {totals.estimated > 0 && <li><i className="is-held" />확정 전 예상 <b><Money value={totals.estimated} /></b></li>}
          </ul>
        </div>
      </section>}
      <div className="aq-list-tools">
        <ChipGroup size="sm" label="지급 상태" value={status} onChange={setStatus}
          options={[{ value: 'all', label: '전체', count: counts.all }, ...(['confirmed', 'estimated', 'paid'] as const).map(s => ({ value: s, label: ROYALTY_STATUS_LABEL[s], count: counts[s] ?? 0 }))]} />
        <button type="button" className="ui-btn is-soft is-sm" onClick={() => saveCsv(shown)}><Glyph name="download" size={14} className="aq-glyph-lead" />명세서 CSV</button>
      </div>
      <div className="aq-catalog-cards aq-stagger">
        {byPeriod.map(([p, list]) => (
          <section key={p} className="settle-day" aria-label={periodText(p)}>
            <h3 className="settle-day-head">{periodText(p)}</h3>
            {list.map(r => (
              <button type="button" key={r.id} className="ui-row aq-statement-card settle-entry" onClick={() => setOpen(r)}>
                {/* 정산·지급 '적립 내역' 줄과 같은 모양 — 아이콘 · 제목 한 줄 · 출처 · 오른쪽 금액 (예상은 흐리게) */}
                <span className="aq-statement-icon" aria-hidden="true"><Glyph name="won" size={17} /></span>
                <div className="min-0">
                  <span className="ui-row-title">{showWork ? `${r.workTitle} ` : ''}{ROYALTY_KIND_LABEL[r.kind]}</span>
                  <span className="ui-row-meta"><span>{ROYALTY_STATUS_LABEL[r.status]}</span><span>{r.source}</span><span>{r.country}</span></span>
                </div>
                <div className="aq-statement-end">
                  <span className={`settle-entry-amount is-plus${r.status === 'estimated' ? ' is-estimated' : ''}`}>+<Money value={r.net} currency={r.currency} /></span>
                </div>
              </button>
            ))}
          </section>
        ))}
      </div>
      {open && (
        <Modal title={`${open.workTitle} · ${ROYALTY_KIND_LABEL[open.kind]}`} onClose={() => setOpen(null)}>
          <dl className="ui-kv aq-statement-detail">
            <div><dt>정산금</dt><dd><Money value={open.net} currency={open.currency} /></dd></div>
            <div><dt>지급 상태</dt><dd><Badge tone={ROYALTY_STATUS_TONE[open.status]}>{ROYALTY_STATUS_LABEL[open.status]}</Badge></dd></div>
            <div><dt>수금액</dt><dd><Money value={open.collected} currency={open.currency} /></dd></div>
            <div><dt>관리 지분</dt><dd>{pct(open.share)}</dd></div>
            <div><dt>수수료</dt><dd><Money value={open.fee} currency={open.currency} /></dd></div>
            <div><dt>수익 발생 기간</dt><dd>{periodText(open.period)}</dd></div>
            <div><dt>징수 기관</dt><dd>{open.source}</dd></div>
            <div><dt>국가 · 통화</dt><dd>{open.country} · {open.currency}</dd></div>
          </dl>
          {showWork && <Link to={`/works/${open.workId}?tab=revenue`} className="ui-btn is-soft is-block">곡 보기</Link>}
        </Modal>
      )}
    </>
  );
}
