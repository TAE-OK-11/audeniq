// 플랫폼별 배급 상태 — 현재 제출본으로 만든 배급 준비 결과. 막힌 곳을 먼저, 전송 가능한 곳은 접어서 보여 준다.
import type { StagingRow } from './api';
import { Link } from '../lib/router';
import { APPROVAL_STATUS, READINESS, checkLabel, dspLabel, pick } from './labels';
import { Section, StatusChip } from './ui';
import { Glyph } from '../components/Glyph';
import { CheckIcon } from '../components/Check';

const ORDER: Record<string, number> = { CONTENT_BLOCKED: 0, AWAITING_PARTNER: 1, READY: 2 };
const issueText = (c: StagingRow['checks'][number]) => c.message ?? c.detail ?? checkLabel(c.code);
/** 배급 준비 검사 등급 (delivery_staging::Severity) — BLOCKER·WARNING만 문제로 센다, INFO는 참고 */
const SEV_TONE: Record<string, string> = { BLOCKER: 'bad', WARNING: 'warn', INFO: 'info' };
const SEV_ORDER: Record<string, number> = { BLOCKER: 0, WARNING: 1, INFO: 2 };
const isIssue = (c: StagingRow['checks'][number]) => c.severity === 'BLOCKER' || c.severity === 'WARNING';

function Row({ d }: { d: StagingRow }) {
  const notes = d.checks.filter(c => !!c.severity && c.severity in SEV_TONE).sort((a, b) => SEV_ORDER[a.severity!] - SEV_ORDER[b.severity!]);
  const tone = d.readiness === 'CONTENT_BLOCKED' ? 'bad' : d.readiness === 'READY' ? 'ok' : 'wait';
  return (
    <li className={`adm-dsp is-${tone}`}>
      <span className="adm-dsp-mark" aria-hidden="true">
        {tone === 'ok' ? <CheckIcon size={11} /> : tone === 'bad' ? <Glyph name="close" size={10} /> : <Glyph name="dot" size={11} />}
      </span>
      <div className="adm-min">
        <div className="adm-dsp-top">
          <b>{dspLabel(d.dsp)}</b>
          <span className="adm-codes">
            {d.ern_is_preview && <span className="adm-dsp-tag">ERN 미리보기</span>}
            <StatusChip value={pick(READINESS, d.readiness)} />
            <StatusChip value={pick(APPROVAL_STATUS, d.approval)} />
          </span>
        </div>
        {notes.length > 0 && (
          <ul className="adm-dsp-issues">
            {notes.map((c, i) => <li key={`${c.code}-${i}`} className={`is-${SEV_TONE[c.severity!]}`}>{issueText(c)}</li>)}
          </ul>
        )}
        {d.route_reason && <p className="adm-dsp-note">{d.route_reason}</p>}
        {d.approval_note && <p className="adm-dsp-note">담당자 메모 · {d.approval_note}</p>}
      </div>
    </li>
  );
}

export function DeliveryStatus({ rows }: { rows: StagingRow[] }) {
  if (!rows.length) return null;
  const sorted = [...rows].sort((a, b) => (ORDER[a.readiness] ?? 1) - (ORDER[b.readiness] ?? 1) || dspLabel(a.dsp).localeCompare(dspLabel(b.dsp), 'ko'));
  const blocked = rows.filter(r => r.readiness === 'CONTENT_BLOCKED').length;
  const ready = sorted.filter(r => r.readiness === 'READY' && !r.checks.some(isIssue));
  const attention = sorted.filter(r => !ready.includes(r));
  const meta = [`${rows.length}곳`, blocked ? `막힘 ${blocked}` : '', ready.length ? `전송 가능 ${ready.length}` : ''].filter(Boolean).join(' · ');
  return (
    <Section title="플랫폼별 배급 상태" meta={`현재 제출본 · ${meta}`} action={<Link to="/admin/deliveries" className="adm-btn soft small">배급 관리</Link>}>
      {attention.length > 0 && <ul className="adm-dsp-list">{attention.map(d => <Row key={`${d.package_id}-${d.dsp}`} d={d} />)}</ul>}
      {ready.length > 0 && (attention.length ? (
        <details className="adm-dsp-more">
          <summary>문제 없는 플랫폼 {ready.length}곳 <Glyph name="chevron-right" size={13} /></summary>
          <ul className="adm-dsp-list">{ready.map(d => <Row key={`${d.package_id}-${d.dsp}`} d={d} />)}</ul>
        </details>
      ) : <ul className="adm-dsp-list">{ready.map(d => <Row key={`${d.package_id}-${d.dsp}`} d={d} />)}</ul>)}
    </Section>
  );
}
