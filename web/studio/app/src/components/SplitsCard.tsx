// 공동작업자 수익 분배 — 곡별로 함께 만든 사람과 정산 수익(AUDENIQ 수수료 공제 후)을 비율로 나눈다.
// 받는 분에게 이메일로 수락 요청이 가고, 수락하면 그분 AUDENIQ 계정 잔액으로 바로 정산된다 (세금·출금도 각자).
// 수락 전에 들어온 몫은 보관했다가 수락하면 옮기고, 거절하면 신청자에게 돌아온다.
// 발매 신청을 마치면 바꿀 수 없다 (서버가 SPLIT_LOCKED로 막는다).
import { useCallback, useEffect, useState } from 'react';
import { orgPath, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { ReadFailure } from './ReadFailure';
import { MOCK } from '../lib/mode';
import { useToast } from './Toast';
import { ManageHead } from './ManageHead';
import { Badge } from '../ui';

interface Share {
  id: string; name: string; email: string | null; share_bps: number;
  status: 'INVITED' | 'ACCEPTED' | 'DECLINED'; invite_status: string;
  recipient_name: string | null; held: string; transferred: string;
}
interface TrackSheet { track_id: string; title: string; disc: number; number: number; shares: Share[]; owner_bps: number }
interface Sheet { editable: boolean; tracks: TrackSheet[] }
interface Row { id?: string; name: string; email: string; pct: string; status?: Share['status']; masked?: string | null }

const STATUS: Record<Share['status'], string> = { INVITED: '수락 대기', ACCEPTED: '수락', DECLINED: '거절' };
const TONE: Record<Share['status'], 'violet' | 'green' | 'red'> = { INVITED: 'violet', ACCEPTED: 'green', DECLINED: 'red' };
const won = (v: string) => `${Number(v).toLocaleString()}원`;
const toBps = (pct: string) => Math.round(Number(pct.replace(',', '.')) * 100);
const fromBps = (bps: number) => String(bps / 100);
const pctLabel = (bps: number) => `${(bps / 100).toLocaleString(undefined, { maximumFractionDigits: 2 })}%`;

export function SplitsCard({ releaseId, embedded = false }: { releaseId: string; embedded?: boolean }) {
  const toast = useToast();
  const [loadFailed, setLoadFailed] = useState(false);
  const [sheet, setSheet] = useState<Sheet | null>(null);
  const [rows, setRows] = useState<Record<string, Row[]>>({});
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const base = useCallback(() => orgPath(`/releases/${encodeURIComponent(releaseId)}/splits`), [releaseId]);
  const load = useCallback(async () => {
    if (MOCK) return;
    try {
      const s = await req<Sheet>(base(), { quiet401: true });
      setSheet(s);
      setRows(Object.fromEntries(s.tracks.map(t => [t.track_id, t.shares.map(sh => ({
        id: sh.id, name: sh.name, email: '', pct: fromBps(sh.share_bps), status: sh.status, masked: sh.email,
      }))])));
      setDirty(false);
      setLoadFailed(false);
    } catch { setLoadFailed(true); }
  }, [base]);
  useEffect(() => { void load(); }, [load]);
  if (loadFailed) return <ReadFailure label="수익 분배" retry={load} />;
  if (!sheet || !sheet.tracks.length) return null;
  const anyShare = sheet.tracks.some(t => t.shares.length);
  if (!sheet.editable && !anyShare) return null;

  const edit = (track: string, fn: (r: Row[]) => Row[]) => { setRows(m => ({ ...m, [track]: fn(m[track] ?? []) })); setDirty(true); };
  // 저장 검사는 서버와 같게 거절한 분 비율도 합에 넣는다 (지우면 그 비율을 다른 분에게 줄 수 있다)
  const ownerBps = (track: string) => 10000 - (rows[track] ?? []).reduce((a, r) => a + (toBps(r.pct) || 0), 0);

  const save = async () => {
    for (const t of sheet.tracks) {
      for (const r of rows[t.track_id] ?? []) {
        if (!r.name.trim()) { toast(`'${t.title}' 공동작업자 이름을 적어 주세요.`, 'error'); return; }
        if (!r.id && !r.email.trim()) { toast(`'${t.title}' ${r.name}님의 이메일을 적어 주세요.`, 'error'); return; }
        const bps = toBps(r.pct);
        if (!(bps >= 1 && bps <= 10000)) { toast(`'${t.title}' ${r.name}님의 비율을 0.01–100% 사이로 적어 주세요.`, 'error'); return; }
      }
      if (ownerBps(t.track_id) < 0) { toast(`'${t.title}' 분배 합계가 100%를 넘어요.`, 'error'); return; }
    }
    setBusy(true);
    try {
      const tracks = sheet.tracks.map(t => ({
        track_id: t.track_id,
        shares: (rows[t.track_id] ?? []).map(r => ({
          ...(r.id ? { id: r.id } : {}), name: r.name.trim(), share_bps: toBps(r.pct),
          ...(r.email.trim() ? { email: r.email.trim() } : {}),
        })),
      }));
      const v = await req<{ invited: number }>(base(), { method: 'PUT', body: { tracks } });
      toast(v.invited ? `저장했어요. ${v.invited}명에게 수락 요청 메일을 보내요.` : '저장했어요.', 'success');
      await load();
    } catch (e) {
      toast(errorMessage(e), 'error');
    } finally {
      setBusy(false);
    }
  };
  const resend = async (share: string) => {
    try { await req(`${base()}/${encodeURIComponent(share)}/resend`, { method: 'POST', body: {} }); toast('수락 요청 메일을 다시 보내요.', 'success'); }
    catch (e) { toast(errorMessage(e), 'error'); }
  };

  return (
    <section className={embedded ? 'aq-splits is-embedded' : 'ui-card aq-manage-card aq-splits'} aria-labelledby="splitsHeading">
      {embedded ? <h2 id="splitsHeading" className="subhead">공동작업자 수익 분배</h2> : <ManageHead id="splitsHeading" icon="users" title="공동작업자 수익 분배" />}
      <p className="ui-field-help">
        곡을 함께 만든 분과 정산 수익(AUDENIQ 수수료 공제 후)을 비율로 나눌 수 있어요. 받는 분에게 이메일로 수락 요청이 가고,
        수락하면 그분 계정 잔액으로 바로 정산돼요(출금·세금도 각자). 수락 전 몫은 보관했다가 수락하면 옮기고, 거절하면 내 정산으로 돌아와요.
        {sheet.editable ? ' 발매 신청을 마치면 바꿀 수 없어요.' : ''}
      </p>
      {sheet.tracks.map(t => {
        const list = rows[t.track_id] ?? [];
        const server = new Map(t.shares.map(s => [s.id, s]));
        const mine = ownerBps(t.track_id);
        if (!sheet.editable && !list.length) return null;
        return (
          <fieldset key={t.track_id} className="aq-splits-track">
            <legend>{sheet.tracks.length > 1 ? `${t.number}. ` : ''}{t.title}</legend>
            {list.map((r, i) => {
              const sv = r.id ? server.get(r.id) : undefined;
              const needsMail = !!r.id && r.status !== 'INVITED';
              return (
                <div key={r.id ?? `new-${i}`} className="aq-splits-row">
                  {sheet.editable ? (
                    <>
                      <input className="ui-input" aria-label="이름" maxLength={100} placeholder="이름·활동명" value={r.name}
                        onChange={e => edit(t.track_id, l => l.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))} />
                      <input className="ui-input" type="email" aria-label="이메일" maxLength={254}
                        placeholder={r.id ? (r.masked ?? (needsMail ? '다시 요청하려면 이메일' : '')) : '이메일'}
                        value={r.email} onChange={e => edit(t.track_id, l => l.map((x, j) => (j === i ? { ...x, email: e.target.value } : x)))} />
                      <span className="aq-splits-pct">
                        <input className="ui-input" inputMode="decimal" aria-label="비율(%)" value={r.pct}
                          onChange={e => edit(t.track_id, l => l.map((x, j) => (j === i ? { ...x, pct: e.target.value.replace(/[^0-9.]/g, '') } : x)))} />%
                      </span>
                      <button type="button" className="ui-btn is-text is-sm" onClick={() => edit(t.track_id, l => l.filter((_, j) => j !== i))}>삭제</button>
                    </>
                  ) : (
                    <>
                      <span className="aq-splits-name">{r.name}{sv?.recipient_name && <span className="muted"> · {sv.recipient_name}</span>}</span>
                      <span className="aq-splits-share">{pctLabel(toBps(r.pct))}</span>
                      <Badge tone={TONE[r.status ?? 'INVITED']} className="aq-splits-status">{STATUS[r.status ?? 'INVITED']}</Badge>
                      <span className="aq-splits-money small muted">
                        {sv && Number(sv.transferred) > 0 && <>전달 {won(sv.transferred)} </>}
                        {sv && Number(sv.held) > 0 && <>보관 {won(sv.held)}</>}
                        {r.status === 'INVITED' && sv && <button type="button" className="ui-btn is-text is-sm" onClick={() => void resend(sv.id)}>요청 다시 보내기</button>}
                      </span>
                    </>
                  )}
                  {sheet.editable && r.id && r.status && <Badge tone={TONE[r.status]} className="aq-splits-status">{STATUS[r.status]}</Badge>}
                </div>
              );
            })}
            <div className="aq-splits-foot">
              <span className={mine < 0 ? 'aq-splits-over' : 'muted small'}>
                나 {pctLabel(Math.max(t.shares.length && !dirty ? t.owner_bps : mine, 0))}{mine < 0 ? ' · 합계가 100%를 넘어요' : ''}
                {sheet.editable && list.some(r => r.status === 'DECLINED') && ' · 거절한 분을 삭제하면 그 비율을 다른 분에게 줄 수 있어요'}
              </span>
              {sheet.editable && list.length < 10 && (
                <button type="button" className="ui-btn is-soft is-sm"
                  onClick={() => edit(t.track_id, l => [...l, { name: '', email: '', pct: '' }])}>공동작업자 추가</button>
              )}
            </div>
          </fieldset>
        );
      })}
      {sheet.editable && (
        <div className="ui-actions">
          <button type="button" className="ui-btn is-primary" disabled={busy || !dirty} onClick={() => void save()}>{busy ? '저장 중' : '분배 저장'}</button>
        </div>
      )}
    </section>
  );
}
