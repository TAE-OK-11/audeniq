// 발매 상세 > 배급 종료 — 계약 종료(30일 전 통보)나 라이선스 만료일을 예약하면, 그날 모든 플랫폼에서 내려가고
// 새 배급이 막히며 90일 안에 최종 정산한다. 예약은 그 전까지 취소할 수 있다 (담당자가 정한 종료는 제외).
import { useCallback, useEffect, useState } from 'react';
import { orgPath, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { MOCK } from '../lib/mode';
import { useToast } from './Toast';
import { useConfirm } from './Confirm';

interface Term {
  end_date: string; reason: string; note: string; status: string; set_by_staff: boolean;
  final_settlement_due: string | null; final_settled_at: string | null; final_note: string;
}
const REASON: Record<string, string> = {
  CONTRACT_END: '배급 계약 종료', LICENSE_EXPIRY: '권리(라이선스) 만료', TERMINATION: '담당자 해지', ORG_CONTRACT_END: '계정 전체 계약 종료',
};
const kstToday = () => new Date(Date.now() + 9 * 3600_000).toISOString().slice(0, 10);
const plus = (d: string, n: number) => { const t = new Date(`${d}T00:00:00Z`); t.setUTCDate(t.getUTCDate() + n); return t.toISOString().slice(0, 10); };

export function TermCard({ releaseId }: { releaseId: string }) {
  const toast = useToast();
  const confirm = useConfirm();
  const [term, setTerm] = useState<Term | null | undefined>(undefined);
  const [open, setOpen] = useState(false);
  const [reason, setReason] = useState('CONTRACT_END');
  const [date, setDate] = useState('');
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  const base = useCallback(() => orgPath(`/releases/${encodeURIComponent(releaseId)}/term`), [releaseId]);
  const load = useCallback(async () => {
    if (MOCK) { setTerm(null); return; }
    try { setTerm((await req<{ term: Term | null }>(base(), { quiet401: true })).term); } catch { setTerm(undefined); }
  }, [base]);
  useEffect(() => { void load(); }, [load]);
  if (term === undefined) return null;
  const min = plus(kstToday(), reason === 'CONTRACT_END' ? 30 : 1);

  const save = async () => {
    if (!date || date < min) { toast(reason === 'CONTRACT_END' ? '계약 종료는 30일 뒤 이후 날짜로 골라 주세요.' : '내일 이후 날짜를 골라 주세요.', 'error'); return; }
    const ok = await confirm({ title: '배급 종료를 예약할까요?', message: `${date}에 모든 플랫폼에서 내려가고 새 배급이 막혀요. 그 전까지는 취소할 수 있어요.`, confirmLabel: '종료 예약' });
    if (!ok) return;
    setBusy(true);
    try { await req(base(), { method: 'PUT', body: { end_date: date, reason, note: note.trim() } }); setOpen(false); await load(); toast('배급 종료를 예약했어요.', 'success'); }
    catch (e) { toast(errorMessage(e), 'error'); } finally { setBusy(false); }
  };
  const cancel = async () => {
    try { await req(`${base()}/cancel`, { method: 'POST', body: {} }); await load(); toast('배급 종료 예약을 취소했어요.', 'success'); }
    catch (e) { toast(errorMessage(e), 'error'); }
  };

  return (
    <section className="ui-card aq-term-card" aria-labelledby="termHeading">
      <h2 id="termHeading">배급 종료·권리 만료</h2>
      {term ? (
        <>
          <dl className="ui-kv is-2">
            <div><dt>종료일</dt><dd>{term.end_date}</dd></div>
            <div><dt>사유</dt><dd>{REASON[term.reason] ?? term.reason}</dd></div>
            <div><dt>상태</dt><dd>{term.status === 'SCHEDULED' ? '예약됨' : term.status === 'ENDED' ? '종료 · 최종 정산 대기' : '최종 정산 완료'}</dd></div>
            {term.final_settlement_due && <div><dt>최종 정산 기한</dt><dd>{term.final_settlement_due}</dd></div>}
          </dl>
          {term.note && <p className="small muted">{term.note}</p>}
          {term.final_note && <p className="small">{term.final_note}</p>}
          {term.status === 'SCHEDULED' && !term.set_by_staff && term.reason !== 'ORG_CONTRACT_END' && (
            <button type="button" className="ui-btn is-text is-sm" onClick={() => void cancel()}>종료 예약 취소</button>
          )}
        </>
      ) : !open ? (
        <>
          <p className="ui-field-help">배급을 끝내거나 라이선스가 만료되는 날을 정하면, 그날 모든 플랫폼에서 내려가고 90일 안에 최종 정산해요.</p>
          <button type="button" className="ui-btn is-soft" onClick={() => setOpen(true)}>종료일 정하기</button>
        </>
      ) : (
        <div className="aq-correction-form">
          <div className="form-grid">
            <div className="ui-field">
              <label htmlFor="termReason">사유</label>
              <select id="termReason" className="ui-select" value={reason} onChange={e => setReason(e.target.value)}>
                <option value="CONTRACT_END">배급 계약 종료 (30일 전 통보)</option>
                <option value="LICENSE_EXPIRY">권리(라이선스) 만료</option>
              </select>
            </div>
            <div className="ui-field">
              <label htmlFor="termDate">종료일</label>
              <input id="termDate" type="date" className="ui-input" min={min} value={date} onChange={e => setDate(e.target.value)} />
            </div>
          </div>
          <div className="ui-field">
            <label htmlFor="termNote">메모 (선택)</label>
            <input id="termNote" className="ui-input" maxLength={1000} value={note} onChange={e => setNote(e.target.value)} placeholder="예: 샘플 사용 허락 기간 만료" />
          </div>
          <div className="aq-confirm-actions">
            <button type="button" className="ui-btn is-soft" onClick={() => setOpen(false)} disabled={busy}>닫기</button>
            <button type="button" className="ui-btn is-primary" onClick={() => void save()} disabled={busy}>{busy ? '저장 중' : '종료 예약'}</button>
          </div>
        </div>
      )}
    </section>
  );
}
