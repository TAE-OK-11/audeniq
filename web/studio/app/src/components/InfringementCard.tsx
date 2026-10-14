// 권리침해 신고 — 이 발매에 들어온 신고와 처리 상태. 배급이 중단되면 기한 안에 소명할 수 있다.
// 신고한 사람의 연락처는 보이지 않는다 (담당자가 연락한다).
import { useCallback, useEffect, useState } from 'react';
import { orgPath, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { MOCK } from '../lib/mode';
import { useToast } from './Toast';

interface Claim {
  id: string; receipt: string; status: string; right_type: string; work: string; target: string;
  staff_note: string; counter_deadline: string | null; counter_notice: string | null; can_answer: boolean; created_at: string;
}

const STATUS: Record<string, string> = {
  SUSPENDED: '배급 중단 · 소명할 수 있어요',
  COUNTER_NOTICED: '소명 검토 중',
  RESTORED: '신고가 받아들여지지 않았어요',
  UPHELD: '신고가 인정됐어요',
};
const RIGHT: Record<string, string> = {
  COPYRIGHT: '저작권', NEIGHBORING: '저작인접권', TRADEMARK: '상표권', PUBLICITY: '초상·성명', OTHER: '기타',
};
const day = (v: string | null) => (v ? new Date(v).toLocaleDateString('ko-KR', { timeZone: 'Asia/Seoul' }) : '');

export function InfringementCard({ releaseId }: { releaseId: string }) {
  const toast = useToast();
  const [items, setItems] = useState<Claim[]>([]);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState('');
  const load = useCallback(async () => {
    if (MOCK) return;
    try {
      const r = await req<{ items: Claim[] }>(orgPath(`/releases/${encodeURIComponent(releaseId)}/infringement-claims`), { quiet401: true });
      setItems(r.items);
    } catch { /* 신고가 없거나 볼 수 없으면 아무것도 표시하지 않는다 */ }
  }, [releaseId]);
  useEffect(() => { void load(); }, [load]);
  if (!items.length) return null;

  const answer = async (c: Claim) => {
    const statement = (drafts[c.id] ?? '').trim();
    if (!statement) { toast('소명 내용을 적어 주세요.', 'error'); return; }
    setBusy(c.id);
    try {
      await req(orgPath(`/infringement-claims/${encodeURIComponent(c.id)}/counter`), { method: 'POST', body: { statement } });
      toast('소명을 보냈어요. 담당자가 검토한 뒤 알려 드려요.', 'success');
      await load();
    } catch (e) {
      toast(errorMessage(e), 'error');
    } finally {
      setBusy('');
    }
  };

  return (
    <section className="ui-card aq-claim-card" aria-labelledby="claimHeading">
      <h2 id="claimHeading">권리침해 신고</h2>
      {items.map(c => (
        <div key={c.id} className="aq-claim">
          <p className="aq-claim-status"><b>{STATUS[c.status] ?? c.status}</b> <span className="muted">· 접수 {c.receipt} · {day(c.created_at)}</span></p>
          <dl className="ui-kv">
            <div><dt>침해 주장 권리</dt><dd>{RIGHT[c.right_type] ?? c.right_type}</dd></div>
            <div><dt>권리 대상 저작물</dt><dd>{c.work}</dd></div>
            <div><dt>신고 대상</dt><dd>{c.target}</dd></div>
            {c.staff_note && <div><dt>담당자 메모</dt><dd>{c.staff_note}</dd></div>}
            {c.counter_notice && <div><dt>보낸 소명</dt><dd>{c.counter_notice}</dd></div>}
          </dl>
          {c.can_answer && (
            <div className="ui-field">
              <label htmlFor={`claim-${c.id}`}>소명 ({day(c.counter_deadline)}까지)</label>
              <textarea
                id={`claim-${c.id}`} rows={4} maxLength={3000} value={drafts[c.id] ?? ''}
                onChange={e => setDrafts(d => ({ ...d, [c.id]: e.target.value }))}
                placeholder="직접 창작했거나 이용 허락을 받았다면 그 근거를 적어 주세요. 계약서 등 서류는 문의로 보내 주세요."
              />
              <div className="aq-confirm-actions">
                <button type="button" className="ui-btn is-primary" disabled={busy === c.id} onClick={() => void answer(c)}>
                  {busy === c.id ? '보내는 중' : '소명 보내기'}
                </button>
              </div>
            </div>
          )}
        </div>
      ))}
    </section>
  );
}
