// 스마트링크 — 발매 하나의 공유 페이지(/l/코드). 플랫폼별 링크를 붙여 넣으면 한 주소로 모아 보여 주고,
// 최근 30일 조회·클릭 수를 보여 준다. 링크는 각 플랫폼 주소(https)만 받는다.
// 플랫폼이 공개하며 알려 준 앨범 ID가 있으면 서버가 그 링크를 채우고(auto), 붙여 넣은 링크는 그 앨범과 맞는지 바로 확인한다.
// 공식 오픈 전(verify=test)에는 다른 곡 링크도 저장되고 안내만 하며, 오픈 뒤(strict)에는 서버가 저장을 막는다.
import { useCallback, useEffect, useRef, useState } from 'react';
import { API_BASE, orgPath, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { MOCK } from '../lib/mode';
import { PLATFORM_NAMES } from '../lib/platformNames';
import { useToast } from './Toast';
import { ManageHead } from './ManageHead';
import { Reveal } from '../ui';
import { Glyph } from './Glyph';
import { CopyButton } from './CopyButton';
import { QrButton } from './QrButton';
import { PromoImageButton } from './PromoImageButton';
import { EmbedButton } from './EmbedButton';
import { todayStr } from '../lib/date';

type Check = 'verified' | 'mismatch' | 'unverified' | 'not_release' | 'invalid';
interface State {
  link: { slug: string; enabled: boolean; links: Record<string, string>; show_lyrics?: boolean } | null;
  auto?: Record<string, string>;
  checks?: Record<string, Check>;
  verify?: 'strict' | 'test';
  stats: { views: number; clicks: Record<string, number>; days: number; alerts?: number; alerts_waiting?: number; presaves?: Record<string, { total: number; saved: number }> };
  platforms: string[]; can_create: boolean;
}

/** 칸 옆 안내 — 막히는 것(빨강)과 확인된 것(초록)만 색을 쓴다 */
function checkNote(c: Check | undefined, strict: boolean): { tone: 'ok' | 'warn' | 'bad'; text: string } | null {
  switch (c) {
    case 'verified': return { tone: 'ok', text: '이 발매의 앨범이에요' };
    case 'mismatch': return strict
      ? { tone: 'bad', text: '이 발매가 아닌 다른 앨범이에요' }
      : { tone: 'warn', text: '다른 앨범이에요 · 테스트 중이라 저장은 돼요' };
    case 'not_release': return strict
      ? { tone: 'bad', text: '앨범·곡 페이지 주소를 넣어 주세요' }
      : { tone: 'warn', text: '앨범·곡 페이지가 아니에요 · 테스트 중이라 저장은 돼요' };
    case 'invalid': return { tone: 'bad', text: '이 플랫폼의 https 주소가 아니에요' };
    default: return null;
  }
}

const pageUrl = (slug: string) => `${window.location.origin}/l/${slug}`;

/** 홍보 이미지에 넣을 발매 정보 (발매 상세가 넘긴다) */
export interface PromoRelease { title: string; artist: string; releaseDate: string | null; cover?: string }

export function SmartLinkCard({ releaseId, promo }: { releaseId: string; promo?: PromoRelease }) {
  const toast = useToast();
  const [st, setSt] = useState<State | null>(null);
  const [edit, setEdit] = useState(false);
  const [links, setLinks] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [lyrics, setLyrics] = useState(false);
  const [checks, setChecks] = useState<Record<string, Check>>({});
  const checkSeq = useRef(0);
  const base = useCallback(() => orgPath(`/releases/${encodeURIComponent(releaseId)}/smart-link`), [releaseId]);
  // 입력이 멈추면 붙여 넣은 링크를 서버에서 확인한다 (저장 전에 칸마다 맞는지 보여 준다)
  useEffect(() => {
    if (!edit || MOCK) return;
    const filled = Object.fromEntries(Object.entries(links).filter(([, v]) => v.trim()));
    const seq = ++checkSeq.current;
    if (!Object.keys(filled).length) { setChecks({}); return; }
    const t = window.setTimeout(() => {
      req<{ checks: Record<string, Check> }>(`${base()}/check`, { method: 'POST', body: { links: filled }, quiet401: true })
        .then(r => { if (seq === checkSeq.current) setChecks(r.checks); })
        .catch(() => { /* 확인을 못 해도 저장 때 서버가 다시 본다 */ });
    }, 400);
    return () => window.clearTimeout(t);
  }, [edit, links, base]);
  const load = useCallback(async () => {
    if (MOCK) return;
    try { setSt(await req<State>(base(), { quiet401: true })); } catch { /* 볼 수 없으면 숨긴다 */ }
  }, [base]);
  useEffect(() => { void load(); }, [load]);
  if (!st || (!st.link && !st.can_create)) return null;
  const l = st.link;

  // showLyrics를 빼면 서버가 지금 선택을 그대로 둔다 (공개 끄기·다시 공개)
  const save = async (enabled: boolean, next: Record<string, string>, showLyrics?: boolean) => {
    setBusy(true);
    try {
      await req(base(), { method: 'PUT', body: { enabled, links: next, ...(showLyrics === undefined ? {} : { show_lyrics: showLyrics }) } });
      toast(enabled ? '스마트링크를 저장했어요.' : '스마트링크를 껐어요.', 'success');
      setEdit(false);
      await load();
    } catch (e) {
      toast(errorMessage(e), 'error');
    } finally {
      setBusy(false);
    }
  };
  const start = () => { setLinks({ ...(l?.links ?? {}) }); setLyrics(l?.show_lyrics ?? false); setChecks(st.checks ?? {}); setEdit(true); };
  const clicks = Object.values(st.stats.clicks).reduce((a, b) => a + b, 0);
  const presaveTotal = Object.values(st.stats.presaves ?? {}).reduce((a, b) => a + b.total, 0);
  const presaveSaved = Object.values(st.stats.presaves ?? {}).reduce((a, b) => a + b.saved, 0);
  const auto = st.auto ?? {};
  const autoCount = Object.keys(auto).filter(p => !l?.links[p]).length;
  const strict = st.verify === 'strict';
  const blocked = strict && Object.entries(checks).some(([p, c]) => (links[p] ?? '').trim() && (c === 'mismatch' || c === 'not_release' || c === 'invalid'));

  return (
    <section className="ui-card aq-manage-card aq-smartlink-edit" aria-labelledby="smartlinkHeading">
      <ManageHead id="smartlinkHeading" icon="link" title="스마트링크" status={l ? (l.enabled ? { tone: 'green', label: '공개 중' } : { tone: 'gray', label: '꺼짐' }) : null} />
      <p className="ui-field-help">플랫폼별 앨범 링크를 한 주소로 모아 SNS·프로필에 공유할 수 있어요.</p>
      {l && (
        <>
          {l.enabled ? (
            <div className="aq-smartlink-url">
              <code>{pageUrl(l.slug)}</code>
              <span className="ui-actions">
                <CopyButton text={pageUrl(l.slug)} />
                <QrButton url={pageUrl(l.slug)} title="스마트링크 QR 코드" filename={`smartlink-${l.slug}.png`} />
                {promo && <PromoImageButton input={{ ...promo, url: pageUrl(l.slug) }} filename={`audeniq-${l.slug}`} />}
                {promo && <EmbedButton input={{ url: pageUrl(l.slug), title: promo.title, artist: promo.artist, released: !promo.releaseDate || promo.releaseDate <= todayStr(),
                  cover: promo.cover ? `${API_BASE || window.location.origin}/api/smart-links/${l.slug}/cover` : undefined }} />}
                <a className="ui-btn is-sm" href={`/l/${l.slug}`} target="_blank" rel="noopener noreferrer"><Glyph name="arrow-up-right" size={14} />열기</a>
              </span>
            </div>
          ) : <p className="small muted">꺼져 있어요. 켜면 같은 주소로 다시 열려요.</p>}
          <p className="small aq-manage-stat">최근 {st.stats.days}일 · 조회 <b>{st.stats.views.toLocaleString()}</b> · 클릭 <b>{clicks.toLocaleString()}</b>
            {clicks > 0 && <span className="muted"> ({Object.entries(st.stats.clicks).sort((a, b) => b[1] - a[1]).map(([p, n]) => `${PLATFORM_NAMES[p] ?? p} ${n}`).join(', ')})</span>}
          </p>
          {autoCount > 0 && <p className="small muted">플랫폼에 공개된 {autoCount}곳은 링크를 자동으로 채웠어요.</p>}
          {(st.stats.alerts ?? 0) > 0 && (
            <p className="small">발매 알림 신청 <b>{(st.stats.alerts ?? 0).toLocaleString()}</b>명
              {(st.stats.alerts_waiting ?? 0) > 0 && <span className="muted"> · 발매일에 {st.stats.alerts_waiting}명에게 메일을 보내요</span>}</p>
          )}
          {presaveTotal > 0 && (
            <p className="small">미리 저장 <b>{presaveTotal.toLocaleString()}</b>명
              <span className="muted"> ({Object.entries(st.stats.presaves ?? {}).sort((a, b) => b[1].total - a[1].total).map(([p, n]) => `${PLATFORM_NAMES[p] ?? p} ${n.total}`).join(', ')}
                {presaveSaved > 0 ? ` · 보관함에 저장됨 ${presaveSaved}` : ' · 발매되는 순간 보관함에 저장돼요'})</span></p>
          )}
        </>
      )}
      <Reveal open={!edit} focus="first">
        <div className="ui-actions">
          <button type="button" className={`ui-btn${l ? '' : ' is-primary'}`} aria-expanded={edit} onClick={start}>{l ? '링크 수정' : '스마트링크 만들기'}</button>
          {l && <button type="button" className="ui-btn is-text is-muted" disabled={busy} onClick={() => void save(!l.enabled, l.links)}>{l.enabled ? '공개 끄기' : '다시 공개'}</button>}
        </div>
      </Reveal>
      <Reveal open={edit}>
        <div className="aq-correction-form">
          <div className="aq-smartlink-rows">
            {st.platforms.map(p => {
              const own = (links[p] ?? '').trim();
              const note = own ? checkNote(checks[p], strict) : null;
              const noteId = `sl-${p}-note`;
              return (
                <div key={p} className="aq-smartlink-row">
                  <label htmlFor={`sl-${p}`}>{PLATFORM_NAMES[p] ?? p}</label>
                  <input id={`sl-${p}`} className="ui-input" type="url" inputMode="url" maxLength={500}
                    placeholder={auto[p] ?? 'https://'} aria-invalid={note?.tone === 'bad' || undefined}
                    aria-describedby={note || (!own && auto[p]) ? noteId : undefined}
                    value={links[p] ?? ''} onChange={e => setLinks(x => ({ ...x, [p]: e.target.value }))} />
                  <small>{st.stats.clicks[p] ? `${st.stats.clicks[p]}회` : ''}</small>
                  {note
                    ? <span id={noteId} className={`aq-smartlink-note is-${note.tone}`}>{note.text}</span>
                    : !own && auto[p] && <span id={noteId} className="aq-smartlink-note is-auto">자동으로 채워져요 · 비워 두면 이 링크를 써요</span>}
                </div>
              );
            })}
          </div>
          <p className="small muted">발매 전이면 링크 없이 먼저 만들어 두세요. 플랫폼에 공개되면 링크가 자동으로 채워지고, 직접 넣은 링크가 있으면 그 링크를 먼저 보여 줘요.</p>
          <div className="aq-alert-pref">
            <span className="aq-alert-pref-icon" aria-hidden="true"><Glyph name="quote" size={18} /></span>
            <span className="min-0">
              <strong>가사 공개</strong>
              <span>발매 신청 때 입력한 가사를 스마트링크에서 곡별로 펼쳐 볼 수 있어요.</span>
            </span>
            <label className="aq-switch">
              <input type="checkbox" role="switch" aria-label="스마트링크에 가사 공개" checked={lyrics} onChange={e => setLyrics(e.target.checked)} />
              <span aria-hidden="true" />
            </label>
          </div>
          <div className="aq-confirm-actions">
            <button type="button" className="ui-btn is-soft" onClick={() => setEdit(false)} disabled={busy}>닫기</button>
            <button type="button" className="ui-btn is-primary" onClick={() => void save(l?.enabled ?? true, links, lyrics)} disabled={busy || blocked}>{busy ? '저장 중' : '저장'}</button>
          </div>
        </div>
      </Reveal>
    </section>
  );
}
