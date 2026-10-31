// 저작권료 — 정산 화면 '적립 내역'과 같은 모양: 위에 숫자 칸, 아래는 기간별로 묶은 금액 줄, 누르면 상세 창.
// 상세 창에 곡 제목 · 유형 · 기간 · 징수 기관 · 국가 · 통화 · 수금액 · 관리 지분 · 수수료 · 정산금 · 지급 상태 (기획안 8.2).
// 확정과 예상은 따로 더한다 — 예상은 아직 받을 수 있는 돈이 아니다
import { useMemo, useState } from 'react';
import type { Royalty, RoyaltyStatus } from '../../api/publishing';
import { ROYALTY_KIND_LABEL, ROYALTY_STATUS_LABEL, ROYALTY_STATUS_TONE, pct, PAYOUT_TIMING } from '../../lib/publishing';
import { Money } from '../Money';
import { Modal } from '../Modal';
import { Link } from '../../lib/router';
import { Glyph } from '../Glyph';
import { Badge, ChipGroup, EmptyState, StatGrid, StatTile } from '../../ui';

import '../../styles/split/Publishing.css';

const periodText = (p: string) => `${p.slice(0, 4)}년 ${Number(p.slice(5, 7))}월분`;

export function royaltyTotals(rows: readonly Royalty[]) {
  const t = { confirmed: 0, estimated: 0, paid: 0 };
  for (const r of rows) t[r.status] += r.net;
  return t;
}

/** 명세서 CSV — label이 있으면 고른 조건(기간·곡·단체·국가)을 파일 이름에 붙인다 */
export function saveRoyaltyCsv(rows: readonly Royalty[], label?: string) {
  const head = ['곡', '수익 유형', '기간', '징수 기관', '국가', '통화', '수금액', '관리 지분(%)', '수수료', '정산금', '상태'];
  const body = rows.map(r => [r.workTitle, ROYALTY_KIND_LABEL[r.kind], r.period, r.source, r.country, r.currency, r.collected, r.share, r.fee, r.net, ROYALTY_STATUS_LABEL[r.status]]);
  const csv = '﻿' + [head, ...body].map(row => row.map(v => `"${String(v).replace(/"/g, '""')}"`).join(',')).join('\n');
  const a = document.createElement('a');
  a.href = URL.createObjectURL(new Blob([csv], { type: 'text/csv;charset=utf-8' }));
  const tag = label ? `-${label.replace(/[\\/:*?"<>|~]+/g, '').replace(/\s*·\s*|\s+/g, '_')}` : '';
  a.download = `audeniq-publishing-royalties${tag}-${new Date().toISOString().slice(0, 10)}.csv`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

/** summary=false: 합계 카드 없이 내역만 (퍼블리싱 '저작권료' 탭 — 합계는 '분석' 탭에) */
export function RoyaltyTable({ rows, showWork = true, emptyText, summary = true, csvLabel }: { rows: readonly Royalty[]; showWork?: boolean; emptyText?: string; summary?: boolean; csvLabel?: string }) {
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
        {emptyText ?? `관리 단체가 사용료를 나눠 주면 여기서 곡별로 보여 드려요. ${PAYOUT_TIMING}`}
      </EmptyState>
    );
  }
  return (
    <>
      {summary && <StatGrid cols={2} className="aq-pbl-royalty-summary">
        <StatTile compact label="누적 확정 저작권료" value={<Money value={totals.confirmed + totals.paid} />} hint={`지급 완료 내역 포함`} />
        <StatTile compact label="확정 전 예상" value={<Money value={totals.estimated} />} hint="확정 후 정산 잔액에 반영돼요" tone="muted" />
      </StatGrid>}
      <div className="aq-list-tools aq-pbl-royalty-tools">
        <ChipGroup size="sm" label="지급 상태" value={status} onChange={setStatus}
          options={[{ value: 'all', label: '전체', count: counts.all }, ...(['confirmed', 'estimated', 'paid'] as const).map(s => ({ value: s, label: ROYALTY_STATUS_LABEL[s], count: counts[s] ?? 0 }))]} />
        {/* 내려받기 두 개는 한 묶음으로 오른쪽에 — 따로 밀려 버튼 사이가 벌어져 보였다 */}
        <span className="aq-pbl-royalty-actions">
          <button type="button" className="ui-btn is-soft is-sm" disabled={!shown.length} onClick={() => saveRoyaltyCsv(shown, csvLabel)}><Glyph name="download" size={14} className="aq-glyph-lead" />명세서 CSV</button>
          {/* 세금·회계용 — 분기마다 곡별 합계가 있는 한 장 (인쇄·PDF) */}
          <Link to="/reports/publishing/statement" className="ui-btn is-soft is-sm"><Glyph name="doc" size={14} className="aq-glyph-lead" />분기 명세서</Link>
        </span>
      </div>
      {!shown.length && <EmptyState compact title="이 상태의 저작권료가 없어요" actions={<button type="button" className="ui-btn is-soft is-sm" onClick={() => setStatus('all')}>전체 내역 보기</button>}>다른 지급 상태를 선택해 주세요.</EmptyState>}
      <div className="aq-catalog-cards aq-stagger aq-pbl-royalty-list">
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
