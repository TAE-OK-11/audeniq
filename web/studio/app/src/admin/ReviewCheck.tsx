import type { Check } from './api';
import { CHECK_STATUS, checkLabel, checkSummary, pick } from './labels';
import { Chip, StatusChip } from './ui';
import { CheckDetail } from './RecordDetail';

/** 검사 항목 — 쉬운 설명을 먼저, 검사 코드와 원문은 ‘자세히’에.
 *  담당자 확인 항목(open)은 구역 제목이 이미 ‘확인 필요’라 상태 칩은 빼고, 2인 승인만 작은 칩으로 */
export function CheckCard({ c, open }: { c: Check; open?: boolean }) {
  const sensitive = open && c.needs_second_approval === true;
  const cls = ['adm-check', open ? 'is-open' : '', sensitive ? 'is-sensitive' : ''].filter(Boolean).join(' ');
  return (
    <div className={cls}>
      {open && <span className="adm-check-mark" aria-hidden="true"><svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2.8" strokeLinecap="round" aria-hidden="true"><path d="M12 6.5v6.5" /><circle cx="12" cy="17.6" r="1.5" fill="currentColor" stroke="none" /></svg></span>}
      <div className="adm-check-body">
        <div className="adm-check-top">
          <b>{checkLabel(c.check_code)}{c.stage && <small> · {c.stage}차 검사</small>}</b>
          <span className="adm-codes">
            {sensitive && <Chip tone="red">2인 승인</Chip>}
            {!open && <StatusChip value={pick(CHECK_STATUS, c.status)} />}
          </span>
        </div>
        <p>{checkSummary(c)}</p>
        <details className="adm-more">
          <summary>자세히</summary>
          <CheckDetail c={c} />
        </details>
      </div>
    </div>
  );
}
