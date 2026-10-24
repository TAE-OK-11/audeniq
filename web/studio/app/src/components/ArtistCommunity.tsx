// 아티스트 소개 페이지의 소식(공지·이벤트)과 팬 응원 — 공개 페이지(ArtistPage·ArtistNews)와 스튜디오 미리보기가 같이 쓴다.
// 응원은 계정 없이 닉네임과 한마디. 아티스트가 숨기거나 지울 수 있고, 하트를 누르면 ‘아티스트가 좋아해요’로 보인다.
import { useEffect, useRef, useState } from 'react';
import { req } from '../api/http';
import { errorMessage } from '../api/errors';
import { MOCK } from '../lib/mode';
import { gradientFor } from '../lib/catalog';
import { shortUrl } from '../lib/ownLinks';
import {
  CHEER_MAX, KIND_LABEL, MOCK_CHEERS, NICK_MAX, ago, dday, eventWhen,
  type ArtistPost, type Cheer,
} from '../lib/artistCommunity';
import { Glyph } from './Glyph';

/** 소식 한 장 — 이벤트면 날짜·장소·D-day, 링크가 있으면 ‘예매·자세히’ 버튼 */
export function PostCard({ post, artist, clamp = false }: { post: ArtistPost; artist?: string; clamp?: boolean }) {
  const event = post.kind === 'EVENT' && post.starts_at;
  const d = event ? dday(post.starts_at as string) : null;
  return (
    <article className={`aq-post${d?.past ? ' is-past' : ''}`}>
      <div className="aq-post-tags">
        <span className={`aq-post-kind is-${post.kind.toLowerCase()}`}>
          <Glyph name={post.kind === 'EVENT' ? 'calendar' : 'megaphone'} size={12} />{KIND_LABEL[post.kind]}
        </span>
        {post.pinned && <span className="aq-post-pin"><Glyph name="pin" size={12} />고정</span>}
        {d && <span className={`aq-post-dday${d.past ? ' is-past' : ''}`}>{d.text}</span>}
        {!event && <time className="aq-post-ago" dateTime={post.created_at}>{ago(post.created_at)}</time>}
      </div>
      <h3 className="aq-post-title">{post.title}</h3>
      {event && (
        <p className="aq-post-when">
          <span><Glyph name="clock" size={14} />{eventWhen(post.starts_at as string)}</span>
          {post.place && <span><Glyph name="globe" size={14} />{post.place}</span>}
        </p>
      )}
      {post.body && <p className={`aq-post-body${clamp ? ' is-clamp' : ''}`}>{post.body}</p>}
      {post.url && (
        <a className="aq-post-link" href={post.url} target="_blank" rel="noopener noreferrer nofollow"
          aria-label={`${post.title}${artist ? ` · ${artist}` : ''} ${event ? '예매·자세히' : '자세히'} (${shortUrl(post.url)})`}>
          <span>{event && !d?.past ? '예매·자세히 보기' : '자세히 보기'}</span>
          <small>{shortUrl(post.url)}</small>
          <Glyph name="arrow-up-right" size={14} />
        </a>
      )}
    </article>
  );
}

const NICK_KEY = 'aq-cheer-nickname';
const remembered = () => { try { return localStorage.getItem(NICK_KEY) ?? ''; } catch { return ''; } };
const remember = (v: string) => { try { localStorage.setItem(NICK_KEY, v); } catch { /* 저장이 막혀 있으면 다음에 다시 적으면 된다 */ } };

/** 팬 응원 — 남기기 + 최신순 목록(30개씩 더 보기). 꺼져 있으면 안내만 */
export function CheerBoard({ slug, artist, total, enabled }: { slug: string; artist: string; total: number; enabled: boolean }) {
  const [items, setItems] = useState<Cheer[] | null>(null);
  const [more, setMore] = useState(false);
  const [count, setCount] = useState(total);
  const [loading, setLoading] = useState(false);
  const [nickname, setNickname] = useState(remembered);
  const [message, setMessage] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [sent, setSent] = useState<string | null>(null);
  const path = `/api/artist-links/${encodeURIComponent(slug)}/cheers`;
  const msgRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (!enabled) return;
    if (MOCK) { setItems(MOCK_CHEERS); return; }
    req<{ items: Cheer[]; more: boolean }>(path, { quiet401: true })
      .then(r => { setItems(r.items); setMore(r.more); }, () => setItems([]));
  }, [path, enabled]);

  const loadMore = async () => {
    const last = items?.[items.length - 1];
    if (!last || loading) return;
    setLoading(true);
    try {
      const r = await req<{ items: Cheer[]; more: boolean }>(`${path}?before=${encodeURIComponent(last.id)}`, { quiet401: true });
      setItems(cur => [...(cur ?? []), ...r.items.filter(c => !cur?.some(x => x.id === c.id))]);
      setMore(r.more);
    } catch { setMore(false); } finally { setLoading(false); }
  };

  const send = async (e: React.FormEvent) => {
    e.preventDefault();
    const nick = nickname.trim();
    const msg = message.trim();
    if (!nick) { setError('닉네임을 적어 주세요.'); document.getElementById('cheerNick')?.focus(); return; }
    if (!msg) { setError('응원 메시지를 적어 주세요.'); msgRef.current?.focus(); return; }
    setBusy(true); setError('');
    try {
      const c = MOCK
        ? { id: `m${Date.now()}`, nickname: nick, message: msg, heart: false, created_at: new Date().toISOString() }
        : await req<Cheer>(path, { method: 'POST', body: { nickname: nick, message: msg }, quiet401: true });
      setItems(cur => [c, ...(cur ?? [])]);
      setCount(n => n + 1);
      setMessage('');
      remember(nick);
      setSent(c.id);
    } catch (err) { setError(errorMessage(err)); } finally { setBusy(false); }
  };

  if (!enabled) return null;
  return (
    <section className="aq-cheers" aria-labelledby="cheersTitle">
      <h2 id="cheersTitle" className="aq-smartlink-list-title">팬 응원 <span>{count.toLocaleString()}</span></h2>
      <form className="aq-cheer-form" onSubmit={send} noValidate>
        <div className="aq-cheer-row">
          <input id="cheerNick" className="ui-input" aria-label="닉네임" placeholder="닉네임" maxLength={NICK_MAX}
            value={nickname} onChange={e => { setNickname(e.target.value); setError(''); }} autoComplete="nickname" />
        </div>
        <textarea ref={msgRef} className="ui-input aq-cheer-msg" aria-label={`${artist}에게 응원 메시지`} rows={3} maxLength={CHEER_MAX}
          placeholder={`${artist}에게 응원 한마디를 남겨 주세요.`} value={message} onChange={e => { setMessage(e.target.value); setError(''); }} />
        <div className="aq-cheer-foot">
          <span className="aq-cheer-count">{message.length} / {CHEER_MAX}</span>
          <button type="submit" className={`ui-btn is-primary is-sm${busy ? ' is-busy' : ''}`} disabled={busy}>
            <Glyph name="heart" size={14} />{busy ? '남기는 중' : '응원 남기기'}
          </button>
        </div>
        {error && <p className="aq-smartlink-alert-err" role="alert">{error}</p>}
        {sent && !error && <p className="aq-cheer-done" role="status">응원을 남겼어요. 고마워요!</p>}
        <p className="aq-cheer-note">누구나 볼 수 있어요. 아티스트가 숨기거나 지울 수 있어요.</p>
      </form>
      {items === null ? <p className="muted small" aria-busy="true">불러오는 중…</p>
        : items.length === 0 ? <p className="aq-cheer-empty">첫 응원을 남겨 주세요.</p>
        : (
          <ul className="aq-cheer-list">
            {items.map(c => <CheerItem key={c.id} c={c} artist={artist} fresh={c.id === sent} />)}
          </ul>
        )}
      {more && (
        <button type="button" className={`ui-btn is-block aq-cheer-more${loading ? ' is-busy' : ''}`} onClick={() => void loadMore()} disabled={loading}>
          {loading ? '불러오는 중' : '응원 더 보기'}
        </button>
      )}
    </section>
  );
}

export function CheerItem({ c, artist, fresh = false, children }: { c: Cheer; artist: string; fresh?: boolean; children?: React.ReactNode }) {
  return (
    <li className={`aq-cheer${fresh ? ' is-fresh' : ''}${c.hidden ? ' is-hidden' : ''}`}>
      <span className="aq-cheer-avatar" style={{ background: gradientFor(c.nickname) }} aria-hidden="true">{c.nickname.trim().charAt(0)}</span>
      <div className="min-0">
        <p className="aq-cheer-head">
          <strong>{c.nickname}</strong>
          <time dateTime={c.created_at}>{ago(c.created_at)}</time>
          {c.hidden && <span className="aq-cheer-hidden-tag">숨김</span>}
        </p>
        <p className="aq-cheer-text">{c.message}</p>
        {c.heart && <p className="aq-cheer-heart"><Glyph name="heart-fill" size={12} />{artist}님이 좋아해요</p>}
      </div>
      {children}
    </li>
  );
}
