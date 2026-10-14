// 배급 종료 — 계약 종료·권리 만료·해지 예정과 종료된 발매(최종 정산 대기). 담당자는 해지일을 정하고(사유 필수),
// 마지막 정산을 반영한 뒤 최종 정산 완료로 닫는다. 종료일에는 시스템이 모든 플랫폼에 발매 중단을 보낸다.
import { useState } from 'react';
import { Link } from '../../lib/router';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { req } from '../../api/http';
import { MOCK } from '../../lib/mode';
import { Badge, EmptyState, ErrorBox, NoDuty, PageHeader, SkeletonList, DELIVERY_TABS, SubTabs, useStaff } from '../ui';

interface TermItem {
  release_id: string; title: string; org_name: string; end_date: string; reason: string; note: string; status: string;
  final_settlement_due: string | null; set_by_staff: boolean;
}
const REASON: Record<string, string> = { CONTRACT_END: '계약 종료', LICENSE_EXPIRY: '권리 만료', TERMINATION: '해지', ORG_CONTRACT_END: '계정 계약 종료' };

export function Terms() {
  const toast = useToast();
  const { can } = useStaff();
  const ok = can('DELIVERY');
  const { data, loading, error, reload } = useAsync(
    () => (MOCK ? Promise.resolve({ items: [] as TermItem[] }) : req<{ items: TermItem[] }>('/api/staff/terms')), []);
  const [notes, setNotes] = useState<Record<string, string>>({});
  const [form, setForm] = useState({ release: '', date: '', reason: 'TERMINATION', note: '' });
  const items = data?.items ?? [];
  const today = new Date(Date.now() + 9 * 3600_000).toISOString().slice(0, 10);

  const terminate = async () => {
    try {
      await req(`/api/staff/releases/${encodeURIComponent(form.release.trim())}/term`, { method: 'POST',
        body: { end_date: form.date, reason: form.reason, note: form.note.trim() } });
      toast('종료일을 정했어요. 그날 모든 플랫폼에 발매 중단을 보내요.', 'success');
      setForm({ release: '', date: '', reason: 'TERMINATION', note: '' }); reload();
    } catch (e) { toast(errorMessage(e), 'error'); }
  };
  const settle = async (t: TermItem) => {
    try {
      await req(`/api/staff/releases/${t.release_id}/term/final-settled`, { method: 'POST', body: { note: notes[t.release_id] ?? '' } });
      toast('최종 정산 완료로 기록했어요.', 'success'); reload();
    } catch (e) { toast(errorMessage(e), 'error'); }
  };

  return (
    <div className="view-enter">
      <PageHeader eyebrow="배급" title="배급 종료" sub="계약 종료·권리 만료·해지 예정과, 종료돼 최종 정산을 기다리는 발매예요."
        actions={<button type="button" className="ui-btn is-soft is-sm" onClick={reload}>새로고침</button>} />
      <SubTabs tabs={DELIVERY_TABS} />
      {!ok && <NoDuty duty="배급" />}
      <section className="ui-card" style={{ marginBottom: 16 }}>
        <h2 className="subhead">해지·종료일 정하기</h2>
        <div className="form-grid">
          <div className="ui-field"><label htmlFor="tmRelease">발매 ID</label>
            <input id="tmRelease" className="ui-input" value={form.release} onChange={e => setForm(f => ({ ...f, release: e.target.value }))} placeholder="심사 화면 주소의 발매 ID" /></div>
          <div className="ui-field"><label htmlFor="tmDate">종료일</label>
            <input id="tmDate" type="date" className="ui-input" min={today} value={form.date} onChange={e => setForm(f => ({ ...f, date: e.target.value }))} /></div>
          <div className="ui-field"><label htmlFor="tmReason">사유</label>
            <select id="tmReason" className="ui-select" value={form.reason} onChange={e => setForm(f => ({ ...f, reason: e.target.value }))}>
              <option value="TERMINATION">해지 (계약 위반 등)</option><option value="LICENSE_EXPIRY">권리 만료</option><option value="CONTRACT_END">계약 종료</option>
            </select></div>
        </div>
        <div className="ui-field"><label htmlFor="tmNote">아티스트에게 보일 사유 (해지면 필수)</label>
          <input id="tmNote" className="ui-input" maxLength={1000} value={form.note} onChange={e => setForm(f => ({ ...f, note: e.target.value }))} /></div>
        <div className="adm-form-actions">
          <button type="button" className="ui-btn is-danger" disabled={!ok || !form.release.trim() || !form.date} onClick={() => void terminate()}>종료일 정하기</button>
        </div>
      </section>
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <SkeletonList rows={3} /> : items.length === 0 ? (
        <EmptyState title="예정되거나 정산을 기다리는 종료가 없어요">아티스트가 종료일을 정하거나 담당자가 해지하면 이곳에 올라와요.</EmptyState>
      ) : (
        <div className="ui-list">
          {items.map(t => (
            <div key={t.release_id} className="ui-card">
              <div className="adm-check-top">
                <span>
                  <Link to={`/admin/reviews/${t.release_id}`} className="ui-row-title">{t.title}</Link>
                  <span className="ui-row-meta"><span>{t.org_name}</span><span>{REASON[t.reason] ?? t.reason}</span><span>종료 {t.end_date}</span>
                    {t.final_settlement_due && <span>최종 정산 {t.final_settlement_due}까지</span>}</span>
                </span>
                <Badge tone={t.status === 'ENDED' ? 'amber' : 'blue'}>{t.status === 'ENDED' ? '최종 정산 대기' : '예정'}</Badge>
              </div>
              {t.note && <p className="small muted">{t.note}</p>}
              {t.status === 'ENDED' && (
                <div className="adm-form-actions">
                  <input className="ui-input" aria-label="정산 메모" placeholder="예: 9월분까지 반영" maxLength={1000}
                    value={notes[t.release_id] ?? ''} onChange={e => setNotes(m => ({ ...m, [t.release_id]: e.target.value }))} />
                  <button type="button" className="ui-btn is-primary" disabled={!ok} onClick={() => void settle(t)}>최종 정산 완료</button>
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
