// 아티스트 페이지 > 보도자료 탭 — 기자·큐레이터·공연 기획자가 그대로 가져다 쓰는 자료를 만든다.
// 보도용 소개(한 줄 소개·짧은 소개·사실·주요 활동·인용)와 보도자료(발매를 고르면 발매 정보로 초안을 채움, 공개하면 보도자료 페이지에).
import { useCallback, useEffect, useState } from 'react';
import { errorMessage } from '../api/errors';
import { artistPressUrl, releaseUrl } from '../lib/publicLinks';
import {
  EMPTY_PROFILE, GUIDE_MARK, LIMITS, deleteRelease, draftFor, fetchPress, fullText, koreanDate, saveProfile, saveRelease,
  type PressDraft, type PressProfile, type PressRelease,
} from '../lib/artistPress';
import type { ReleaseInsight } from '../lib/artistStudio';
import { Badge, Callout, SkeletonList } from '../ui';
import { Glyph } from './Glyph';
import { Modal, useModalClose } from './Modal';
import { useToast } from './Toast';
import { useConfirm } from './Confirm';
import { CopyButton } from './CopyButton';

function Count({ n, max }: { n: number; max: number }) {
  return <span className={`aq-al-count${n > max ? ' is-over' : ''}`}>{n} / {max}</span>;
}

export function PressKitManager({ artistId, artist, slug, contact, releases, onCreatePage }: {
  artistId: string; artist: string; slug: string | null; contact: string; releases: ReleaseInsight[]; onCreatePage: () => void;
}) {
  const toast = useToast();
  const [profile, setProfile] = useState<PressProfile | null>(null);
  const [list, setList] = useState<PressRelease[]>([]);
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState<PressRelease | 'new' | null>(null);

  const load = useCallback(async () => {
    if (!slug) return;
    try {
      const r = await fetchPress(artistId);
      setProfile(r.profile); setList(r.releases);
    } catch (e) { toast(errorMessage(e), 'error'); setProfile({ ...EMPTY_PROFILE }); }
  }, [artistId, slug, toast]);
  useEffect(() => { void load(); }, [load]);

  if (!slug) {
    return (
      <Callout action={<button type="button" className="ui-btn is-sm" onClick={onCreatePage}>만들기</button>}>
        보도자료 페이지는 소개 페이지 주소에 붙어요. 소개 페이지를 먼저 만들어 주세요.
      </Callout>
    );
  }
  if (!profile) return <SkeletonList rows={4} />;

  const set = <K extends keyof PressProfile>(k: K, v: PressProfile[K]) => setProfile(p => (p ? { ...p, [k]: v } : p));
  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try { setProfile(await saveProfile(artistId, profile)); toast('보도용 소개를 저장했어요. 보도자료 페이지에 바로 보여요.', 'success'); }
    catch (err) { toast(errorMessage(err), 'error'); } finally { setBusy(false); }
  };
  const url = artistPressUrl(slug);

  return (
    <div className="aq-press-mgr">
      <section className="ui-card aq-studio-block aq-press-share" aria-label="보도자료 페이지">
        <span className="min-0">
          <strong>보도자료 페이지</strong>
          <small>{url.replace(/^https?:\/\//, '')}</small>
        </span>
        <span className="aq-press-share-tools">
          <CopyButton text={url} label="주소 복사" icon="link" className="ui-btn is-sm" />
          <a className="ui-btn is-sm" href={url} target="_blank" rel="noopener noreferrer"><Glyph name="arrow-up-right" size={14} />열기</a>
        </span>
      </section>

      <form className="ui-card aq-studio-block aq-press-form" onSubmit={save} noValidate aria-labelledby="pressProfileTitle">
        <h3 id="pressProfileTitle">보도용 소개</h3>
        <p className="aq-studio-note">기자·큐레이터가 기사·소개 글에 그대로 옮겨 쓰는 내용이에요. 3인칭으로, 사실 위주로 써 주세요.</p>
        <div className="ui-field">
          <div className="aq-al-section-head"><label htmlFor="prTagline">한 줄 소개</label><Count n={profile.tagline.length} max={LIMITS.tagline} /></div>
          <input id="prTagline" className="ui-input" maxLength={LIMITS.tagline + 10} placeholder="예: 새벽의 도시를 노래하는 싱어송라이터"
            value={profile.tagline} onChange={e => set('tagline', e.target.value)} />
        </div>
        <div className="ui-field">
          <div className="aq-al-section-head"><label htmlFor="prBio">짧은 소개</label><Count n={profile.bio_short.length} max={LIMITS.bio_short} /></div>
          <textarea id="prBio" className="ui-textarea" rows={4} maxLength={LIMITS.bio_short + 20}
            placeholder="두세 문장으로. 예: 서울에서 활동하는 싱어송라이터. 도시의 풍경과 하루의 감정을 담담한 목소리로 기록한다."
            value={profile.bio_short} onChange={e => set('bio_short', e.target.value)} />
        </div>
        <div className="aq-press-facts-form">
          <div className="ui-field"><label htmlFor="prGenre">장르</label>
            <input id="prGenre" className="ui-input" maxLength={LIMITS.genre} placeholder="인디 팝" value={profile.genre} onChange={e => set('genre', e.target.value)} /></div>
          <div className="ui-field"><label htmlFor="prBased">활동 지역</label>
            <input id="prBased" className="ui-input" maxLength={LIMITS.based_in} placeholder="서울" value={profile.based_in} onChange={e => set('based_in', e.target.value)} /></div>
          <div className="ui-field"><label htmlFor="prLabel">소속</label>
            <input id="prLabel" className="ui-input" maxLength={LIMITS.label} placeholder="소속사·레이블 (없으면 비워 두기)" value={profile.label} onChange={e => set('label', e.target.value)} /></div>
          <div className="ui-field"><label htmlFor="prDebut">데뷔 연도</label>
            <input id="prDebut" className="ui-input" inputMode="numeric" maxLength={4} placeholder="2024" value={profile.debut}
              onChange={e => set('debut', e.target.value.replace(/\D/g, ''))} /></div>
        </div>

        <fieldset className="ui-group aq-press-list">
          <legend>주요 활동 <span className="aq-optional">선택</span></legend>
          <p className="ui-field-help">차트·공연·수상·방송처럼 확인할 수 있는 일만 한 줄씩 적어 주세요.</p>
          {profile.highlights.map((h, i) => (
            <div key={i} className="aq-press-row">
              <input className="ui-input" aria-label={`주요 활동 ${i + 1}`} maxLength={LIMITS.highlight} value={h}
                onChange={e => set('highlights', profile.highlights.map((x, j) => (j === i ? e.target.value : x)))} />
              <button type="button" className="ui-btn is-ghost is-icon is-sm" aria-label={`주요 활동 ${i + 1} 빼기`}
                onClick={() => set('highlights', profile.highlights.filter((_, j) => j !== i))}><Glyph name="close" size={14} /></button>
            </div>
          ))}
          {profile.highlights.length < LIMITS.highlights && (
            <button type="button" className="ui-btn is-soft is-sm" onClick={() => set('highlights', [...profile.highlights, ''])}><Glyph name="plus" size={14} />주요 활동 더하기</button>
          )}
        </fieldset>

        <fieldset className="ui-group aq-press-list">
          <legend>인용 <span className="aq-optional">선택</span></legend>
          <p className="ui-field-help">기사·리뷰·큐레이터가 남긴 말을 출처와 함께. 실제로 나온 말만 넣어 주세요.</p>
          {profile.quotes.map((q, i) => (
            <div key={i} className="aq-press-quote-row">
              <textarea className="ui-textarea" rows={2} aria-label={`인용 ${i + 1}`} maxLength={LIMITS.quote} placeholder="인용할 말" value={q.quote}
                onChange={e => set('quotes', profile.quotes.map((x, j) => (j === i ? { ...x, quote: e.target.value } : x)))} />
              <div className="aq-press-row">
                <input className="ui-input" aria-label={`인용 ${i + 1} 출처`} maxLength={LIMITS.source} placeholder="출처 (매체·이름)" value={q.source}
                  onChange={e => set('quotes', profile.quotes.map((x, j) => (j === i ? { ...x, source: e.target.value } : x)))} />
                <button type="button" className="ui-btn is-ghost is-icon is-sm" aria-label={`인용 ${i + 1} 빼기`}
                  onClick={() => set('quotes', profile.quotes.filter((_, j) => j !== i))}><Glyph name="close" size={14} /></button>
              </div>
            </div>
          ))}
          {profile.quotes.length < LIMITS.quotes && (
            <button type="button" className="ui-btn is-soft is-sm" onClick={() => set('quotes', [...profile.quotes, { quote: '', source: '' }])}><Glyph name="plus" size={14} />인용 더하기</button>
          )}
        </fieldset>
        <button type="submit" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy}>{busy ? '저장하는 중' : '보도용 소개 저장'}</button>
      </form>

      <section className="aq-set-card" aria-labelledby="pressReleasesTitle">
        <h2 id="pressReleasesTitle">보도자료 <small>{list.length}</small></h2>
        <p className="ui-field-help aq-set-help">발매를 고르면 발매 정보로 초안을 채워 드려요. 공개한 보도자료는 보도자료 페이지에서 누구나 전문을 복사해 갈 수 있어요.</p>
        {list.map(r => (
          <button key={r.id} type="button" className="aq-set-row" onClick={() => setEditing(r)}>
            <span className="aq-set-label aq-press-row-title">{r.title}</span>
            <span className="aq-set-value">
              <Badge tone={r.published ? 'green' : 'gray'}>{r.published ? '공개' : '초안'}</Badge>
            </span>
            <Glyph name="chevron-right" size={16} className="aq-set-chev" />
          </button>
        ))}
        <button type="button" className="ui-btn is-soft is-block aq-press-new" onClick={() => setEditing('new')}><Glyph name="plus" size={15} />새 보도자료 쓰기</button>
      </section>

      {editing && (
        <Modal title={editing === 'new' ? '새 보도자료' : '보도자료 고치기'} onClose={() => setEditing(null)} modalClass="aq-set-modal aq-press-modal">
          <ReleaseEditor
            artistId={artistId} artist={artist} profile={profile} contact={contact} releases={releases}
            item={editing === 'new' ? null : editing}
            onSaved={item => setList(l => [item, ...l.filter(x => x.id !== item.id)])}
            onDeleted={id => setList(l => l.filter(x => x.id !== id))}
          />
        </Modal>
      )}
    </div>
  );
}

function ReleaseEditor({ artistId, artist, profile, contact, releases, item, onSaved, onDeleted }: {
  artistId: string; artist: string; profile: PressProfile; contact: string; releases: ReleaseInsight[];
  item: PressRelease | null; onSaved: (r: PressRelease) => void; onDeleted: (id: string) => void;
}) {
  const close = useModalClose();
  const toast = useToast();
  const confirm = useConfirm();
  const [d, setD] = useState<PressDraft>(() => item
    ? { release_id: item.release_id, title: item.title, lead: item.lead, body: item.body, published: item.published }
    : { release_id: null, title: '', lead: '', body: '', published: false });
  const [busy, setBusy] = useState(false);
  const rel = releases.find(r => r.release_id === d.release_id) ?? null;

  const fill = async (releaseId: string | null) => {
    const r = releases.find(x => x.release_id === releaseId);
    if (!r) { setD(x => ({ ...x, release_id: null })); return; }
    const empty = !d.title.trim() && !d.lead.trim() && !d.body.trim();
    if (!empty) {
      const ok = await confirm({ title: '발매 정보로 초안을 다시 채울까요?', message: '지금 쓴 제목·첫 문단·본문이 초안으로 바뀌어요.', confirmLabel: '다시 채우기' });
      if (!ok) { setD(x => ({ ...x, release_id: releaseId })); return; }
    }
    const draft = draftFor({
      artist, title: r.title, type: r.type, releaseDate: r.release_date, released: r.released, profile,
      link: r.link?.visible ? releaseUrl(r.link.slug) : undefined, contact,
    });
    setD(x => ({ ...x, release_id: releaseId, ...draft }));
  };

  const save = async (publish: boolean) => {
    if (!d.title.trim()) { toast('제목을 적어 주세요.', 'error'); document.getElementById('prTitle')?.focus(); return; }
    if (publish && (d.body.includes(GUIDE_MARK) || d.lead.includes(GUIDE_MARK))) {
      const ok = await confirm({ title: '안내 문장이 남아 있어요', message: `‘${GUIDE_MARK} …)’로 시작하는 안내 문장을 내용으로 바꾼 뒤 공개해 주세요. 그대로 공개할까요?`, confirmLabel: '그대로 공개' });
      if (!ok) return;
    }
    setBusy(true);
    try {
      const saved = await saveRelease(artistId, item?.id ?? null, { ...d, published: publish }, rel?.title ?? null);
      onSaved(saved);
      toast(publish ? '보도자료를 공개했어요. 보도자료 페이지에 보여요.' : '초안으로 저장했어요. 나만 볼 수 있어요.', 'success');
      close();
    } catch (err) { toast(errorMessage(err), 'error'); } finally { setBusy(false); }
  };
  const remove = async () => {
    if (!item) return;
    const ok = await confirm({ title: '이 보도자료를 지울까요?', message: '되돌릴 수 없어요. 페이지에서만 내리려면 ‘비공개로 저장’을 써 주세요.', confirmLabel: '지우기', danger: true });
    if (!ok) return;
    try { await deleteRelease(artistId, item.id); onDeleted(item.id); toast('보도자료를 지웠어요.', 'success'); close(); }
    catch (err) { toast(errorMessage(err), 'error'); }
  };

  return (
    <div className="aq-set-editor aq-press-editor">
      <div className="ui-field">
        <label htmlFor="prRelease">어떤 발매 소식인가요 <span className="aq-optional">선택</span></label>
        <select id="prRelease" className="ui-select" value={d.release_id ?? ''} onChange={e => void fill(e.target.value || null)}>
          <option value="">발매와 상관없는 소식</option>
          {releases.map(r => <option key={r.release_id} value={r.release_id}>{r.title} · {koreanDate(r.release_date)}</option>)}
        </select>
        <small className="ui-field-help">고르면 제목·첫 문단·발매 정보가 채워져요. 곡 소개처럼 아티스트만 쓸 수 있는 부분은 안내 문장으로 남겨 둬요.</small>
      </div>
      <div className="ui-field">
        <div className="aq-al-section-head"><label htmlFor="prTitle">제목</label><Count n={d.title.length} max={LIMITS.title} /></div>
        <input id="prTitle" className="ui-input" maxLength={LIMITS.title + 10} value={d.title} onChange={e => setD(x => ({ ...x, title: e.target.value }))} />
      </div>
      <div className="ui-field">
        <div className="aq-al-section-head"><label htmlFor="prLead">첫 문단</label><Count n={d.lead.length} max={LIMITS.lead} /></div>
        <textarea id="prLead" className="ui-textarea" rows={3} maxLength={LIMITS.lead + 20} placeholder="누가, 언제, 무엇을 — 한두 문장으로"
          value={d.lead} onChange={e => setD(x => ({ ...x, lead: e.target.value }))} />
      </div>
      <div className="ui-field">
        <div className="aq-al-section-head"><label htmlFor="prBody">본문</label><Count n={d.body.length} max={LIMITS.body} /></div>
        <textarea id="prBody" className="ui-textarea aq-press-body" rows={12} maxLength={LIMITS.body + 100}
          value={d.body} onChange={e => setD(x => ({ ...x, body: e.target.value }))} />
      </div>
      {d.title && <CopyButton text={fullText(d)} label="전문 복사" icon="doc" className="ui-btn is-sm aq-press-copy" />}
      <div className="aq-press-editor-actions">
        <button type="button" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy} onClick={() => void save(true)}>
          {item?.published ? '고친 내용 공개' : '공개하기'}
        </button>
        <button type="button" className="ui-btn is-block" disabled={busy} onClick={() => void save(false)}>
          {item?.published ? '비공개로 저장' : '초안으로 저장'}
        </button>
        {item && <button type="button" className="ui-btn is-text is-sm aq-danger-text" disabled={busy} onClick={() => void remove()}>지우기</button>}
      </div>
    </div>
  );
}
