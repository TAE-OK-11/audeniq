// 플레이리스트 피칭 — 아티스트가 발매일 7일 전까지 보낸 대표곡 소개. 담당자가 검토해 플랫폼 에디터에게
// 전달하고(전달한 플랫폼 기록), 결과(선정된 플레이리스트 또는 미선정)를 남긴다. 단계마다 아티스트에게 알림이 간다.
import { useState } from 'react';
import { Link } from '../../lib/router';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { staffApi, type PitchItem } from '../api';
import { ago } from '../labels';
import { Badge, EmptyState, ErrorBox, NoDuty, PageHeader, SkeletonList, DELIVERY_TABS, SubTabs, useStaff } from '../ui';

const STATUS: Record<PitchItem['status'], { label: string; tone: 'blue' | 'green' | 'red' | 'gray' }> = {
  SUBMITTED: { label: '접수', tone: 'blue' },
  IN_REVIEW: { label: '검토 중', tone: 'blue' },
  PITCHED: { label: '플랫폼 전달', tone: 'gray' },
  SELECTED: { label: '선정', tone: 'green' },
  NOT_SELECTED: { label: '미선정', tone: 'red' },
  WITHDRAWN: { label: '아티스트 취소', tone: 'gray' },
};
const PLATFORMS: [string, string][] = [
  ['melon', '멜론'], ['genie', '지니'], ['bugs', '벅스'], ['flo', 'FLO'], ['vibe', 'VIBE'],
  ['spotify', 'Spotify'], ['apple', 'Apple Music'], ['youtube', 'YouTube Music'],
];
const platformName = (slug: string) => PLATFORMS.find(([s]) => s === slug)?.[1] ?? slug;
const initialPick = (p: PitchItem) =>
  (p.platforms.length ? p.platforms : p.platforms_requested ?? []).filter(s => PLATFORMS.some(([k]) => k === s));

export function Pitches() {
  const toast = useToast();
  const { can } = useStaff();
  const { data, loading, error, reload } = useAsync(() => staffApi.pitches(), []);
  const [picked, setPicked] = useState<Record<string, string[]>>({});
  const [results, setResults] = useState<Record<string, string>>({});
  const [notes, setNotes] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState('');
  const items = data?.items ?? [];
  const ok = can('DELIVERY');

  const move = async (p: PitchItem, status: string, done: string, platforms: string[] = []) => {
    setBusy(p.id);
    try {
      await staffApi.updatePitch(p.id, { status, platforms, result: results[p.id] ?? '', note: notes[p.id] ?? '' });
      toast(done, 'success');
      reload();
    } catch (e) {
      toast(errorMessage(e, '처리하지 못했어요.'), 'error');
    } finally {
      setBusy('');
    }
  };
  const toggle = (p: PitchItem, slug: string) => setPicked(m => {
    const cur = m[p.id] ?? initialPick(p);
    return { ...m, [p.id]: cur.includes(slug) ? cur.filter(x => x !== slug) : [...cur, slug] };
  });

  return (
    <div className="view-enter">
      <PageHeader
        eyebrow="배급"
        title="플레이리스트 피칭"
        sub="아티스트가 보낸 대표곡 소개예요. 플랫폼 에디터에게 전달하고 결과를 기록하면 아티스트에게 알림이 가요."
        actions={<button type="button" className="ui-btn is-soft is-sm" onClick={reload}>새로고침</button>}
      />
      <SubTabs tabs={DELIVERY_TABS} />
      {!ok && <NoDuty duty="배급" />}
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <SkeletonList rows={3} /> : items.length === 0 ? (
        <EmptyState title="피칭이 없어요">아티스트가 발매 상세에서 피칭하면 이곳에 올라와요.</EmptyState>
      ) : (
        <div className="ui-list">
          {items.map(p => {
            const sel = picked[p.id] ?? initialPick(p);
            const open = p.status === 'SUBMITTED' || p.status === 'IN_REVIEW';
            return (
              <div key={p.id} className="ui-card">
                <div className="adm-check-top">
                  <span>
                    <Link to={`/admin/reviews/${p.release_id}`} className="ui-row-title">{p.title}</Link>
                    <span className="ui-row-meta">
                      <span>{p.org_name}</span>{p.artist && <span>{p.artist}</span>}
                      {p.release_date && <span>발매 {p.release_date}</span>}<span>접수 {ago(p.created_at)}</span>
                    </span>
                  </span>
                  <Badge tone={STATUS[p.status].tone}>{STATUS[p.status].label}</Badge>
                </div>
                <div className="adm-note ui-on-soft" style={{ marginTop: 12 }}>
                  <small className="muted">대표곡</small><p>{p.track_title}</p>
                  <small className="muted">곡 소개</small><p style={{ whiteSpace: 'pre-wrap' }}>{p.description}</p>
                  {p.genres && <><small className="muted">장르·스타일</small><p>{p.genres}</p></>}
                  {p.moods && <><small className="muted">분위기</small><p>{p.moods}</p></>}
                  {p.instruments && <><small className="muted">주요 악기</small><p>{p.instruments}</p></>}
                  {p.similar_artists && <><small className="muted">비슷한 아티스트</small><p>{p.similar_artists}</p></>}
                  {p.marketing && <><small className="muted">홍보 계획</small><p style={{ whiteSpace: 'pre-wrap' }}>{p.marketing}</p></>}
                  {p.platforms.length > 0 && <><small className="muted">전달한 플랫폼</small><p>{p.platforms.map(platformName).join(', ')}</p></>}
                  {p.result && <><small className="muted">결과</small><p>{p.result}</p></>}
                  {p.staff_note && <><small className="muted">메모</small><p>{p.staff_note}</p></>}
                </div>
                {open && (
                  <>
                    <fieldset className="ui-field" style={{ marginTop: 12, border: 0, padding: 0 }}>
                      <legend>전달할 플랫폼</legend>
                      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px 14px' }}>
                        {PLATFORMS.map(([slug, name]) => (
                          <label key={slug} className="small"><input type="checkbox" checked={sel.includes(slug)} onChange={() => toggle(p, slug)} /> {name}</label>
                        ))}
                      </div>
                    </fieldset>
                    <div className="adm-form-actions">
                      {p.status === 'SUBMITTED' && <button type="button" className="ui-btn is-soft" disabled={!ok || busy === p.id} onClick={() => void move(p, 'IN_REVIEW', '검토 중으로 바꿨어요.')}>검토 시작</button>}
                      <button type="button" className="ui-btn is-primary" disabled={!ok || busy === p.id || !sel.length}
                        onClick={() => void move(p, 'PITCHED', '플랫폼에 전달했다고 기록했어요.', sel)}>
                        플랫폼에 전달함
                      </button>
                    </div>
                  </>
                )}
                {p.status === 'PITCHED' && (
                  <>
                    <div className="ui-field" style={{ marginTop: 12 }}>
                      <label htmlFor={`pp-res-${p.id}`}>선정된 플레이리스트</label>
                      <input id={`pp-res-${p.id}`} className="ui-input" maxLength={300} value={results[p.id] ?? ''} onChange={e => setResults(r => ({ ...r, [p.id]: e.target.value }))} placeholder="예: 멜론 '오늘의 발견'" />
                    </div>
                    <div className="ui-field">
                      <label htmlFor={`pp-note-${p.id}`}>아티스트에게 남길 말<span className="aq-optional">선택</span></label>
                      <textarea id={`pp-note-${p.id}`} rows={2} maxLength={1000} value={notes[p.id] ?? ''} onChange={e => setNotes(n => ({ ...n, [p.id]: e.target.value }))} />
                    </div>
                    <div className="adm-form-actions">
                      <button type="button" className="ui-btn is-soft" disabled={!ok || busy === p.id} onClick={() => void move(p, 'NOT_SELECTED', '미선정으로 기록했어요.')}>미선정</button>
                      <button type="button" className="ui-btn is-primary" disabled={!ok || busy === p.id || !(results[p.id] ?? '').trim()} onClick={() => void move(p, 'SELECTED', '선정으로 기록했어요.')}>선정</button>
                    </div>
                  </>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
