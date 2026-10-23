// 플레이리스트 피칭 — 발매일 7일 전까지 대표곡 하나를 플랫폼 에디터에게 소개한다.
// 담당자가 확인해 플랫폼에 전달하고, 선정 여부는 플랫폼이 정한다. 단계마다 알림이 간다.
import { useCallback, useEffect, useState } from 'react';
import { orgPath, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { MOCK } from '../lib/mode';
import { useToast } from './Toast';
import { ManageHead } from './ManageHead';

interface Pitch {
  id: string; track_id: string; track_title: string; description: string; status: string;
  platforms: string[]; result: string; staff_note: string; created_at: string;
}
interface State {
  pitch: Pitch | null; tracks: { id: string; title: string }[];
  can_pitch: boolean; days_until_release: number | null; lead_days: number;
}

const STATUS: Record<string, string> = {
  SUBMITTED: '신청을 받았어요. 담당자가 확인하고 있어요.',
  IN_REVIEW: '담당자가 검토하고 있어요.',
  PITCHED: '플랫폼 에디터에게 전달했어요. 선정 여부는 플랫폼이 정해요.',
  SELECTED: '플레이리스트에 선정됐어요!',
  NOT_SELECTED: '이번에는 선정되지 않았어요.',
  WITHDRAWN: '피칭을 취소했어요.',
};
/** 머리 상태 배지 — 짧게 */
const BADGE: Record<string, { tone: 'blue' | 'violet' | 'green' | 'gray'; label: string }> = {
  SUBMITTED: { tone: 'violet', label: '접수' }, IN_REVIEW: { tone: 'violet', label: '검토 중' },
  PITCHED: { tone: 'blue', label: '플랫폼 전달' }, SELECTED: { tone: 'green', label: '선정' },
  NOT_SELECTED: { tone: 'gray', label: '미선정' },
};
const PLATFORM_KO: Record<string, string> = {
  spotify: 'Spotify', apple: 'Apple Music', melon: '멜론', genie: '지니', bugs: '벅스', flo: 'FLO', vibe: 'VIBE', youtube: 'YouTube Music',
};

const EMPTY = { description: '', genres: '', moods: '', instruments: '', similar_artists: '', marketing: '' };

export function PitchCard({ releaseId }: { releaseId: string }) {
  const toast = useToast();
  const [st, setSt] = useState<State | null>(null);
  const [open, setOpen] = useState(false);
  const [track, setTrack] = useState('');
  const [f, setF] = useState(EMPTY);
  const [busy, setBusy] = useState(false);
  const base = useCallback(() => orgPath(`/releases/${encodeURIComponent(releaseId)}/pitch`), [releaseId]);
  const load = useCallback(async () => {
    if (MOCK) return;
    try { setSt(await req<State>(base(), { quiet401: true })); } catch { /* 볼 수 없으면 숨긴다 */ }
  }, [base]);
  useEffect(() => { void load(); }, [load]);
  if (!st) return null;
  const p = st.pitch;
  const active = p && p.status !== 'WITHDRAWN';
  // 철회한 피칭은 없는 것과 같다 — 다시 피칭할 수 없는 이유(마감)를 보여 준다
  const tooLate = !active && !st.can_pitch && st.days_until_release !== null && st.days_until_release < st.lead_days;
  if (!p && !st.can_pitch && !tooLate) return null;

  const start = () => { setTrack(st.tracks[0]?.id ?? ''); setF(EMPTY); setOpen(true); };
  const set = (k: keyof typeof EMPTY) => (e: { target: { value: string } }) => setF(v => ({ ...v, [k]: e.target.value }));
  const submit = async () => {
    if (f.description.trim().length < 20) { toast('곡 소개를 20자 이상 적어 주세요.', 'error'); return; }
    setBusy(true);
    try {
      await req(base(), { method: 'POST', body: { track_id: track, ...f } });
      toast('피칭을 보냈어요. 진행 상황은 알림으로 알려 드려요.', 'success');
      setOpen(false);
      await load();
    } catch (e) {
      toast(errorMessage(e), 'error');
    } finally {
      setBusy(false);
    }
  };
  const withdraw = async () => {
    try { await req(`${base()}/withdraw`, { method: 'POST', body: {} }); await load(); } catch (e) { toast(errorMessage(e), 'error'); }
  };
  return (
    <section className="ui-card aq-manage-card aq-pitch-card" aria-labelledby="pitchHeading">
      <ManageHead id="pitchHeading" icon="star" title="플레이리스트 피칭" status={active ? BADGE[p.status] ?? null : null} />
      <p className="ui-field-help">대표곡 하나를 플랫폼 에디터에게 소개해요. 발매일 {st.lead_days}일 전까지 보내야 하고, 선정 여부는 플랫폼이 정해요.</p>
      {active && (
        <div className="aq-claim-status">
          <p className="small"><b>{p.track_title}</b> <span className="muted">· {STATUS[p.status] ?? p.status}</span>
            {(p.status === 'SUBMITTED' || p.status === 'IN_REVIEW') && <button type="button" className="ui-btn is-text is-sm" onClick={() => void withdraw()}>피칭 취소</button>}
          </p>
          {p.platforms.length > 0 && <p className="small muted">전달한 플랫폼: {p.platforms.map(x => PLATFORM_KO[x] ?? x).join(', ')}</p>}
          {p.result && <p className="small">{p.result}</p>}
          {p.staff_note && <p className="small muted">{p.staff_note}</p>}
        </div>
      )}
      {tooLate && <p className="small muted">발매일까지 {Math.max(st.days_until_release ?? 0, 0)}일 남아 이번 발매는 피칭할 수 없어요. 다음 발매는 일찍 신청해 주세요.</p>}
      {st.can_pitch && !open && <button type="button" className="ui-btn is-primary" onClick={start}>피칭하기</button>}
      {open && (
        <div className="aq-correction-form">
          <div className="ui-field">
            <label htmlFor="pitchTrack">대표곡</label>
            <select id="pitchTrack" className="ui-select" value={track} onChange={e => setTrack(e.target.value)}>
              {st.tracks.map(t => <option key={t.id} value={t.id}>{t.title}</option>)}
            </select>
          </div>
          <div className="ui-field">
            <label htmlFor="pitchDesc">곡 소개</label>
            <textarea id="pitchDesc" rows={4} maxLength={1000} value={f.description} onChange={set('description')}
              placeholder="어떤 곡인지, 만든 배경, 어울리는 순간을 적어 주세요. (20자 이상)" />
          </div>
          <div className="aq-pitch-grid">
            <div className="ui-field"><label htmlFor="pitchGenres">장르·스타일</label>
              <input id="pitchGenres" className="ui-input" maxLength={200} value={f.genres} onChange={set('genres')} placeholder="예: 로파이, 시티팝" /></div>
            <div className="ui-field"><label htmlFor="pitchMoods">분위기</label>
              <input id="pitchMoods" className="ui-input" maxLength={200} value={f.moods} onChange={set('moods')} placeholder="예: 차분함, 새벽" /></div>
            <div className="ui-field"><label htmlFor="pitchInst">주요 악기</label>
              <input id="pitchInst" className="ui-input" maxLength={200} value={f.instruments} onChange={set('instruments')} placeholder="예: 피아노, 신스" /></div>
            <div className="ui-field"><label htmlFor="pitchSimilar">비슷한 아티스트</label>
              <input id="pitchSimilar" className="ui-input" maxLength={200} value={f.similar_artists} onChange={set('similar_artists')} /></div>
          </div>
          <div className="ui-field">
            <label htmlFor="pitchMkt">홍보 계획<span className="aq-optional">선택</span></label>
            <textarea id="pitchMkt" rows={2} maxLength={1000} value={f.marketing} onChange={set('marketing')} placeholder="예: 뮤직비디오 공개, 라디오 출연" />
          </div>
          <div className="aq-confirm-actions">
            <button type="button" className="ui-btn is-soft" onClick={() => setOpen(false)} disabled={busy}>닫기</button>
            <button type="button" className="ui-btn is-primary" onClick={() => void submit()} disabled={busy || !track}>{busy ? '보내는 중' : '피칭 보내기'}</button>
          </div>
        </div>
      )}
    </section>
  );
}
