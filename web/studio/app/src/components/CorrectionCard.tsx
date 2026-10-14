// 발매 후 정보 수정 — 플랫폼에 나간 뒤에도 문제없이 바꿀 수 있는 것만: 곡별 크레딧(작곡·작사·편곡·프로듀서 등)과 장르.
// 제목·아티스트·피처링·음원·발매일·권리 표기는 바꿀 수 없다 (새 발매로 신청).
// 담당자가 확인하면 그 발매가 나간 모든 플랫폼에 수정 내용을 보낸다.
import { useCallback, useEffect, useState } from 'react';
import { orgPath, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { MOCK } from '../lib/mode';
import { useToast } from './Toast';

interface Credit { name: string; role: string }
interface TrackRow { track_id: string; title: string; credits: Credit[] }
interface State {
  can_request: boolean; genre: string | null; tracks: TrackRow[];
  request: { id: string; status: string; reason: string; decision_note: string; created_at: string } | null;
}

const STATUS: Record<string, string> = {
  PENDING: '담당자가 확인하고 있어요.',
  APPROVED: '플랫폼에 수정 내용을 보냈어요. 반영되는 데 며칠 걸릴 수 있어요.',
  DECLINED: '요청이 반려됐어요.',
  CANCELLED: '요청을 취소했어요.',
};
// 아티스트·피처링 표기는 플랫폼의 아티스트 정보라 여기서 바꾸지 않는다
const LOCKED = /^(artist|main artist|featured artist|featuring|feat\.?)$/i;
const ROLES = ['Composer', 'Lyricist', 'Arranger', 'Producer', 'Mixing Engineer', 'Mastering Engineer', 'Recording Engineer', 'Musician'];
const ROLE_KO: Record<string, string> = {
  Composer: '작곡', Lyricist: '작사', Arranger: '편곡', Producer: '프로듀서', 'Mixing Engineer': '믹싱',
  'Mastering Engineer': '마스터링', 'Recording Engineer': '녹음', Musician: '연주',
};

export function CorrectionCard({ releaseId }: { releaseId: string }) {
  const toast = useToast();
  const [st, setSt] = useState<State | null>(null);
  const [open, setOpen] = useState(false);
  const [genre, setGenre] = useState('');
  const [tracks, setTracks] = useState<TrackRow[]>([]);
  const [reason, setReason] = useState('');
  const [busy, setBusy] = useState(false);
  const base = useCallback(() => orgPath(`/releases/${encodeURIComponent(releaseId)}/corrections`), [releaseId]);
  const load = useCallback(async () => {
    if (MOCK) return;
    try { setSt(await req<State>(base(), { quiet401: true })); } catch { /* 볼 수 없으면 숨긴다 */ }
  }, [base]);
  useEffect(() => { void load(); }, [load]);
  if (!st || (!st.can_request && !st.request)) return null;

  const start = () => {
    setGenre(st.genre ?? '');
    setTracks(st.tracks.map(t => ({ ...t, credits: t.credits.filter(c => !LOCKED.test(c.role)) })));
    setReason('');
    setOpen(true);
  };
  const setCredit = (ti: number, ci: number, patch: Partial<Credit>) =>
    setTracks(ts => ts.map((t, i) => (i !== ti ? t : { ...t, credits: t.credits.map((c, j) => (j === ci ? { ...c, ...patch } : c)) })));
  const submit = async () => {
    if (!reason.trim()) { toast('수정하는 이유를 적어 주세요.', 'error'); return; }
    const original = new Map(st.tracks.map(t => [t.track_id, JSON.stringify(t.credits.filter(c => !LOCKED.test(c.role)))]));
    const changedTracks = tracks
      .map(t => ({ track_id: t.track_id, credits: t.credits.map(c => ({ name: c.name.trim(), role: c.role.trim() })).filter(c => c.name && c.role) }))
      .filter(t => original.get(t.track_id) !== JSON.stringify(t.credits));
    const changes: Record<string, unknown> = {};
    if (genre.trim() && genre.trim() !== (st.genre ?? '')) changes.genre = genre.trim();
    if (changedTracks.length) changes.tracks = changedTracks;
    if (!Object.keys(changes).length) { toast('바뀐 내용이 없어요.', 'error'); return; }
    setBusy(true);
    try {
      await req(base(), { method: 'POST', body: { changes, reason: reason.trim() } });
      toast('수정 요청을 보냈어요. 담당자가 확인한 뒤 알려 드려요.', 'success');
      setOpen(false);
      await load();
    } catch (e) {
      toast(errorMessage(e), 'error');
    } finally {
      setBusy(false);
    }
  };
  const cancel = async () => {
    try { await req(`${base()}/cancel`, { method: 'POST', body: {} }); await load(); } catch (e) { toast(errorMessage(e), 'error'); }
  };

  return (
    <section className="ui-card aq-correction-card" aria-labelledby="correctionHeading">
      <h2 id="correctionHeading">발매 후 정보 수정</h2>
      <p className="ui-field-help">크레딧(작곡·작사·편곡 등)과 장르만 고칠 수 있어요. 제목·아티스트·음원을 바꾸려면 새로 발매해 주세요.</p>
      {st.request && (
        <p className="aq-claim-status"><b>{STATUS[st.request.status] ?? st.request.status}</b>
          {st.request.decision_note && <span className="muted"> · {st.request.decision_note}</span>}
          {st.request.status === 'PENDING' && <button type="button" className="ui-btn is-text is-sm" onClick={() => void cancel()}>요청 취소</button>}
        </p>
      )}
      {st.can_request && !open && <button type="button" className="ui-btn is-soft" onClick={start}>정보 수정 요청</button>}
      {open && (
        <div className="aq-correction-form">
          <div className="ui-field">
            <label htmlFor="corrGenre">장르</label>
            <input id="corrGenre" className="ui-input" maxLength={60} value={genre} onChange={e => setGenre(e.target.value)} />
          </div>
          {tracks.map((t, ti) => (
            <fieldset key={t.track_id} className="aq-correction-track">
              <legend>{t.title}</legend>
              {t.credits.map((c, ci) => (
                <div key={ci} className="aq-correction-row">
                  <input className="ui-input" aria-label="이름" maxLength={200} value={c.name} onChange={e => setCredit(ti, ci, { name: e.target.value })} placeholder="이름" />
                  <select className="ui-select" aria-label="역할" value={c.role} onChange={e => setCredit(ti, ci, { role: e.target.value })}>
                    {!ROLES.includes(c.role) && <option value={c.role}>{c.role}</option>}
                    {ROLES.map(r => <option key={r} value={r}>{ROLE_KO[r]}</option>)}
                  </select>
                  <button type="button" className="ui-btn is-text is-sm" onClick={() => setTracks(ts => ts.map((x, i) => (i === ti ? { ...x, credits: x.credits.filter((_, j) => j !== ci) } : x)))}>삭제</button>
                </div>
              ))}
              <button type="button" className="ui-btn is-soft is-sm" onClick={() => setTracks(ts => ts.map((x, i) => (i === ti ? { ...x, credits: [...x.credits, { name: '', role: 'Composer' }] } : x)))}>크레딧 추가</button>
            </fieldset>
          ))}
          <div className="ui-field">
            <label htmlFor="corrReason">수정하는 이유</label>
            <textarea id="corrReason" rows={3} maxLength={1000} value={reason} onChange={e => setReason(e.target.value)} placeholder="예: 작곡가 이름 철자가 틀렸어요" />
          </div>
          <div className="aq-confirm-actions">
            <button type="button" className="ui-btn is-soft" onClick={() => setOpen(false)} disabled={busy}>닫기</button>
            <button type="button" className="ui-btn is-primary" onClick={() => void submit()} disabled={busy}>{busy ? '보내는 중' : '수정 요청'}</button>
          </div>
        </div>
      )}
    </section>
  );
}
