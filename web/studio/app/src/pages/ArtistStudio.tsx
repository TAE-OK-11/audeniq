// 아티스트 페이지 (/artist) — 팬이 보는 모든 것을 한 화면에서: 소개 페이지·스마트링크·소식·팬 응원의 반응을 보고(반응),
// 고치고(관리), 기자·큐레이터용 보도자료를 쓰고(보도자료), 프로필 사진을 바꿔 플랫폼에도 보낸다(프로필 사진). 아티스트가 여럿이면 맨 위에서 고른다.
// 서버: crates/core/src/artist_studio.rs (반응·사진), artist_links.rs (소개 페이지), artist_community.rs (소식·응원), artist_pages.rs (플랫폼 페이지).
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Link, useSearchParams } from '../lib/router';
import { API_BASE, orgPath, req } from '../api/http';
import { api } from '../api/client';
import { errorMessage } from '../api/errors';
import { MOCK } from '../lib/mode';
import { num } from '../lib/format';
import { useProfile } from '../store/profile';
import { PLATFORM_NAMES } from '../lib/platformNames';
import { channelLabel } from '../lib/channels';
import { STATUS_LABEL, STATUS_TONE } from '../lib/platformRequests';
import { artistPath, artistUrl } from '../lib/publicLinks';
import {
  PERIODS, SELF_SERVE, SENDABLE, change, clickRate, fetchInsights, mockPhoto, photoUrl, removePhoto, sendPhoto, setPhoto,
  type Insights, type Period, type ReleaseInsight,
} from '../lib/artistStudio';
import { Badge, Callout, Cover, EmptyState, PageHeader, Segmented, SkeletonList, StatGrid, StatTile, Tabs } from '../ui';
import { Glyph } from '../components/Glyph';
import { Modal } from '../components/Modal';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { CopyButton } from '../components/CopyButton';
import { QrButton } from '../components/QrButton';
import { VisitsChart } from '../components/VisitsChart';
import { PhotoCropper, readImage, type CroppedPhoto } from '../components/PhotoCropper';
import { LinkEditor, type ArtistLinkRow } from '../components/ArtistLinkCard';
import { PagesEditor, pagesSummary, type ArtistRow } from '../components/ArtistPagesCard';
import { CheersManager, PostsManager } from '../components/ArtistCommunityManager';
import { PressKitManager } from '../components/PressKitManager';
import { gradientFor } from '../lib/catalog';

type Tab = 'insights' | 'manage' | 'press' | 'photo';
const TABS: { value: Tab; label: string }[] = [
  { value: 'insights', label: '반응' },
  { value: 'manage', label: '관리' },
  { value: 'press', label: '보도자료' },
  { value: 'photo', label: '프로필 사진' },
];

type StudioArtist = ArtistLinkRow & { photo?: boolean };

/** 이전 기간과 비교한 글자 — 늘면 '+12%', 줄면 '−8%', 비교할 수 없으면 '이전 기간 기록 없음' */
function delta(now: number, before: number): string {
  const c = change(now, before);
  if (c == null) return now ? '이전 기간 기록 없음' : '아직 기록이 없어요';
  if (c === 0) return '이전 기간과 같아요';
  return `이전 기간보다 ${c > 0 ? '+' : '-'}${Math.abs(c)}%`;
}

// ---------------------------------------------------------------------------
// 화면
// ---------------------------------------------------------------------------
export function ArtistStudio() {
  const profile = useProfile();
  const [params, setParams] = useSearchParams();
  const [artists, setArtists] = useState<StudioArtist[] | null>(null);
  const [pages, setPages] = useState<ArtistRow[]>([]);
  const [error, setError] = useState('');
  const tab = (TABS.some(t => t.value === params.get('tab')) ? params.get('tab') : 'insights') as Tab;
  const chosen = params.get('artist');
  const setParam = (k: string, v: string) => {
    const next = new URLSearchParams(params);
    next.set(k, v);
    setParams(next, { replace: true });
  };

  const loadArtists = useCallback(async () => {
    if (MOCK) {
      setArtists(cur => cur ?? [{
        artist_id: 'mock', name: profile.name.trim() || '서린', releases: 3, photo: false,
        link: { slug: 'mock', enabled: true, bio: '도시의 하루를 음악으로 기록해요.', views: 0, links: [], contact: '', cheers_enabled: true, posts: 3, cheers: 18 },
      }]);
      return;
    }
    try { setArtists((await req<{ items: StudioArtist[] }>(orgPath('/artist-links'), { quiet401: true })).items); }
    catch (e) { setError(errorMessage(e)); setArtists([]); }
  }, [profile.name]);
  const loadPages = useCallback(async () => {
    if (MOCK) return;
    try { setPages((await req<{ items: ArtistRow[] }>(orgPath('/artist-pages/all'), { quiet401: true })).items); }
    catch { /* 볼 수 없으면 그 줄만 뺀다 */ }
  }, []);
  useEffect(() => { void loadArtists(); void loadPages(); }, [loadArtists, loadPages]);

  const artist = artists?.find(a => a.artist_id === chosen) ?? artists?.[0] ?? null;
  const patchArtist = (patch: Partial<StudioArtist>) =>
    setArtists(list => list?.map(a => (a.artist_id === artist?.artist_id ? { ...a, ...patch } : a)) ?? list);

  return (
    <div className="view aq-studio" id="view-artist">
      <PageHeader
        eyebrow="내 작업실" title="아티스트 페이지"
        sub="소개 페이지·스마트링크·소식·팬 응원의 반응을 보고 한곳에서 고쳐요."
        actions={artists && artists.length > 1 ? (
          <select className="ui-select aq-studio-pick" aria-label="아티스트" value={artist?.artist_id ?? ''} onChange={e => setParam('artist', e.target.value)}>
            {artists.map(a => <option key={a.artist_id} value={a.artist_id}>{a.name}</option>)}
          </select>
        ) : undefined}
      />
      {error && <Callout tone="danger" role="alert">{error}</Callout>}
      {artists === null ? <SkeletonList rows={4} /> : !artist ? (
        <EmptyState icon={<Glyph name="music" size={22} />} title="아직 아티스트가 없어요"
          actions={<Link to="/upload" className="ui-btn is-primary">첫 발매 등록하기</Link>}>
          발매를 신청하면 그 아티스트의 페이지를 여기서 꾸밀 수 있어요.
        </EmptyState>
      ) : (
        <ArtistBody
          key={artist.artist_id} artist={artist} page={pages.find(p => p.artist_id === artist.artist_id) ?? null}
          tab={tab} onTab={t => setParam('tab', t)}
          onLinkSaved={row => patchArtist(row)} onPagesSaved={() => void loadPages()} onPhoto={v => patchArtist({ photo: v })}
          onCount={patch => artist.link && patchArtist({ link: { ...artist.link, ...patch } })}
        />
      )}
    </div>
  );
}

function ArtistBody({ artist, page, tab, onTab, onLinkSaved, onPagesSaved, onPhoto, onCount }: {
  artist: StudioArtist; page: ArtistRow | null; tab: Tab; onTab: (t: Tab) => void;
  onLinkSaved: (row: ArtistLinkRow) => void; onPagesSaved: () => void; onPhoto: (v: boolean) => void;
  onCount: (patch: Partial<NonNullable<ArtistLinkRow['link']>>) => void;
}) {
  const toast = useToast();
  const [days, setDays] = useState<Period>(30);
  const [data, setData] = useState<Insights | null>(null);
  const [loadError, setLoadError] = useState('');
  const [photoVersion, setPhotoVersion] = useState(() => Date.now());
  const [editLink, setEditLink] = useState(false);
  const link = artist.link;
  const pageInfo = link ? { slug: link.slug, enabled: link.enabled, cheers_enabled: link.cheers_enabled !== false } : null;

  const load = useCallback(async (d: Period) => {
    setLoadError('');
    try { setData(await fetchInsights(artist.artist_id, artist.name, d, pageInfo)); }
    catch (e) { setLoadError(errorMessage(e)); }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [artist.artist_id, artist.name, link?.slug, link?.enabled]);
  useEffect(() => { void load(days); }, [load, days]);

  const photoSrc = MOCK ? mockPhoto() : (artist.photo || data?.photo) ? photoUrl(artist.artist_id, photoVersion) : null;

  return (
    <>
      {/* 팬이 보게 될 페이지 한 장 — 사진·이름·주소·공유 */}
      <section className="ui-card aq-studio-id" aria-label={`${artist.name} 페이지`}>
        <button type="button" className="aq-studio-avatar" onClick={() => onTab('photo')} aria-label="프로필 사진 바꾸기">
          {photoSrc
            ? <img src={photoSrc} alt="" />
            : <span style={{ background: gradientFor(artist.artist_id) }}>{artist.name.trim().charAt(0)}</span>}
          <i aria-hidden="true"><Glyph name="pencil" size={13} /></i>
        </button>
        <div className="aq-studio-who min-0">
          <h2>{artist.name}</h2>
          {link ? (
            <p className="aq-studio-url">
              <Badge tone={link.enabled ? 'green' : 'gray'}>{link.enabled ? '공개 중' : '꺼짐'}</Badge>
              <span className="min-0">{artistUrl(link.slug).replace(/^https?:\/\//, '')}</span>
            </p>
          ) : <p className="aq-studio-url">소개 페이지를 만들면 팬에게 보여 줄 주소가 생겨요.</p>}
        </div>
        <div className="aq-promo-tools aq-studio-tools" role="group" aria-label="공유">
          {link ? (
            <>
              <CopyButton text={artistUrl(link.slug)} label="주소 복사" icon="link" className="aq-promo-tool" />
              <QrButton url={artistUrl(link.slug)} title={`${artist.name} 소개 페이지 QR 코드`} filename={`artist-${link.slug}.png`} className="aq-promo-tool" />
              <a className="aq-promo-tool" href={artistPath(link.slug)} target="_blank" rel="noopener noreferrer"><Glyph name="arrow-up-right" size={14} />열기</a>
            </>
          ) : (
            <button type="button" className="ui-btn is-primary is-sm" onClick={() => setEditLink(true)}><Glyph name="plus" size={14} className="aq-glyph-lead" />소개 페이지 만들기</button>
          )}
        </div>
      </section>

      <Tabs value={tab} items={TABS.map(t => ({ ...t, id: `studioTab-${t.value}`, controls: 'studioPanel' }))} onChange={onTab} label="아티스트 페이지" className="is-fill aq-studio-tabs" />
      <div id="studioPanel" role="tabpanel" aria-labelledby={`studioTab-${tab}`} className="ui-tab-panel" key={tab}>
        {tab === 'insights' && (
          loadError ? <Callout tone="danger" role="alert" action={<button type="button" className="ui-btn is-sm" onClick={() => void load(days)}>다시 불러오기</button>}>{loadError}</Callout>
            : <InsightsPanel data={data?.days === days ? data : null} days={days} onDays={setDays} hasPage={!!link} onCreate={() => setEditLink(true)} />
        )}
        {tab === 'manage' && (
          <ManagePanel artist={artist} page={page} data={data} onEditLink={() => setEditLink(true)} onPagesSaved={onPagesSaved} onCount={onCount}
            onCheersEnabled={async v => {
              if (!link) return;
              try {
                if (!MOCK) {
                  await req(orgPath(`/artists/${encodeURIComponent(artist.artist_id)}/link`), {
                    method: 'PUT', body: { enabled: link.enabled, bio: link.bio, links: link.links ?? [], contact: link.contact ?? '', cheers: v },
                  });
                }
                onCount({ cheers_enabled: v });
              } catch (e) { toast(errorMessage(e), 'error'); }
            }} />
        )}
        {tab === 'press' && (
          <PressKitManager artistId={artist.artist_id} artist={artist.name} slug={link?.slug ?? null} contact={link?.contact ?? ''}
            releases={data?.releases ?? []} onCreatePage={() => setEditLink(true)} />
        )}
        {tab === 'photo' && (
          <PhotoPanel artist={artist} data={data} photoSrc={photoSrc}
            onChanged={has => { onPhoto(has); setPhotoVersion(Date.now()); void load(days); }}
            onSent={() => void load(days)} />
        )}
      </div>

      {editLink && (
        <Modal title={`${artist.name} 소개 페이지`} onClose={() => setEditLink(false)} modalClass="aq-set-modal">
          <LinkEditor artist={artist} onSaved={row => { onLinkSaved(row); void load(days); }} />
        </Modal>
      )}
    </>
  );
}

// ---------------------------------------------------------------------------
// 반응
// ---------------------------------------------------------------------------
function InsightsPanel({ data, days, onDays, hasPage, onCreate }: {
  data: Insights | null; days: Period; onDays: (d: Period) => void; hasPage: boolean; onCreate: () => void;
}) {
  const periodPick = (
    <Segmented size="sm" label="기간" value={String(days) as `${Period}`} onChange={v => onDays(Number(v) as Period)}
      options={PERIODS.map(p => ({ value: String(p) as `${Period}`, label: `${p}일` }))} />
  );
  if (!data) return <><div className="aq-studio-bar">{periodPick}</div><SkeletonList rows={4} /></>;
  const t = data.totals;
  const p = data.previous;
  const rate = clickRate(t);
  const rateBefore = clickRate(p);
  const empty = !t.page_views && !t.link_views && !t.clicks;
  const platforms = Object.entries(data.platforms).filter(([, n]) => n > 0).sort((a, b) => b[1] - a[1]);
  const sources = Object.entries(data.sources).filter(([, n]) => n > 0).sort((a, b) => b[1] - a[1]);

  return (
    <div className="aq-studio-insights">
      <div className="aq-studio-bar">
        <p className="aq-studio-note">최근 {days}일 · 하루는 한국 시간 기준</p>
        {periodPick}
      </div>
      {!hasPage && (
        <Callout action={<button type="button" className="ui-btn is-sm" onClick={onCreate}>만들기</button>}>
          소개 페이지가 아직 없어요. 만들면 발매를 한 주소로 모아 SNS에 걸 수 있어요.
        </Callout>
      )}
      <StatGrid cols={4} className="aq-studio-stats">
        <StatTile label="소개 페이지 방문" value={num(t.page_views)} unit="회" hint={delta(t.page_views, p.page_views)} />
        <StatTile label="스마트링크 방문" value={num(t.link_views)} unit="회" hint={delta(t.link_views, p.link_views)} />
        <StatTile label="플랫폼으로 이동" value={num(t.clicks)} unit="회" hint={delta(t.clicks, p.clicks)} />
        <StatTile label="이동률" value={rate == null ? '–' : num(rate)} unit={rate == null ? undefined : '%'}
          hint={rate == null ? '스마트링크 방문이 생기면 보여요' : rateBefore == null ? '방문 중 플랫폼으로 간 비율' : `이전 기간 ${rateBefore}%`} />
      </StatGrid>

      <section className="ui-card aq-studio-block" aria-labelledby="studioDaily">
        <h3 id="studioDaily">날마다 방문</h3>
        {empty
          ? <p className="aq-studio-empty">이 기간에는 아직 방문이 없어요. 주소를 SNS·프로필에 걸면 여기에 쌓여요.</p>
          : <VisitsChart series={data.series} />}
      </section>

      <div className="aq-studio-two">
        <section className="ui-card aq-studio-block" aria-labelledby="studioPlatforms">
          <h3 id="studioPlatforms">어느 플랫폼으로 갔나요</h3>
          {platforms.length ? <ShareBars rows={platforms.map(([k, n]) => [PLATFORM_NAMES[k] ?? k, n])} unit="회" />
            : <p className="aq-studio-empty">스마트링크에서 플랫폼 버튼을 누르면 보여요.</p>}
        </section>
        <section className="ui-card aq-studio-block" aria-labelledby="studioSources">
          <h3 id="studioSources">어디서 들어왔나요</h3>
          {sources.length ? <ShareBars rows={sources.map(([k, n]) => [channelLabel(k), n])} unit="회" />
            : <p className="aq-studio-empty">채널별 링크(인스타그램·유튜브 등)로 공유하면 들어온 곳이 보여요. 발매 상세 &gt; 관리 &gt; 스마트링크에서 채널 링크를 받을 수 있어요.</p>}
        </section>
      </div>

      <section className="ui-card aq-studio-block" aria-labelledby="studioReleases">
        <h3 id="studioReleases">발매별</h3>
        {data.releases.length ? (
          <ul className="ui-list aq-studio-releases">
            {data.releases.map(r => <ReleaseRow key={r.release_id} r={r} />)}
          </ul>
        ) : <p className="aq-studio-empty">신청한 발매가 생기면 발매마다 방문과 이동이 보여요.</p>}
      </section>

      <section className="ui-card aq-studio-block" aria-labelledby="studioFans">
        <h3 id="studioFans">팬 반응</h3>
        <StatGrid cols={3} className="aq-studio-fans">
          <StatTile compact label="팬 응원" value={num(data.community.cheers)} unit="개" hint={data.community.cheers_new ? `최근 ${days}일 +${num(data.community.cheers_new)}` : `최근 ${days}일 새 응원 없음`} />
          <StatTile compact label="하트 남긴 응원" value={num(data.community.hearts)} unit="개" />
          <StatTile compact label="올린 소식" value={num(data.community.posts)} unit="개" />
        </StatGrid>
      </section>
    </div>
  );
}

/** 몇 개 항목의 비율 — 이름 · 막대 · 숫자 (한 가지 색, 많은 순) */
function ShareBars({ rows, unit }: { rows: [string, number][]; unit: string }) {
  const total = rows.reduce((n, [, v]) => n + v, 0) || 1;
  const max = Math.max(...rows.map(([, v]) => v), 1);
  const shown = rows.slice(0, 6);
  const rest = rows.slice(6).reduce((n, [, v]) => n + v, 0);
  const list: [string, number][] = rest ? [...shown, ['그 밖', rest]] : shown;
  return (
    <ol className="aq-share">
      {list.map(([label, v]) => (
        <li key={label}>
          <span className="aq-share-label">{label}</span>
          <span className="aq-share-track" aria-hidden="true"><i style={{ width: `${Math.max(2, (v / max) * 100)}%` }} /></span>
          <span className="aq-share-value">{num(v)}{unit} <small>{Math.round((v / total) * 100)}%</small></span>
        </li>
      ))}
    </ol>
  );
}

function ReleaseRow({ r }: { r: ReleaseInsight }) {
  const cover = r.cover && r.link?.visible && !MOCK ? `${API_BASE}/api/smart-links/${encodeURIComponent(r.link.slug)}/cover` : null;
  const status = !r.link ? { tone: 'gray' as const, text: '스마트링크 없음' }
    : !r.link.enabled ? { tone: 'gray' as const, text: '꺼짐' }
      : !r.link.visible ? { tone: 'amber' as const, text: '지금은 안 보여요' }
        : !r.released ? { tone: 'blue' as const, text: '발매 전' } : null;
  const extra = [
    r.alerts ? `발매 알림 ${num(r.alerts)}명` : '',
    r.presaves ? `프리세이브 ${num(r.presaves)}명` : '',
  ].filter(Boolean).join(' · ');
  return (
    <li>
      <Link to={`/releases/${encodeURIComponent(r.release_id)}?tab=manage`} className="ui-row aq-studio-release">
        <Cover id={r.release_id} text={r.title} src={cover} />
        <span className="min-0 aq-studio-release-main">
          <strong>{r.title}</strong>
          <small>{r.link ? `방문 ${num(r.views)} · 이동 ${num(r.clicks)}` : '스마트링크를 만들면 반응이 쌓여요'}{extra && ` · ${extra}`}</small>
        </span>
        {status && <Badge tone={status.tone}>{status.text}</Badge>}
        <Glyph name="chevron-right" size={16} className="aq-set-chev" />
      </Link>
    </li>
  );
}

// ---------------------------------------------------------------------------
// 관리
// ---------------------------------------------------------------------------
function ManagePanel({ artist, page, data, onEditLink, onPagesSaved, onCount, onCheersEnabled }: {
  artist: StudioArtist; page: ArtistRow | null; data: Insights | null;
  onEditLink: () => void; onPagesSaved: () => void;
  onCount: (patch: Partial<NonNullable<ArtistLinkRow['link']>>) => void; onCheersEnabled: (v: boolean) => Promise<void>;
}) {
  const [open, setOpen] = useState<'pages' | 'news' | 'cheers' | null>(null);
  const link = artist.link;
  const ps = page ? pagesSummary(page) : null;
  const releases = data?.releases ?? [];
  const withLink = releases.filter(r => r.link);

  return (
    <div className="aq-studio-manage">
      <section className="aq-set-card" aria-labelledby="studioPage">
        <h2 id="studioPage">소개 페이지</h2>
        <SetRow label="이름·소개·링크" value={link ? (link.bio ? link.bio.split('\n')[0] : '소개 글 쓰기') : '만들기'} empty={!link || !link.bio} onClick={onEditLink} />
        <SetRow label="소식" value={link ? (link.posts ? `${num(link.posts)}개` : '공지·이벤트 올리기') : '소개 페이지를 먼저 만들어요'} empty={!link?.posts} onClick={link ? () => setOpen('news') : onEditLink} />
        <SetRow label="팬 응원" value={!link ? '소개 페이지를 먼저 만들어요' : link.cheers_enabled === false ? '꺼짐' : link.cheers ? `${num(link.cheers)}개` : '아직 없어요'}
          empty={!link?.cheers} onClick={link ? () => setOpen('cheers') : onEditLink} />
        {ps && <SetRow label="플랫폼 페이지" value={ps.text} empty={ps.empty} onClick={() => setOpen('pages')} />}
      </section>

      <section className="aq-set-card" aria-labelledby="studioLinks">
        <h2 id="studioLinks">스마트링크 <small>{withLink.length} / {releases.length}</small></h2>
        <p className="ui-field-help aq-set-help">발매마다 하나씩, 플랫폼 링크를 한 주소로 모아요. 누르면 그 발매의 스마트링크 관리로 가요.</p>
        {data === null ? <SkeletonList rows={2} /> : releases.length ? releases.map(r => (
          <Link key={r.release_id} to={`/releases/${encodeURIComponent(r.release_id)}?tab=manage`} className="aq-set-row">
            <span className="aq-set-label">{r.title}</span>
            <span className={`aq-set-value${r.link ? '' : ' is-empty'}`}>
              {!r.link ? '만들기' : !r.link.enabled ? '꺼짐' : !r.link.visible ? '지금은 안 보여요' : r.released ? '공개 중' : '발매 전 공개 중'}
            </span>
            <Glyph name="chevron-right" size={16} className="aq-set-chev" />
          </Link>
        )) : <p className="aq-studio-empty">신청한 발매가 생기면 여기서 스마트링크를 만들 수 있어요.</p>}
      </section>

      {open === 'pages' && page && (
        <Modal title={`${artist.name} 플랫폼 페이지`} onClose={() => setOpen(null)} modalClass="aq-set-modal">
          <PagesEditor artist={page} onSaved={onPagesSaved} />
        </Modal>
      )}
      {open === 'news' && link && (
        <Modal title={`${artist.name} 소식`} onClose={() => setOpen(null)} modalClass="aq-set-modal">
          <PostsManager artistId={artist.artist_id} artist={artist.name} slug={link.slug} onCount={n => onCount({ posts: n })} />
        </Modal>
      )}
      {open === 'cheers' && link && (
        <Modal title={`${artist.name} 팬 응원`} onClose={() => setOpen(null)} modalClass="aq-set-modal">
          <CheersManager artistId={artist.artist_id} artist={artist.name} enabled={link.cheers_enabled !== false}
            onEnabled={onCheersEnabled} onCount={n => onCount({ cheers: n })} />
        </Modal>
      )}
    </div>
  );
}

function SetRow({ label, value, empty, onClick }: { label: string; value: string; empty?: boolean; onClick: () => void }) {
  return (
    <button type="button" className="aq-set-row" onClick={onClick}>
      <span className="aq-set-label">{label}</span>
      <span className={`aq-set-value${empty ? ' is-empty' : ''}`}>{value}</span>
      <Glyph name="chevron-right" size={16} className="aq-set-chev" />
    </button>
  );
}

// ---------------------------------------------------------------------------
// 프로필 사진
// ---------------------------------------------------------------------------
function PhotoPanel({ artist, data, photoSrc, onChanged, onSent }: {
  artist: StudioArtist; data: Insights | null; photoSrc: string | null;
  onChanged: (has: boolean) => void; onSent: () => void;
}) {
  const toast = useToast();
  const confirm = useConfirm();
  const input = useRef<HTMLInputElement>(null);
  const [image, setImage] = useState<HTMLImageElement | null>(null);
  const [pick, setPick] = useState<string[]>([]);
  const [sending, setSending] = useState(false);
  const requests = useMemo(() => new Map((data?.photo_requests ?? []).map(r => [r.platform, r])), [data]);
  const isOpen = (p: string) => ['SUBMITTED', 'IN_PROGRESS', 'SENT_TO_PLATFORM'].includes(requests.get(p)?.status ?? '');
  // 처음엔 아직 지금 사진으로 바꾸지 않은 곳을 모두 골라 둔다
  useEffect(() => {
    setPick(SENDABLE.filter(p => !isOpen(p) && !(requests.get(p)?.status === 'RESOLVED' && requests.get(p)?.current)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [requests]);
  const has = !!photoSrc;

  const choose = async (file: File | undefined) => {
    if (!file) return;
    if (!['image/jpeg', 'image/png'].includes(file.type)) { toast('JPG나 PNG 사진을 골라 주세요.', 'error'); return; }
    if (file.size > 20 * 1024 * 1024) { toast('20MB 이하 사진을 골라 주세요.', 'error'); return; }
    try {
      const img = await readImage(file);
      if (Math.min(img.naturalWidth, img.naturalHeight) < 1000) {
        toast(`사진이 ${img.naturalWidth}×${img.naturalHeight}px이에요. 짧은 쪽이 1000px 이상인 사진을 골라 주세요.`, 'error');
        return;
      }
      setImage(img);
    } catch { toast('사진을 열 수 없어요. 다른 사진을 골라 주세요.', 'error'); }
  };
  const save = async (photo: CroppedPhoto) => {
    try {
      const up = await api.uploadFile(photo.file, 'IMAGE');
      await setPhoto(artist.artist_id, up.assetId, photo.preview);
      onChanged(true);
      toast('프로필 사진을 바꿨어요. 소개 페이지에 바로 보여요.', 'success');
    } catch (e) {
      // 막 올린 사진은 안전 검사가 끝나야 쓸 수 있다
      toast(errorMessage(e), 'error');
      throw e;
    }
  };
  const remove = async () => {
    const ok = await confirm({ title: '프로필 사진을 지울까요?', message: '소개 페이지에는 이름 첫 글자가 보여요. 플랫폼에 이미 보낸 사진은 그대로예요.', confirmLabel: '지우기', danger: true });
    if (!ok) return;
    try { await removePhoto(artist.artist_id); onChanged(false); toast('프로필 사진을 지웠어요.', 'success'); }
    catch (e) { toast(errorMessage(e), 'error'); }
  };
  const send = async () => {
    if (!pick.length) return;
    setSending(true);
    try {
      const r = await sendPhoto(artist.artist_id, pick);
      const names = (list: string[]) => list.map(p => PLATFORM_NAMES[p] ?? p).join('·');
      if (r.created.length) toast(`${names(r.created.map(c => c.platform))}에 사진 바꾸기를 요청했어요. 담당자가 플랫폼에 보내고 결과를 알려 드려요.`, 'success');
      else toast(`${names(r.skipped)}은 이미 요청이 진행 중이에요.`, 'info');
      onSent();
    } catch (e) { toast(errorMessage(e), 'error'); } finally { setSending(false); }
  };
  const ready = pick.filter(p => !isOpen(p)).length;
  const sendLabel = ready ? `${ready}곳에 사진 보내기` : SENDABLE.every(isOpen) ? '모든 곳에 요청이 진행 중이에요' : '보낼 곳을 골라 주세요';
  const download = () => {
    if (!photoSrc) return;
    const a = document.createElement('a');
    a.href = photoSrc;
    a.download = `${artist.name.trim() || 'artist'}-profile.jpg`;
    document.body.appendChild(a);
    a.click();
    a.remove();
  };

  return (
    <div className="aq-studio-photo">
      <section className="ui-card aq-studio-block aq-photo-now" aria-labelledby="photoNow">
        <div className="aq-photo-preview" aria-hidden="true">
          {photoSrc ? <img src={photoSrc} alt="" /> : <span style={{ background: gradientFor(artist.artist_id) }}>{artist.name.trim().charAt(0)}</span>}
        </div>
        <div className="min-0">
          <h3 id="photoNow">{has ? '지금 프로필 사진' : '프로필 사진이 아직 없어요'}</h3>
          <p className="aq-studio-note">정사각형으로 잘라 써요. 짧은 쪽이 1000px 이상인 JPG·PNG, 얼굴이 가운데 오면 플랫폼마다 동그랗게 잘려도 잘 보여요.</p>
          <div className="aq-photo-actions">
            <button type="button" className="ui-btn is-primary is-sm" onClick={() => input.current?.click()}>
              <Glyph name="image" size={14} className="aq-glyph-lead" />{has ? '사진 바꾸기' : '사진 올리기'}
            </button>
            {has && <button type="button" className="ui-btn is-sm" onClick={download}><Glyph name="download" size={14} className="aq-glyph-lead" />사진 받기</button>}
            {has && <button type="button" className="ui-btn is-text is-sm" onClick={() => void remove()}>지우기</button>}
          </div>
          <input ref={input} type="file" accept="image/jpeg,image/png" hidden onChange={e => { void choose(e.target.files?.[0]); e.target.value = ''; }} />
        </div>
      </section>

      <section className="aq-set-card aq-photo-send" aria-labelledby="photoSend">
        <h2 id="photoSend">AUDENIQ이 바꿔 드려요</h2>
        <p className="ui-field-help aq-set-help">유통사가 사진을 보낼 수 있는 곳이에요. 담당자가 플랫폼에 보내고, 반영되면 알림으로 알려 드려요. 반영까지는 플랫폼마다 며칠에서 몇 주 걸려요.</p>
        <ul className="aq-photo-platforms">
          {SENDABLE.map(p => {
            const r = requests.get(p);
            const busy = isOpen(p);
            const old = r && !r.current;
            return (
              <li key={p}>
                <label className={`ui-check aq-photo-platform${busy || !has ? ' is-disabled' : ''}`}>
                  <input type="checkbox" checked={has && !busy && pick.includes(p)} disabled={busy || !has}
                    onChange={e => setPick(list => (e.target.checked ? [...list, p] : list.filter(x => x !== p)))} />
                  <span className="aq-photo-platform-name">{PLATFORM_NAMES[p] ?? p}</span>
                  {r && <Badge tone={STATUS_TONE[r.status] ?? 'gray'}>{old && r.status === 'RESOLVED' ? '이전 사진으로 반영' : STATUS_LABEL[r.status] ?? r.status}</Badge>}
                </label>
              </li>
            );
          })}
        </ul>
        {!has && <p className="aq-studio-note">사진을 먼저 올리면 보낼 수 있어요.</p>}
        <button type="button" className={`ui-btn is-primary is-block${sending ? ' is-busy' : ''}`} disabled={!has || !ready || sending} onClick={() => void send()}>
          {sending ? '요청하는 중' : sendLabel}
        </button>
        <Link to="/inquiries" className="ui-btn is-text is-sm aq-photo-history">요청 내역 보기</Link>
      </section>

      <section className="aq-set-card" aria-labelledby="photoSelf">
        <h2 id="photoSelf">직접 바꿔야 하는 곳</h2>
        <p className="ui-field-help aq-set-help">이 플랫폼들은 유통사가 사진을 바꿀 수 없고, 아티스트 계정으로만 바꿀 수 있어요. 위 ‘사진 받기’로 같은 사진을 받아 올려 주세요.</p>
        {SELF_SERVE.map(s => (
          <a key={s.key} className="aq-set-row" href={s.url} target="_blank" rel="noopener noreferrer">
            <span className="aq-set-label">{PLATFORM_NAMES[s.key] ?? s.key}</span>
            <span className="aq-set-value">{s.tool}</span>
            <Glyph name="arrow-up-right" size={16} className="aq-set-chev" />
          </a>
        ))}
      </section>

      {image && (
        <Modal title="사진 자르기" onClose={() => { URL.revokeObjectURL(image.src); setImage(null); }} modalClass="aq-set-modal">
          <PhotoCropper image={image} onDone={save} />
        </Modal>
      )}
    </div>
  );
}
