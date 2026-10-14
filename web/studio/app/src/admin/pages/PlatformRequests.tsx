// 플랫폼 요청 — 아티스트 프로필(오매칭·분리·통합·OAC)과 유튜브 Content ID(채널 제외·소유권 주장 해제·이의제기·켜고 끄기).
// 담당자가 플랫폼에 전달(접수 번호 기록)하고 결과를 남기면 아티스트에게 단계별로 알림이 간다.
// Content ID 요청을 완료하면 발매의 Content ID 상태가 바뀌고, 수익 상태는 유튜브에서 확인한 대로 따로 기록한다.
import { useState } from 'react';
import { Link } from '../../lib/router';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { req } from '../../api/http';
import { MOCK } from '../../lib/mode';
import { PLATFORM_NAMES } from '../../lib/platformNames';
import { KINDS, MONETIZATION_LABEL, STATUS_LABEL, STATUS_TONE, kindSpec, type PlatformRequest } from '../../lib/platformRequests';
import { ago } from '../labels';
import { Badge, EmptyState, ErrorBox, NoDuty, PageHeader, SkeletonList, DELIVERY_TABS, SubTabs, useStaff } from '../ui';

const OPEN = ['SUBMITTED', 'IN_PROGRESS', 'SENT_TO_PLATFORM'];

export function PlatformRequests() {
  const toast = useToast();
  const { can } = useStaff();
  const [all, setAll] = useState(false);
  const [group, setGroup] = useState('');
  const { data, loading, error, reload } = useAsync(
    () => (MOCK ? Promise.resolve({ items: [] as PlatformRequest[] })
      : req<{ items: PlatformRequest[] }>(`/api/staff/platform-requests?status=${all ? 'all' : 'open'}&group=${group}`)),
    [all, group],
  );
  const [refs, setRefs] = useState<Record<string, string>>({});
  const [notes, setNotes] = useState<Record<string, string>>({});
  const [money, setMoney] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState('');
  const ok = can('DELIVERY');
  const items = data?.items ?? [];

  const move = async (r: PlatformRequest, status: string, done: string) => {
    setBusy(r.id);
    try {
      await req(`/api/staff/platform-requests/${r.id}/status`, { method: 'POST', body: { status, platform_ref: refs[r.id] ?? '', note: notes[r.id] ?? '' } });
      toast(done, 'success'); reload();
    } catch (e) { toast(errorMessage(e, '처리하지 못했어요.'), 'error'); } finally { setBusy(''); }
  };
  const saveMoney = async (r: PlatformRequest) => {
    if (!r.release_id || !money[r.id]) return;
    try {
      await req(`/api/staff/releases/${r.release_id}/content-id`, { method: 'POST', body: { monetization: money[r.id], note: '' } });
      toast('Content ID 수익 상태를 기록했어요.', 'success');
    } catch (e) { toast(errorMessage(e), 'error'); }
  };

  return (
    <div className="view-enter">
      <PageHeader
        eyebrow="배급"
        title="플랫폼 요청"
        sub="아티스트 페이지 정리와 유튜브 Content ID 요청이에요. 플랫폼에 전달하면 접수 번호를 남기고, 결과를 기록해 주세요."
        actions={<button type="button" className="ui-btn is-soft is-sm" onClick={reload}>새로고침</button>}
      />
      <SubTabs tabs={DELIVERY_TABS} />
      {!ok && <NoDuty duty="배급" />}
      <div className="adm-filter-row" style={{ display: 'flex', gap: 8, flexWrap: 'wrap', margin: '0 0 12px' }}>
        <select className="ui-select" aria-label="구분" value={group} onChange={e => setGroup(e.target.value)}>
          <option value="">전체</option><option value="profile">아티스트 프로필</option><option value="content_id">유튜브 Content ID</option>
        </select>
        <label className="small"><input type="checkbox" checked={all} onChange={e => setAll(e.target.checked)} /> 끝난 요청도 보기</label>
      </div>
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <SkeletonList rows={3} /> : items.length === 0 ? (
        <EmptyState title="처리할 요청이 없어요">아티스트가 ‘플랫폼 요청’에서 보내면 이곳에 올라와요.</EmptyState>
      ) : (
        <div className="ui-list">
          {items.map(r => {
            const spec = kindSpec(r.kind);
            const open = OPEN.includes(r.status);
            const cid = r.kind.startsWith('CID_');
            return (
              <div key={r.id} className="ui-card">
                <div className="adm-check-top">
                  <span>
                    <span className="ui-row-title">{spec?.label ?? r.kind}</span>
                    <span className="ui-row-meta">
                      <span>{r.org_name}</span><span>{r.artist_name}</span><span>{PLATFORM_NAMES[r.platform] ?? r.platform}</span>
                      {r.release_id && <Link to={`/admin/reviews/${r.release_id}`}>{r.release_title ?? '발매'}</Link>}
                      <span>접수 {ago(r.created_at)}</span>
                    </span>
                  </span>
                  <Badge tone={STATUS_TONE[r.status] ?? 'gray'}>{STATUS_LABEL[r.status] ?? r.status}</Badge>
                </div>
                <div className="adm-note ui-on-soft" style={{ marginTop: 12 }}>
                  {r.urls.map((u, i) => (
                    <div key={u}><small className="muted">{spec?.urls[i] ?? '링크'}</small><p><a href={u} target="_blank" rel="noopener noreferrer">{u}</a></p></div>
                  ))}
                  {r.description && <><small className="muted">상황 설명</small><p style={{ whiteSpace: 'pre-wrap' }}>{r.description}</p></>}
                  {r.platform_ref && <><small className="muted">플랫폼 접수 번호</small><p>{r.platform_ref}</p></>}
                  {r.resolution && <><small className="muted">결과</small><p>{r.resolution}</p></>}
                </div>
                {open && (
                  <>
                    <div className="form-grid" style={{ marginTop: 12 }}>
                      <div className="ui-field">
                        <label htmlFor={`pr-ref-${r.id}`}>플랫폼 접수 번호 (전달할 때)</label>
                        <input id={`pr-ref-${r.id}`} className="ui-input" maxLength={200} value={refs[r.id] ?? ''} onChange={e => setRefs(m => ({ ...m, [r.id]: e.target.value }))} />
                      </div>
                      <div className="ui-field">
                        <label htmlFor={`pr-note-${r.id}`}>아티스트에게 남길 말 (처리 불가면 필수)</label>
                        <input id={`pr-note-${r.id}`} className="ui-input" maxLength={1000} value={notes[r.id] ?? ''} onChange={e => setNotes(m => ({ ...m, [r.id]: e.target.value }))} />
                      </div>
                    </div>
                    <div className="adm-form-actions">
                      {r.status === 'SUBMITTED' && <button type="button" className="ui-btn is-soft" disabled={!ok || busy === r.id} onClick={() => void move(r, 'IN_PROGRESS', '처리 중으로 바꿨어요.')}>처리 시작</button>}
                      {r.status !== 'SENT_TO_PLATFORM' && <button type="button" className="ui-btn is-soft" disabled={!ok || busy === r.id} onClick={() => void move(r, 'SENT_TO_PLATFORM', '플랫폼에 전달했다고 기록했어요.')}>플랫폼에 전달함</button>}
                      <button type="button" className="ui-btn is-soft" disabled={!ok || busy === r.id} onClick={() => void move(r, 'REJECTED', '처리 불가로 기록했어요.')}>처리 불가</button>
                      <button type="button" className="ui-btn is-primary" disabled={!ok || busy === r.id} onClick={() => void move(r, 'RESOLVED', '완료로 기록했어요.')}>완료</button>
                    </div>
                  </>
                )}
                {cid && r.release_id && (
                  <div className="adm-form-actions" style={{ marginTop: 8 }}>
                    <select className="ui-select" aria-label="Content ID 수익 상태" value={money[r.id] ?? ''} onChange={e => setMoney(m => ({ ...m, [r.id]: e.target.value }))}>
                      <option value="">유튜브 수익 상태 기록…</option>
                      {Object.entries(MONETIZATION_LABEL).map(([k, l]) => <option key={k} value={k}>{l}</option>)}
                    </select>
                    <button type="button" className="ui-btn is-soft is-sm" disabled={!ok || !money[r.id]} onClick={() => void saveMoney(r)}>기록</button>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
      <p className="small muted" style={{ marginTop: 16 }}>요청 종류: {KINDS.map(k => k.label).join(' · ')}</p>
    </div>
  );
}
