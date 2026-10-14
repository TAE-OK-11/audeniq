// 계약서 > 배급 계약 종료 — 계정 소유자가 전체 배급 계약 종료일(30일 전 통보)을 정한다. 그날 모든 발매가 내려가고
// 새 발매 신청을 받지 않으며 90일 안에 최종 정산한다. 그 전까지 취소할 수 있다.
import { useCallback, useEffect, useState } from 'react';
import { orgPath, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { useAuth } from '../api/auth';
import { MOCK } from '../lib/mode';
import { useToast } from './Toast';
import { useConfirm } from './Confirm';

interface OrgTerm { end_date: string; note: string; status: string; set_by_staff: boolean }
interface ReleaseTerm { release_id: string; title: string; end_date: string; reason: string; status: string; final_settlement_due: string | null }
const STATUS: Record<string, string> = { SCHEDULED: '예약', ENDED: '종료 · 최종 정산 대기', FINAL_SETTLED: '최종 정산 완료' };
const plus = (n: number) => new Date(Date.now() + 9 * 3600_000 + n * 86400_000).toISOString().slice(0, 10);

export function OrgContractEnd() {
  const toast = useToast();
  const confirm = useConfirm();
  const { org } = useAuth();
  const [data, setData] = useState<{ term: OrgTerm | null; releases: ReleaseTerm[] } | null>(null);
  const [date, setDate] = useState('');
  const [open, setOpen] = useState(false);
  const load = useCallback(async () => {
    if (MOCK) return;
    try { setData(await req(orgPath('/contract-end'), { quiet401: true })); } catch { setData(null); }
  }, []);
  useEffect(() => { void load(); }, [load]);
  if (!data) return null;
  const owner = !org?.role || org.role === 'OWNER';
  const t = data.term;

  const save = async () => {
    if (!date || date < plus(30)) { toast('계약 종료일은 30일 뒤 이후로 골라 주세요.', 'error'); return; }
    const ok = await confirm({ title: '배급 계약을 종료할까요?', message: `${date}에 모든 발매가 플랫폼에서 내려가고, 새 발매 신청을 받지 않아요. 남은 수익은 그 뒤 90일 안에 최종 정산해요.`, confirmLabel: '계약 종료 예약', danger: true });
    if (!ok) return;
    try { await req(orgPath('/contract-end'), { method: 'PUT', body: { end_date: date } }); setOpen(false); await load(); }
    catch (e) { toast(errorMessage(e), 'error'); }
  };
  const cancel = async () => {
    try { await req(orgPath('/contract-end/cancel'), { method: 'POST', body: {} }); await load(); toast('계약 종료를 취소했어요.', 'success'); }
    catch (e) { toast(errorMessage(e), 'error'); }
  };

  return (
    <section className="ui-card aq-term-card" aria-labelledby="orgTermHeading">
      <h2 id="orgTermHeading" className="subhead">배급 계약 종료</h2>
      {t ? (
        <p>{t.status === 'ENDED' ? `${t.end_date}에 배급 계약이 끝났어요.` : `${t.end_date}에 배급 계약이 끝나요. 새 발매 신청은 받지 않아요.`}
          {t.status === 'SCHEDULED' && owner && !t.set_by_staff && <button type="button" className="ui-btn is-text is-sm" onClick={() => void cancel()}>종료 취소</button>}</p>
      ) : owner && (open ? (
        <div className="aq-confirm-actions" style={{ justifyContent: 'flex-start' }}>
          <input type="date" className="ui-input" aria-label="계약 종료일" min={plus(30)} value={date} onChange={e => setDate(e.target.value)} style={{ maxWidth: 200 }} />
          <button type="button" className="ui-btn is-soft" onClick={() => setOpen(false)}>닫기</button>
          <button type="button" className="ui-btn is-primary" onClick={() => void save()}>종료 예약</button>
        </div>
      ) : (
        <p className="small muted">배급을 그만두려면 30일 전에 종료일을 정해 주세요. <button type="button" className="ui-btn is-text is-sm" onClick={() => setOpen(true)}>종료일 정하기</button></p>
      ))}
      {data.releases.length > 0 && (
        <ul className="ui-list">
          {data.releases.map(r => (
            <li key={r.release_id} className="ui-row">
              <span className="min-0"><span className="ui-row-title">{r.title}</span>
                <span className="ui-row-meta"><span>종료 {r.end_date}</span>{r.final_settlement_due && <span>최종 정산 {r.final_settlement_due}까지</span>}</span></span>
              <span className="small">{STATUS[r.status] ?? r.status}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
