// 플랫폼 요청 (/platform-requests) — 발매 뒤 플랫폼에서 처리할 일: 아티스트 페이지 오매칭·분리·통합·유튜브 OAC,
// 유튜브 Content ID(채널 제외·소유권 주장 해제·이의제기·켜고 끄기). 접수부터 플랫폼 전달·결과까지 단계별로 보여 준다.
import { useEffect, useMemo, useState } from 'react';
import { useSearchParams } from '../lib/router';
import { orgPath, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { api } from '../api/client';
import type { Release } from '../api/types';
import { MOCK } from '../lib/mode';
import { niceDate } from '../lib/format';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { Badge } from '../ui';
import { PLATFORM_NAMES } from '../lib/platformNames';
import { KINDS, STATUS_LABEL, STATUS_TONE, kindSpec, type PlatformRequest, type RequestGroup } from '../lib/platformRequests';

const PROFILE_PLATFORMS = ['melon', 'genie', 'flo', 'bugs', 'vibe', 'spotify', 'apple', 'youtube', 'amazon', 'deezer', 'tidal'];

function RequestForm({ group, releases, artist, onDone }: { group: RequestGroup; releases: Release[]; artist: string; onDone: () => void }) {
  const toast = useToast();
  const kinds = KINDS.filter(k => k.group === group);
  const [kind, setKind] = useState(kinds[0].kind);
  const spec = kindSpec(kind)!;
  const [platform, setPlatform] = useState('melon');
  const [releaseId, setReleaseId] = useState('');
  const [name, setName] = useState(artist);
  const [urls, setUrls] = useState<string[]>([]);
  const [desc, setDesc] = useState('');
  const [busy, setBusy] = useState(false);
  useEffect(() => { setUrls([]); }, [kind]);
  useEffect(() => { if (!name && artist) setName(artist); }, [artist, name]);
  const submitted = releases.filter(r => r.status !== 'draft');

  const send = async () => {
    if (spec.release === 'required' && !releaseId) { toast('발매를 골라 주세요.', 'error'); return; }
    if (urls.filter(u => u.trim()).length < spec.minUrls) { toast(`${spec.urls[0]}를 적어 주세요.`, 'error'); return; }
    if (spec.description === 'required' && desc.trim().length < 10) { toast('상황을 10자 이상 적어 주세요.', 'error'); return; }
    setBusy(true);
    try {
      if (!MOCK) {
        await req(orgPath('/platform-requests'), { method: 'POST', body: {
          kind, platform: spec.youtube ? 'youtube' : platform, artist_name: name.trim(),
          ...(releaseId ? { release_id: releaseId } : {}), urls: urls.map(u => u.trim()).filter(Boolean), description: desc.trim(),
        } });
      }
      toast('요청을 접수했어요. 진행 상황은 아래와 알림으로 알려 드려요.', 'success');
      setUrls([]); setDesc('');
      onDone();
    } catch (e) { toast(errorMessage(e), 'error'); } finally { setBusy(false); }
  };

  return (
    <section className="ui-card aq-preq-form" aria-labelledby="preqFormHeading">
      <h2 id="preqFormHeading" className="subhead">새 요청</h2>
      <div className="ui-field">
        <label htmlFor="preqKind">무엇을 도와 드릴까요?</label>
        <select id="preqKind" className="ui-select" value={kind} onChange={e => setKind(e.target.value)}>
          {kinds.map(k => <option key={k.kind} value={k.kind}>{k.label}</option>)}
        </select>
        <small className="ui-field-help">{spec.help}</small>
      </div>
      <div className="form-grid">
        {!spec.youtube && (
          <div className="ui-field">
            <label htmlFor="preqPlatform">플랫폼</label>
            <select id="preqPlatform" className="ui-select" value={platform} onChange={e => setPlatform(e.target.value)}>
              {PROFILE_PLATFORMS.map(p => <option key={p} value={p}>{PLATFORM_NAMES[p] ?? p}</option>)}
            </select>
          </div>
        )}
        <div className="ui-field">
          <label htmlFor="preqArtist">아티스트명</label>
          <input id="preqArtist" className="ui-input" maxLength={200} value={name} onChange={e => setName(e.target.value)} />
        </div>
        {spec.release !== 'none' && (
          <div className="ui-field">
            <label htmlFor="preqRelease">발매{spec.release === 'optional' ? ' (해당되면)' : ''}</label>
            <select id="preqRelease" className="ui-select" value={releaseId} onChange={e => setReleaseId(e.target.value)}>
              <option value="">{spec.release === 'required' ? '발매를 골라 주세요' : '선택하지 않음'}</option>
              {submitted.map(r => <option key={r.id} value={r.id}>{r.title}</option>)}
            </select>
          </div>
        )}
      </div>
      {spec.urls.map((label, i) => (
        <div key={`${kind}-${i}`} className="ui-field">
          <label htmlFor={`preqUrl${i}`}>{label}</label>
          <input id={`preqUrl${i}`} className="ui-input" type="url" inputMode="url" maxLength={500} placeholder="https://"
            value={urls[i] ?? ''} onChange={e => setUrls(u => { const n = [...u]; n[i] = e.target.value; return n; })} />
        </div>
      ))}
      <div className="ui-field">
        <label htmlFor="preqDesc">상황 설명{spec.description === 'required' ? '' : ' (선택)'}</label>
        <textarea id="preqDesc" rows={3} maxLength={2000} value={desc} onChange={e => setDesc(e.target.value)} />
      </div>
      <div className="aq-confirm-actions">
        <button type="button" className="ui-btn is-primary" disabled={busy} onClick={() => void send()}>{busy ? '보내는 중' : '요청 보내기'}</button>
      </div>
    </section>
  );
}

function RequestItem({ r, open, onCancel }: { r: PlatformRequest; open: boolean; onCancel: () => void }) {
  const spec = kindSpec(r.kind);
  return (
    <details className="ui-card aq-preq-item" open={open}>
      <summary>
        <span className="min-0">
          <span className="ui-row-title">{spec?.label ?? r.kind}</span>
          <span className="ui-row-meta">
            <span>{PLATFORM_NAMES[r.platform] ?? r.platform}</span>{r.release_title && <span>{r.release_title}</span>}<span>{niceDate(r.created_at)}</span>
          </span>
        </span>
        <Badge tone={STATUS_TONE[r.status] ?? 'gray'}>{STATUS_LABEL[r.status] ?? r.status}</Badge>
      </summary>
      <ol className="aq-preq-steps">
        {r.events.map((e, i) => (
          <li key={i}><b>{STATUS_LABEL[e.status] ?? e.status}</b> <span className="muted small">{niceDate(e.at)}</span>
            {e.note && <p className="small">{e.note}</p>}</li>
        ))}
      </ol>
      {r.platform_ref && <p className="small muted">플랫폼 접수 번호: {r.platform_ref}</p>}
      {r.urls.length > 0 && <ul className="aq-preq-urls">{r.urls.map(u => <li key={u}><a href={u} target="_blank" rel="noopener noreferrer">{u}</a></li>)}</ul>}
      {r.description && <p className="small" style={{ whiteSpace: 'pre-wrap' }}>{r.description}</p>}
      {(r.status === 'SUBMITTED' || r.status === 'IN_PROGRESS') && (
        <button type="button" className="ui-btn is-text is-sm" onClick={onCancel}>요청 취소</button>
      )}
    </details>
  );
}

export function PlatformRequests() {
  const toast = useToast();
  const confirm = useConfirm();
  const [params, setParams] = useSearchParams();
  const group: RequestGroup = params.get('tab') === 'content_id' ? 'content_id' : 'profile';
  const focus = params.get('id') ?? '';
  const [items, setItems] = useState<PlatformRequest[] | null>(null);
  const [error, setError] = useState('');
  const [releases, setReleases] = useState<Release[]>([]);
  const load = async () => {
    if (MOCK) { setItems([]); return; }
    try { setItems((await req<{ items: PlatformRequest[] }>(orgPath('/platform-requests'))).items); setError(''); }
    catch (e) { setError(errorMessage(e)); }
  };
  useEffect(() => { void load(); api.listReleases().then(setReleases, () => setReleases([])); }, []);
  // 알림 링크(?id=)로 들어오면 그 요청이 있는 탭을 연다
  useEffect(() => {
    const hit = items?.find(i => i.id === focus);
    const g = hit && kindSpec(hit.kind)?.group;
    if (g && g !== group) { const n = new URLSearchParams(params); n.set('tab', g); setParams(n, { replace: true }); }
  }, [items, focus]);
  const artist = useMemo(() => releases.find(r => r.artist)?.artist ?? '', [releases]);
  const shown = (items ?? []).filter(i => kindSpec(i.kind)?.group === group);
  const cancel = async (r: PlatformRequest) => {
    if (!(await confirm({ title: '요청을 취소할까요?', message: '아직 플랫폼에 전달되지 않은 요청만 취소할 수 있어요.', confirmLabel: '요청 취소' }))) return;
    try { await req(orgPath(`/platform-requests/${encodeURIComponent(r.id)}/cancel`), { method: 'POST', body: {} }); void load(); }
    catch (e) { toast(errorMessage(e), 'error'); }
  };

  return (
    <div className="view">
      <div className="ui-page-head">
        <div>
          <p className="eyebrow">발매 후 관리</p>
          <h1>플랫폼 요청</h1>
          <p>아티스트 페이지 정리와 유튜브 Content ID 관련 요청을 보내고, 플랫폼 처리 결과까지 확인해요.</p>
        </div>
      </div>
      <div className="ui-chips" role="tablist" aria-label="요청 구분">
        {([['profile', '아티스트 프로필'], ['content_id', '유튜브 Content ID']] as const).map(([v, l]) => (
          <button key={v} type="button" role="tab" aria-selected={group === v} className={`ui-chip${group === v ? ' is-on' : ''}`}
            onClick={() => { const n = new URLSearchParams(params); n.set('tab', v); n.delete('id'); setParams(n); }}>{l}</button>
        ))}
      </div>
      <RequestForm key={group} group={group} releases={releases} artist={artist} onDone={() => void load()} />
      <h2 className="subhead">요청 내역</h2>
      {error ? <p className="aq-smartlink-alert-err">{error}</p> : !items ? <p className="muted">불러오는 중…</p> : shown.length === 0 ? (
        <div className="ui-empty"><h2>요청 내역이 없어요.</h2><p>위에서 요청을 보내면 진행 상황이 여기에 표시돼요.</p></div>
      ) : (
        <div className="aq-preq-list">
          {shown.map(r => <RequestItem key={r.id} r={r} open={r.id === focus} onCancel={() => void cancel(r)} />)}
        </div>
      )}
    </div>
  );
}
