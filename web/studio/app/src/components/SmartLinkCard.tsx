// 스마트링크 — 발매 하나의 공유 페이지(/l/코드). 플랫폼별 링크를 붙여 넣으면 한 주소로 모아 보여 주고,
// 최근 30일 조회·클릭 수를 보여 준다. 링크는 각 플랫폼 주소(https)만 받는다.
import { useCallback, useEffect, useState } from 'react';
import { orgPath, req } from '../api/http';
import { errorMessage } from '../api/errors';
import { MOCK } from '../lib/mode';
import { PLATFORM_NAMES } from '../lib/platformNames';
import { useToast } from './Toast';
import { ManageHead } from './ManageHead';
import { Glyph } from './Glyph';
import { CopyButton } from './CopyButton';

interface State {
  link: { slug: string; enabled: boolean; links: Record<string, string> } | null;
  stats: { views: number; clicks: Record<string, number>; days: number; alerts?: number; alerts_waiting?: number };
  platforms: string[]; can_create: boolean;
}

const pageUrl = (slug: string) => `${window.location.origin}/l/${slug}`;

export function SmartLinkCard({ releaseId }: { releaseId: string }) {
  const toast = useToast();
  const [st, setSt] = useState<State | null>(null);
  const [edit, setEdit] = useState(false);
  const [links, setLinks] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const base = useCallback(() => orgPath(`/releases/${encodeURIComponent(releaseId)}/smart-link`), [releaseId]);
  const load = useCallback(async () => {
    if (MOCK) return;
    try { setSt(await req<State>(base(), { quiet401: true })); } catch { /* 볼 수 없으면 숨긴다 */ }
  }, [base]);
  useEffect(() => { void load(); }, [load]);
  if (!st || (!st.link && !st.can_create)) return null;
  const l = st.link;

  const save = async (enabled: boolean, next: Record<string, string>) => {
    setBusy(true);
    try {
      await req(base(), { method: 'PUT', body: { enabled, links: next } });
      toast(enabled ? '스마트링크를 저장했어요.' : '스마트링크를 껐어요.', 'success');
      setEdit(false);
      await load();
    } catch (e) {
      toast(errorMessage(e), 'error');
    } finally {
      setBusy(false);
    }
  };
  const start = () => { setLinks({ ...(l?.links ?? {}) }); setEdit(true); };
  const clicks = Object.values(st.stats.clicks).reduce((a, b) => a + b, 0);

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
                <a className="ui-btn is-sm" href={`/l/${l.slug}`} target="_blank" rel="noopener noreferrer"><Glyph name="arrow-up-right" size={14} />열기</a>
              </span>
            </div>
          ) : <p className="small muted">꺼져 있어요. 켜면 같은 주소로 다시 열려요.</p>}
          <p className="small aq-manage-stat">최근 {st.stats.days}일 · 조회 <b>{st.stats.views.toLocaleString()}</b> · 클릭 <b>{clicks.toLocaleString()}</b>
            {clicks > 0 && <span className="muted"> ({Object.entries(st.stats.clicks).sort((a, b) => b[1] - a[1]).map(([p, n]) => `${PLATFORM_NAMES[p] ?? p} ${n}`).join(', ')})</span>}
          </p>
          {(st.stats.alerts ?? 0) > 0 && (
            <p className="small">발매 알림 신청 <b>{(st.stats.alerts ?? 0).toLocaleString()}</b>명
              {(st.stats.alerts_waiting ?? 0) > 0 && <span className="muted"> · 발매일에 {st.stats.alerts_waiting}명에게 메일을 보내요</span>}</p>
          )}
        </>
      )}
      {!edit && (
        <div className="ui-actions">
          <button type="button" className={`ui-btn${l ? '' : ' is-primary'}`} onClick={start}>{l ? '링크 수정' : '스마트링크 만들기'}</button>
          {l && <button type="button" className="ui-btn is-text is-muted" disabled={busy} onClick={() => void save(!l.enabled, l.links)}>{l.enabled ? '공개 끄기' : '다시 공개'}</button>}
        </div>
      )}
      {edit && (
        <div className="aq-correction-form">
          <div className="aq-smartlink-rows">
            {st.platforms.map(p => (
              <div key={p} className="aq-smartlink-row">
                <label htmlFor={`sl-${p}`}>{PLATFORM_NAMES[p] ?? p}</label>
                <input id={`sl-${p}`} className="ui-input" type="url" inputMode="url" maxLength={500} placeholder="https://"
                  value={links[p] ?? ''} onChange={e => setLinks(x => ({ ...x, [p]: e.target.value }))} />
                <small>{st.stats.clicks[p] ? `${st.stats.clicks[p]}회` : ''}</small>
              </div>
            ))}
          </div>
          <p className="small muted">발매 전이면 링크 없이 먼저 만들어 두고, 플랫폼에 올라오면 링크를 채워 주세요.</p>
          <div className="aq-confirm-actions">
            <button type="button" className="ui-btn is-soft" onClick={() => setEdit(false)} disabled={busy}>닫기</button>
            <button type="button" className="ui-btn is-primary" onClick={() => void save(l?.enabled ?? true, links)} disabled={busy}>{busy ? '저장 중' : '저장'}</button>
          </div>
        </div>
      )}
    </section>
  );
}
