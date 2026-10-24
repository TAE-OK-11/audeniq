// 스튜디오 > 계정 정보 > 아티스트 페이지 — 소개 페이지의 소식(공지·이벤트)을 올리고 고치는 창과 팬 응원을 관리하는 창.
// ArtistHubCard의 ‘소식’·‘팬 응원’ 줄에서 열린다. 공개 페이지에 보이는 모양(PostCard·CheerItem)을 그대로 보여 준다.
import { useCallback, useEffect, useState } from 'react';
import { orgPath, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { MOCK } from '../lib/mode';
import {
  MOCK_CHEERS, MOCK_POSTS, POST_BODY_MAX, POST_PLACE_MAX, POST_TITLE_MAX, isoToLocal, localToIso,
  type ArtistPost, type Cheer, type PostKind,
} from '../lib/artistCommunity';
import { artistNewsPath, artistPath } from '../lib/publicLinks';
import { Reveal, Segmented, SkeletonList } from '../ui';
import { DateTimeField } from './Picker';
import { Glyph } from './Glyph';
import { useToast } from './Toast';
import { useConfirm } from './Confirm';
import { CheerItem, CheerSkeleton, PostCard } from './ArtistCommunity';

/** 지울 때 — 줄이 옅어지며 접힌 뒤(0.26초) 목록에서 뺀다 */
const LEAVE_MS = 260;
const leaveDelay = () => (window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ? 0 : LEAVE_MS);

const base = (artist: string) => orgPath(`/artists/${encodeURIComponent(artist)}/link`);
const KINDS = [{ value: 'NOTICE' as const, label: '공지' }, { value: 'EVENT' as const, label: '이벤트' }];

interface Draft { kind: PostKind; title: string; body: string; when: string; place: string; url: string; pinned: boolean }
const blank: Draft = { kind: 'NOTICE', title: '', body: '', when: '', place: '', url: '', pinned: false };
const toDraft = (p: ArtistPost): Draft => ({ kind: p.kind, title: p.title, body: p.body, when: isoToLocal(p.starts_at), place: p.place, url: p.url, pinned: p.pinned });

function PostForm({ artistId, post, onSaved, onCancel }: { artistId: string; post: ArtistPost | null; onSaved: (p: ArtistPost) => void; onCancel: () => void }) {
  const toast = useToast();
  const [d, setD] = useState<Draft>(post ? toDraft(post) : blank);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => { setD(cur => ({ ...cur, [k]: v })); setError(''); };
  const event = d.kind === 'EVENT';

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!d.title.trim()) { setError('제목을 적어 주세요.'); document.getElementById('postTitle')?.focus(); return; }
    const startsAt = event ? localToIso(d.when) : null;
    if (event && !startsAt) { setError('이벤트 날짜와 시간을 골라 주세요.'); return; }
    const url = d.url.trim() === 'https://' ? '' : d.url.trim();
    if (url && !/^https:\/\/[^\s/]+\.[^\s]+$/.test(url)) { setError('링크는 https://로 시작하는 주소로 넣어 주세요.'); document.getElementById('postUrl')?.focus(); return; }
    const body = { kind: d.kind, title: d.title.trim(), body: d.body.trim(), starts_at: startsAt, place: event ? d.place.trim() : '', url, pinned: d.pinned };
    setBusy(true);
    try {
      const now = new Date().toISOString();
      const saved = MOCK
        ? { id: post?.id ?? `m${Date.now()}`, ...body, created_at: post?.created_at ?? now, updated_at: now }
        : await req<ArtistPost>(post ? `${base(artistId)}/posts/${post.id}` : `${base(artistId)}/posts`, { method: post ? 'PUT' : 'POST', body });
      onSaved(saved);
      toast(post ? '소식을 고쳤어요.' : '소식을 올렸어요. 소개 페이지에 바로 보여요.', 'success');
    } catch (err) { setError(errorMessage(err)); } finally { setBusy(false); }
  };

  return (
    <form className="aq-post-form aq-mgr-swap" onSubmit={save} noValidate>
      <Segmented value={d.kind} options={KINDS} onChange={v => set('kind', v)} label="종류" fill />
      <div className="ui-field">
        <label htmlFor="postTitle">제목</label>
        <input id="postTitle" className="ui-input" maxLength={POST_TITLE_MAX} value={d.title} onChange={e => set('title', e.target.value)}
          placeholder={event ? '예: 단독 공연 ‘새벽의 도시’' : '예: 새 EP 작업을 마쳤어요'} />
      </div>
      <Reveal open={event}>
        <div className="aq-post-form-row">
          <div className="ui-field">
            <label htmlFor="postWhen">날짜·시간</label>
            <DateTimeField id="postWhen" className="ui-input" value={d.when} onChange={v => set('when', v)} required />
          </div>
          <div className="ui-field">
            <label htmlFor="postPlace">장소 <span className="muted small">(선택)</span></label>
            <input id="postPlace" className="ui-input" maxLength={POST_PLACE_MAX} value={d.place} onChange={e => set('place', e.target.value)} placeholder="예: 홍대 롤링홀 · 온라인" />
          </div>
        </div>
      </Reveal>
      <div className="ui-field">
        <label htmlFor="postBody">내용 <span className="muted small">(선택)</span></label>
        <textarea id="postBody" className="ui-input" rows={4} maxLength={POST_BODY_MAX} value={d.body} onChange={e => set('body', e.target.value)}
          placeholder="팬에게 전하고 싶은 이야기를 적어 주세요." />
        <small className="ui-field-help aq-post-count">{d.body.length.toLocaleString()} / {POST_BODY_MAX.toLocaleString()}</small>
      </div>
      <div className="ui-field">
        <label htmlFor="postUrl">{event ? '예매·자세히 링크' : '링크'} <span className="muted small">(선택)</span></label>
        <input id="postUrl" className="ui-input" type="url" inputMode="url" maxLength={500} value={d.url} onChange={e => set('url', e.target.value)} placeholder="https://" />
      </div>
      <div className="aq-alert-pref">
        <span className="aq-alert-pref-icon" aria-hidden="true"><Glyph name="pin" size={18} /></span>
        <span className="min-0"><strong>맨 위에 고정</strong><span>소개 페이지와 소식 목록 맨 위에 보여요.</span></span>
        <label className="aq-switch">
          <input type="checkbox" role="switch" aria-label="맨 위에 고정" checked={d.pinned} onChange={e => set('pinned', e.target.checked)} />
          <span aria-hidden="true" />
        </label>
      </div>
      {error && <p className="ui-field-help is-error" role="alert">{error}</p>}
      <div className="aq-post-form-actions">
        <button type="button" className="ui-btn is-soft" onClick={onCancel}>취소</button>
        <button type="submit" className={`ui-btn is-primary${busy ? ' is-busy' : ''}`} disabled={busy}>{busy ? '저장하는 중' : post ? '저장' : '올리기'}</button>
      </div>
    </form>
  );
}

/** 소식 관리 — 목록(공개 페이지와 같은 순서)과 새 소식·고치기·지우기 */
export function PostsManager({ artistId, artist, slug, onCount }: { artistId: string; artist: string; slug: string; onCount: (n: number) => void }) {
  const toast = useToast();
  const confirm = useConfirm();
  const [items, setItems] = useState<ArtistPost[] | null>(null);
  const [max, setMax] = useState(100);
  const [editing, setEditing] = useState<ArtistPost | 'new' | null>(null);
  const [leaving, setLeaving] = useState<string | null>(null);
  const [justSaved, setJustSaved] = useState<string | null>(null);
  // 글쓰기에서 목록으로 돌아올 때만 목록이 살짝 떠오른다 (창이 처음 열릴 때는 창 자체 등장에 맡긴다)
  const [returned, setReturned] = useState(false);

  const load = useCallback(async () => {
    if (MOCK) { setItems(cur => cur ?? MOCK_POSTS); return; }
    try {
      const r = await req<{ items: ArtistPost[]; max: number }>(`${base(artistId)}/posts`, { quiet401: true });
      setItems(r.items); setMax(r.max);
    } catch (err) { setItems([]); toast(errorMessage(err), 'error'); }
  }, [artistId, toast]);
  useEffect(() => { void load(); }, [load]);
  useEffect(() => { if (items) onCount(items.length); }, [items, onCount]);

  const saved = (p: ArtistPost) => {
    setEditing(null);
    setReturned(true);
    setJustSaved(p.id);
    if (MOCK) setItems(cur => [p, ...(cur ?? []).filter(x => x.id !== p.id)].sort((a, b) => Number(b.pinned) - Number(a.pinned)));
    else void load();
  };
  const remove = async (p: ArtistPost) => {
    const ok = await confirm({ title: `‘${p.title}’ 소식을 지울까요?`, message: '소개 페이지에서 바로 사라지고 되돌릴 수 없어요.', confirmLabel: '지우기', danger: true });
    if (!ok) return;
    try {
      if (!MOCK) await req(`${base(artistId)}/posts/${p.id}`, { method: 'DELETE' });
      setLeaving(p.id);
      window.setTimeout(() => { setItems(cur => (cur ?? []).filter(x => x.id !== p.id)); setLeaving(null); }, leaveDelay());
      toast('소식을 지웠어요.', 'success');
    } catch (err) { toast(errorMessage(err), 'error'); }
  };

  if (editing) {
    return <PostForm artistId={artistId} post={editing === 'new' ? null : editing} onSaved={saved} onCancel={() => { setEditing(null); setReturned(true); }} />;
  }
  return (
    <div className={`aq-community-mgr${returned ? ' aq-mgr-swap' : ''}`}>
      <p className="ui-field-help">공지나 공연·팬미팅 같은 이벤트를 올리면 <a href={artistPath(slug)} target="_blank" rel="noopener noreferrer">소개 페이지</a>에 바로 보여요. 고정한 글과 다가오는 이벤트가 먼저 나와요.</p>
      <button type="button" className="ui-btn is-primary is-block" onClick={() => setEditing('new')} disabled={!!items && items.length >= max}>
        <Glyph name="plus" size={16} />새 소식 올리기
      </button>
      {items === null ? <SkeletonList rows={3} />
        : items.length === 0 ? <p className="aq-cheer-empty"><Glyph name="megaphone" size={18} />아직 올린 소식이 없어요.</p>
        : (
          <ul className="aq-post-list">
            {items.map(p => (
              <li key={p.id} className={`aq-mgr-post${leaving === p.id ? ' is-leaving' : ''}${justSaved === p.id ? ' is-saved' : ''}`}>
                <PostCard post={p} artist={artist} clamp />
                <div className="aq-mgr-actions">
                  <button type="button" className="ui-btn is-sm is-soft" onClick={() => setEditing(p)}><Glyph name="pencil" size={14} />고치기</button>
                  <button type="button" className="ui-btn is-sm is-soft" onClick={() => void remove(p)}><Glyph name="trash" size={14} />지우기</button>
                </div>
              </li>
            ))}
          </ul>
        )}
      {items && items.length > 0 && (
        <a className="aq-mgr-open" href={artistNewsPath(slug)} target="_blank" rel="noopener noreferrer">팬에게 보이는 소식 페이지 열기<Glyph name="arrow-up-right" size={14} /></a>
      )}
    </div>
  );
}

/** 팬 응원 관리 — 숨김·하트·지우기, 받기 켜고 끄기 */
export function CheersManager({ artistId, artist, enabled, onEnabled, onCount }: {
  artistId: string; artist: string; enabled: boolean; onEnabled: (v: boolean) => Promise<void>; onCount: (n: number) => void;
}) {
  const toast = useToast();
  const confirm = useConfirm();
  const [items, setItems] = useState<Cheer[] | null>(null);
  const [more, setMore] = useState(false);
  const [stats, setStats] = useState({ total: 0, hidden: 0 });
  const [busy, setBusy] = useState<string | null>(null);
  const [leaving, setLeaving] = useState<string | null>(null);
  const [popped, setPopped] = useState<string | null>(null);
  const [switching, setSwitching] = useState(false);

  const load = useCallback(async (before?: string) => {
    if (MOCK) { setItems(cur => cur ?? MOCK_CHEERS); setStats({ total: MOCK_CHEERS.length, hidden: 0 }); return; }
    try {
      const r = await req<{ items: Cheer[]; more: boolean; total: number; hidden: number }>(
        `${base(artistId)}/cheers${before ? `?before=${encodeURIComponent(before)}` : ''}`, { quiet401: true });
      setItems(cur => (before ? [...(cur ?? []), ...r.items] : r.items));
      setMore(r.more); setStats({ total: r.total, hidden: r.hidden });
    } catch (err) { setItems(cur => cur ?? []); toast(errorMessage(err), 'error'); }
  }, [artistId, toast]);
  useEffect(() => { void load(); }, [load]);
  useEffect(() => { onCount(stats.total); }, [stats.total, onCount]);

  const patch = async (c: Cheer, body: { hidden?: boolean; heart?: boolean }) => {
    setBusy(c.id);
    try {
      const r = MOCK ? { hidden: body.hidden ?? !!c.hidden, heart: body.heart ?? c.heart } : await req<{ hidden: boolean; heart: boolean }>(`${base(artistId)}/cheers/${c.id}`, { method: 'PUT', body });
      setItems(cur => (cur ?? []).map(x => (x.id === c.id ? { ...x, ...r } : x)));
      if (body.heart) setPopped(c.id);
      if (body.hidden !== undefined && body.hidden !== !!c.hidden) setStats(s => ({ ...s, hidden: s.hidden + (r.hidden ? 1 : -1) }));
    } catch (err) { toast(errorMessage(err), 'error'); } finally { setBusy(null); }
  };
  const remove = async (c: Cheer) => {
    const ok = await confirm({ title: `${c.nickname}님의 응원을 지울까요?`, message: '되돌릴 수 없어요. 잠시 가리려면 ‘숨기기’를 써 주세요.', confirmLabel: '지우기', danger: true });
    if (!ok) return;
    setBusy(c.id);
    try {
      if (!MOCK) await req(`${base(artistId)}/cheers/${c.id}`, { method: 'DELETE' });
      setLeaving(c.id);
      window.setTimeout(() => { setItems(cur => (cur ?? []).filter(x => x.id !== c.id)); setLeaving(null); }, leaveDelay());
      setStats(s => ({ total: s.total - 1, hidden: s.hidden - (c.hidden ? 1 : 0) }));
    } catch (err) { toast(errorMessage(err), 'error'); } finally { setBusy(null); }
  };
  const toggle = async (v: boolean) => {
    setSwitching(true);
    try { await onEnabled(v); toast(v ? '팬 응원을 다시 받아요.' : '팬 응원을 껐어요. 남긴 응원도 페이지에서 가려져요.', 'success'); }
    catch (err) { toast(errorMessage(err), 'error'); } finally { setSwitching(false); }
  };

  return (
    <div className="aq-community-mgr">
      <div className="aq-alert-pref">
        <span className="aq-alert-pref-icon" aria-hidden="true"><Glyph name="heart" size={18} /></span>
        <span className="min-0">
          <strong>팬 응원 받기</strong>
          <span>{enabled ? '누구나 닉네임으로 응원을 남길 수 있어요.' : '꺼져 있어요. 페이지에 응원이 보이지 않아요.'}</span>
        </span>
        <label className="aq-switch">
          <input type="checkbox" role="switch" aria-label="팬 응원 받기" checked={enabled} disabled={switching} onChange={e => void toggle(e.target.checked)} />
          <span aria-hidden="true" />
        </label>
      </div>
      <p className="ui-field-help">
        {stats.total ? `응원 ${stats.total.toLocaleString()}개${stats.hidden ? ` · 숨김 ${stats.hidden}개` : ''}. ` : ''}
        하트를 누르면 ‘{artist}님이 좋아해요’로 보여요. 불편한 글은 숨기거나 지울 수 있어요.
      </p>
      {items === null ? <CheerSkeleton />
        : items.length === 0 ? <p className="aq-cheer-empty"><Glyph name="heart" size={18} />아직 받은 응원이 없어요. 소개 페이지를 SNS에 걸어 보세요.</p>
        : (
          <ul className="aq-cheer-list">
            {items.map(c => (
              <CheerItem key={c.id} c={c} artist={artist} leaving={leaving === c.id}>
                <div className="aq-mgr-cheer-actions">
                  <button type="button" className={`ui-btn is-ghost is-icon is-sm aq-heart-btn${c.heart ? ' is-on' : ''}`} disabled={busy === c.id}
                    aria-pressed={c.heart} aria-label={c.heart ? '하트 취소' : '하트'} onClick={() => void patch(c, { heart: !c.heart })}>
                    <span key={c.heart ? 'on' : 'off'} className={popped === c.id && c.heart ? 'aq-heart-pop' : undefined}><Glyph name={c.heart ? 'heart-fill' : 'heart'} size={16} /></span>
                  </button>
                  <button type="button" className="ui-btn is-ghost is-icon is-sm" disabled={busy === c.id}
                    aria-label={c.hidden ? '다시 보이기' : '숨기기'} title={c.hidden ? '다시 보이기' : '숨기기'} onClick={() => void patch(c, { hidden: !c.hidden })}>
                    <Glyph name={c.hidden ? 'eye' : 'eye-off'} size={16} />
                  </button>
                  <button type="button" className="ui-btn is-ghost is-icon is-sm" disabled={busy === c.id} aria-label="지우기" title="지우기" onClick={() => void remove(c)}>
                    <Glyph name="trash" size={16} />
                  </button>
                </div>
              </CheerItem>
            ))}
          </ul>
        )}
      {more && <button type="button" className="ui-btn is-block is-soft" onClick={() => void load(items?.[items.length - 1]?.id)}>더 보기</button>}
    </div>
  );
}

