import type { Check } from './api';
import { CHECK_STATUS, checkLabel, checkSummary, pick } from './labels';
import { Chip, StatusChip } from './ui';

/** 검사 항목 — 쉬운 설명을 먼저, 검사 코드와 원문은 ‘상세 보기’에 */
export function CheckCard({ c, open }: { c: Check; open?: boolean }) {
  const sensitive = open && c.needs_second_approval === true;
  const cls = ['adm-check', open ? 'is-open' : '', sensitive ? 'is-sensitive' : ''].filter(Boolean).join(' ');
  return (
    <div className={cls}>
      <div className="adm-check-top">
        <b>{checkLabel(c.check_code)}{c.stage && <small> · {c.stage}차 검사</small>}</b>
        <span className="adm-codes">
          {sensitive && <Chip tone="red">2인 승인 필요</Chip>}
          <StatusChip value={pick(CHECK_STATUS, c.status)} />
        </span>
      </div>
      <p>{checkSummary(c)}</p>
      <details className="adm-more">
        <summary>상세 보기</summary>
        <div><span className="adm-code">{c.check_code}</span></div>
        {c.original_status && c.original_status !== c.status && <p>시스템 결과: {pick(CHECK_STATUS, c.original_status)[0]} · 담당자 결정 반영</p>}
        {c.detail && <p className="adm-raw">{c.detail}</p>}
      </details>
    </div>
  );
}

