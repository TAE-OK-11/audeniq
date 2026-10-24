// 아티스트 소개 페이지의 소식(공지·이벤트)과 팬 응원 — 공개 페이지(ArtistPage·ArtistNews)와 스튜디오 관리 창이 같이 쓴다.
// 이벤트는 왼쪽에 날짜 칸(월·일·요일)을 두어 공연 티켓처럼, 긴 글은 세 줄에서 접고 ‘더 보기’로 부드럽게 편다.
// 응원은 계정 없이 닉네임과 한마디. 남기면 하트가 톡 튀고 새 응원이 맨 위에 옅게 빛나며 들어온다.
// 아티스트가 숨기거나 지울 수 있고, 하트를 누르면 ‘아티스트가 좋아해요’로 보인다.
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { req } from '../api/http';
import { errorMessage } from '../api/errors';
import { MOCK } from '../lib/mode';
import { gradientFor } from '../lib/catalog';
import { shortUrl } from '../lib/ownLinks';
import {
  CHEER_MAX, KIND_LABEL, MOCK_CHEERS, NICK_MAX, ago, dday, eventDay, eventWhen,
  type ArtistPost, type Cheer,
} from '../lib/artistCommunity';
import { CountUp } from '../ui';
import { Glyph } from './Glyph';

/** 세 줄 넘는 글 — 접어 두고 ‘더 보기’로 높이를 늘려 편다 */
function PostBody({ text, clamp }: { text: string; clamp: boolean }) {
  const ref = useRef<HTMLParagraphElement>(null);
  const [over, setOver] = useState(false);
  const [open, setOpen] = useState(false);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!clamp || !el) return;
    setOver(el.scrollHeight > el.clientHeight + 2);
  }, [text, clamp]);
  const expand = () => {
    const el = ref.current;
    if (!el) { setOpen(true); return; }
    const from = el.clientHeight;
    setOpen(true);
    // 접힌 높이 → 전체 높이로 (WAAPI, 동작 줄이기면 바로)
    requestAnimationFrame(() => {
      if (window.matchMedia?.('(prefers-reduced-motion: reduce)').matches || !el.animate) return;
      el.animate([{ height: `${from}px` }, { height: `${el.scrollHeight}px` }], { duration: 320, easing: 'cubic-bezier(.32,.72,0,1)' });
    });
  };
  return (
    <>
      <p ref={ref} className={`aq-post-body${clamp && !open ? ' is-clamp' : ''}`}>{text}</p>
      {clamp && over && !open && (
        <button type="button" className="aq-post-more" onClick={expand}>더 보기<Glyph name="chevron-down" size={14} /></button>
      )}
    </>
  );
}

/** 소식 한 장 — 이벤트면 날짜 칸·시간·장소·D-day, 링크가 있으면 ‘예매·자세히’ 줄 */
export function PostCard({ post, artist, clamp = false }: { post: ArtistPost; artist?: string; clamp?: boolean }) {
  const event = post.kind === 'EVENT' && post.starts_at ? post.starts_at : null;
  const d = event ? dday(event) : null;
  const day = event ? eventDay(event) : null;
  return (
    <article className={`aq-post is-${post.kind.toLowerCase()}${post.pinned ? ' is-pinned' : ''}${d?.past ? ' is-past' : ''}`}>
      <div className="aq-post-top">
        {day && (
          <span className="aq-post-date" aria-hidden="true">
            <small>{day.month}월</small><strong>{day.day}</strong><small>{day.weekday}</small>
          </span>
        )}
        <div className="aq-post-headline">
          <div className="aq-post-tags">
            <span className={`aq-post-kind is-${post.kind.toLowerCase()}`}>
              <Glyph name={event ? 'calendar' : 'megaphone'} size={12} />{KIND_LABEL[post.kind]}
            </span>
            {post.pinned && <span className="aq-post-pin"><Glyph name="pin" size={12} />고정</span>}
            {d && <span className={`aq-post-dday${d.past ? ' is-past' : d.text === '오늘' ? ' is-today' : ''}`}>{d.text}</span>}
            {!event && <time className="aq-post-ago" dateTime={post.created_at}>{ago(post.created_at)}</time>}
          </div>
          <h3 className="aq-post-title">{post.title}</h3>
        </div>
      </div>
      {event && (
        <p className="aq-post-when">
          <span><Glyph name="clock" size={14} /><time dateTime={event}>{eventWhen(event)}</time></span>
          {post.place && <span><Glyph name="globe" size={14} />{post.place}</span>}
        </p>
      )}
      {post.body && <PostBody text={post.body} clamp={clamp} />}
      {post.url && (
        <a className="aq-post-link" href={post.url} target="_blank" rel="noopener noreferrer nofollow"
          aria-label={`${post.title}${artist ? ` · ${artist}` : ''} ${event && !d?.past ? '예매·자세히' : '자세히'} (${shortUrl(post.url)})`}>
          <span>{event && !d?.past ? '예매·자세히 보기' : '자세히 보기'}</span>
          <small>{shortUrl(post.url)}</small>
          <Glyph name="arrow-up-right" size={14} />
        </a>
      )}
    </article>
  );
}

/** 불러오는 동안 — 응원 줄 모양 그대로 반짝이는 자리 */
export function CheerSkeleton({ rows = 3 }: { rows?: number }) {
  return (
    <ul className="aq-cheer-list is-skeleton" role="status" aria-label="불러오는 중">
      {Array.from({ length: rows }, (_, i) => (
        <li key={i} className="aq-cheer" style={{ animationDelay: `${i * 70}ms` }}>
          <span className="aq-cheer-avatar aq-skel" />
          <span className="aq-skel-lines"><span className="aq-skel" /><span className="aq-skel" /></span>
        </li>
      ))}
    </ul>
  );
}

const NICK_KEY = 'aq-cheer-nickname';
const remembered = () => { try { return localStorage.getItem(NICK_KEY) ?? ''; } catch { return ''; } };
const remember = (v: string) => { try { localStorage.setItem(NICK_KEY, v); } catch { /* 저장이 막혀 있으면 다음에 다시 적으면 된다 */ } };

/** 팬 응원 — 남기기 + 최신순 목록(30개씩 더 보기). 꺼져 있으면 아무것도 보이지 않는다 */
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
  const formRef = useRef<HTMLFormElement>(null);

  useEffect(() => {
    if (!enabled) return;
    if (MOCK) { const t = window.setTimeout(() => setItems(MOCK_CHEERS), 350); return () => window.clearTimeout(t); }
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

  // 막히면 폼을 살짝 흔든다 (스튜디오 입력 오류와 같은 움직임)
  const fail = (msg: string, focus?: HTMLElement | null) => {
    setError(msg);
    focus?.focus();
    if (!window.matchMedia?.('(prefers-reduced-motion: reduce)').matches) {
      formRef.current?.animate?.([{ transform: 'none' }, { transform: 'translateX(-4px)' }, { transform: 'translateX(3px)' }, { transform: 'translateX(-1px)' }, { transform: 'none' }], { duration: 320, easing: 'ease-out' });
    }
  };
  const send = async (e: React.FormEvent) => {
    e.preventDefault();
    const nick = nickname.trim();
    const msg = message.trim();
    if (!nick) { fail('닉네임을 적어 주세요.', document.getElementById('cheerNick')); return; }
    if (!msg) { fail('응원 메시지를 적어 주세요.', msgRef.current); return; }
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
    } catch (err) { fail(errorMessage(err)); } finally { setBusy(false); }
  };

  if (!enabled) return null;
  const left = CHEER_MAX - message.length;
  return (
    <section className="aq-cheers" aria-labelledby="cheersTitle">
      <h2 id="cheersTitle" className="aq-smartlink-list-title">팬 응원{count > 0 && <> <span><CountUp value={count} format={n => n.toLocaleString()} /></span></>}</h2>
      <form ref={formRef} className="aq-cheer-form" onSubmit={send} noValidate>
        <input id="cheerNick" className="ui-input" aria-label="닉네임" placeholder="닉네임" maxLength={NICK_MAX}
          value={nickname} onChange={e => { setNickname(e.target.value); setError(''); }} autoComplete="nickname" />
        <textarea ref={msgRef} className="ui-input aq-cheer-msg" aria-label={`${artist}에게 응원 메시지`} rows={3} maxLength={CHEER_MAX}
          placeholder={count > 0 ? `${artist}에게 응원 한마디를 남겨 주세요.` : `${artist}에게 첫 응원을 남겨 주세요.`} value={message}
          onChange={e => { setMessage(e.target.value); setError(''); setSent(null); }} />
        <div className="aq-cheer-foot">
          <span className={`aq-cheer-count${left <= 30 ? ' is-near' : ''}`}>{message.length} / {CHEER_MAX}</span>
          <button type="submit" className={`ui-btn is-primary is-sm aq-cheer-send${busy ? ' is-busy' : ''}`} disabled={busy}>
            <span key={sent ?? 'idle'} className={`aq-cheer-send-heart${sent ? ' is-pop' : ''}`}><Glyph name={sent ? 'heart-fill' : 'heart'} size={14} /></span>
            {busy ? '남기는 중' : '응원 남기기'}
          </button>
        </div>
        {error && <p className="aq-smartlink-alert-err" role="alert">{error}</p>}
        {sent && !error && <p className="aq-cheer-done" role="status">응원을 남겼어요. 고마워요!</p>}
        <p className="aq-cheer-note">누구나 볼 수 있어요. 아티스트가 숨기거나 지울 수 있어요.</p>
      </form>
      {items === null ? <CheerSkeleton />
        // 아직 없으면 빈 상자를 두지 않는다 — 바로 위 폼이 이미 ‘응원을 남겨 주세요’라고 말한다
        : items.length === 0 ? null
        : (
          <ul className="aq-cheer-list">
            {items.map(c => <CheerItem key={c.id} c={c} artist={artist} fresh={c.id === sent} />)}
          </ul>
        )}
      {more && (
        <button type="button" className={`ui-btn is-block is-soft aq-cheer-more${loading ? ' is-busy' : ''}`} onClick={() => void loadMore()} disabled={loading}>
          {loading ? '불러오는 중' : '응원 더 보기'}
        </button>
      )}
    </section>
  );
}

export function CheerItem({ c, artist, fresh = false, leaving = false, children }: {
  c: Cheer; artist: string; fresh?: boolean; leaving?: boolean; children?: React.ReactNode;
}) {
  return (
    <li className={`aq-cheer${fresh ? ' is-fresh' : ''}${c.hidden ? ' is-hidden' : ''}${leaving ? ' is-leaving' : ''}`}>
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
