// 발매 상세 > 배급 종료 — 계약 종료·유통사 이전(30일 전 통보)이나 라이선스 만료일을 예약하면, 그날 모든 플랫폼에서
// 내려가고 새 배급이 막히며 90일 안에 최종 정산한다. 예약은 그 전까지 취소할 수 있다 (담당자가 정한 종료는 제외).
// 유통사 이전은 새 유통사가 같은 코드로 배급할 수 있게 이전 자료(UPC·ISRC·플랫폼 ID)를 내려받을 수 있다.
import { useCallback, useEffect, useState } from 'react';
import { orgPath, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { MOCK } from '../lib/mode';
import { useToast } from './Toast';
import { useConfirm } from './Confirm';
import { DateField } from './Picker';
import { ManageHead } from './ManageHead';

interface Term {
  end_date: string; reason: string; note: string; status: string; set_by_staff: boolean; transfer_to?: string | null;
  final_settlement_due: string | null; final_settled_at: string | null; final_note: string;
}
const REASON: Record<string, string> = {
  CONTRACT_END: '배급 계약 종료', LICENSE_EXPIRY: '권리(라이선스) 만료', TERMINATION: '담당자 해지', ORG_CONTRACT_END: '계정 전체 계약 종료',
  TRANSFER: '유통사 이전',
};
// 30일 전 통보가 필요한 사유
const NOTICE = (r: string) => r === 'CONTRACT_END' || r === 'TRANSFER';
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
  const [transferTo, setTransferTo] = useState('');
  const [busy, setBusy] = useState(false);
  const base = useCallback(() => orgPath(`/releases/${encodeURIComponent(releaseId)}/term`), [releaseId]);
  const load = useCallback(async () => {
    if (MOCK) { setTerm(null); return; }
    try { setTerm((await req<{ term: Term | null }>(base(), { quiet401: true })).term); } catch { setTerm(undefined); }
  }, [base]);
  useEffect(() => { void load(); }, [load]);
  if (term === undefined) return null;
  const min = plus(kstToday(), NOTICE(reason) ? 30 : 1);
  const transfer = reason === 'TRANSFER';

  const save = async () => {
    if (!date || date < min) { toast(NOTICE(reason) ? `${transfer ? '유통사 이전' : '계약 종료'}은 30일 뒤 이후 날짜로 골라 주세요.` : '내일 이후 날짜를 골라 주세요.', 'error'); return; }
    if (transfer && !transferTo.trim()) { toast('옮겨 갈 유통사 이름을 적어 주세요.', 'error'); return; }
    const ok = await confirm({
      title: transfer ? '유통사 이전을 예약할까요?' : '배급 종료를 예약할까요?',
      message: transfer
        ? `${date}에 모든 플랫폼에서 내려가요. 새 유통사가 같은 날 배급하도록 이전 자료를 전달해 주세요. 그 전까지는 취소할 수 있어요.`
        : `${date}에 모든 플랫폼에서 내려가고 새 배급이 막혀요. 그 전까지는 취소할 수 있어요.`,
      confirmLabel: transfer ? '이전 예약' : '종료 예약',
    });
    if (!ok) return;
    setBusy(true);
    try {
      await req(base(), { method: 'PUT', body: { end_date: date, reason, note: note.trim(), ...(transfer ? { transfer_to: transferTo.trim() } : {}) } });
      setOpen(false); await load(); toast(transfer ? '유통사 이전을 예약했어요.' : '배급 종료를 예약했어요.', 'success');
    }
    catch (e) { toast(errorMessage(e), 'error'); } finally { setBusy(false); }
  };
  // 새 유통사에 넘길 이전 자료 — UPC·ISRC·플랫폼별 ID를 JSON 파일로
  const downloadHandover = async () => {
    try {
      const data = await req<unknown>(`${base()}/handover`);
      const url = URL.createObjectURL(new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' }));
      const a = document.createElement('a');
      a.href = url; a.download = `handover-${releaseId}.json`; a.click();
      URL.revokeObjectURL(url);
    } catch (e) { toast(errorMessage(e), 'error'); }
  };
  const cancel = async () => {
    try { await req(`${base()}/cancel`, { method: 'POST', body: {} }); await load(); toast('배급 종료 예약을 취소했어요.', 'success'); }
    catch (e) { toast(errorMessage(e), 'error'); }
  };

  return (
    <section className="ui-card aq-manage-card aq-term-card" aria-labelledby="termHeading">
      <ManageHead id="termHeading" icon="calendar" title="배급 종료·권리 만료"
        status={term ? (term.status === 'SCHEDULED' ? { tone: 'amber', label: `${term.end_date} 종료 예정` } : term.status === 'ENDED' ? { tone: 'violet', label: '최종 정산 대기' } : { tone: 'gray', label: '종료' }) : null} />
      {term ? (
        <>
          <dl className="ui-kv is-2">
            <div><dt>종료일</dt><dd>{term.end_date}</dd></div>
            <div><dt>사유</dt><dd>{REASON[term.reason] ?? term.reason}</dd></div>
            {term.transfer_to && <div><dt>옮겨 갈 유통사</dt><dd>{term.transfer_to}</dd></div>}
            {term.final_settlement_due && <div><dt>최종 정산 기한</dt><dd>{term.final_settlement_due}</dd></div>}
          </dl>
          {term.note && <p className="small muted">{term.note}</p>}
          {term.final_note && <p className="small">{term.final_note}</p>}
          {term.reason === 'TRANSFER' && (
            <button type="button" className="ui-btn is-soft is-sm" onClick={() => void downloadHandover()}>이전 자료 받기 (UPC·ISRC·플랫폼 ID)</button>
          )}
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
                <option value="TRANSFER">다른 유통사로 이전 (30일 전 통보)</option>
              </select>
            </div>
            <div className="ui-field">
              <label htmlFor="termDate">종료일</label>
              <DateField id="termDate" min={min} value={date} onChange={setDate} />
            </div>
          </div>
          {transfer && (
            <div className="ui-field">
              <label htmlFor="termTransferTo">옮겨 갈 유통사 <span className="required">*</span></label>
              <input id="termTransferTo" className="ui-input" maxLength={200} value={transferTo} onChange={e => setTransferTo(e.target.value)} placeholder="예: 새 유통사 이름" />
              <p className="ui-field-help">이전일에 모든 플랫폼에서 내려가요. 같은 날 새 유통사가 배급하도록 이전 자료를 전달해 주세요.</p>
            </div>
          )}
          <div className="ui-field">
            <label htmlFor="termNote">메모<span className="aq-optional">선택</span></label>
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
