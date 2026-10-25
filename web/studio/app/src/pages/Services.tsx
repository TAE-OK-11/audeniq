// 부가서비스 (/services) — 뮤직비디오 배급·심의, 신속 발매, AI 가사 싱크를 신청하고 결제(정산금·카드)하고 진행을 본다.
// 무료로 쓸 수 있는 것(가사 등록·Content ID·아티스트 페이지 등)도 한 가격표에. 서버: crates/core/src/addons.rs
import { useCallback, useEffect, useMemo, useState } from 'react';
import { Link, useSearchParams } from '../lib/router';
import { api } from '../api/client';
import { errorMessage } from '../api/errors';
import { useAsync } from '../hooks/useAsync';
import { money, niceDate } from '../lib/format';
import { PLATFORM_NAMES } from '../lib/platformNames';
import {
  LYRIC_SERVICE_LABEL, PRICE_TABLE, RATING_LABEL, SERVICE_LABEL, STATUS_LABEL, STATUS_TONE, TIER,
  cancelOrder, confirmCard, createOrder, fetchAddons, fetchTracks, lrcLines, orderStage, payCard, paySettlement, putLrc, statusNote,
  type AddonOrder, type AddonState, type MvTier, type Service, type TrackRow,
} from '../lib/addons';
import type { Release } from '../api/types';
import { Badge, Callout, EmptyState, PageHeader, SkeletonList } from '../ui';
import { Glyph, type GlyphName } from '../components/Glyph';
import { Modal, useModalClose } from '../components/Modal';
import { StageCard } from '../components/StageCard';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { CheckIcon } from '../components/Check';

const ICON: Record<Service, GlyphName> = { MV_DISTRIBUTION: 'release', EXPRESS_RELEASE: 'clock', LYRIC_SYNC: 'music' };
type Form = 'mv' | 'lyrics' | 'lrc';

/** 신청한(작성 중이 아닌) 발매 — 부가서비스는 신청한 발매에만 */
const submitted = (rs: Release[]) => rs.filter(r => r.status !== 'draft');

export function Services() {
  const toast = useToast();
  const [params, setParams] = useSearchParams();
  const [state, setState] = useState<AddonState | null>(null);
  const [error, setError] = useState('');
  const { data: releases = [] } = useAsync(() => api.listReleases(), [], { key: 'releases' });
  const [form, setForm] = useState<Form | null>(null);
  const [openId, setOpenId] = useState<string | null>(null);
  const [paying, setPaying] = useState<AddonOrder | null>(null);

  const load = useCallback(async () => {
    try { setState(await fetchAddons()); setError(''); } catch (e) { setError(errorMessage(e)); }
  }, []);
  useEffect(() => { void load(); }, [load]);

  // 주소로 온 일: ?new=mv|lyrics|lrc (가격표·다른 화면), ?order=ID (알림), 카드 결제창에서 돌아옴(?pay=card&id=…)
  useEffect(() => {
    const n = params.get('new');
    const order = params.get('order');
    const id = params.get('id');
    if (!n && !order && params.get('pay') !== 'card') return;
    if (n === 'mv' || n === 'lyrics' || n === 'lrc') setForm(n);
    if (order) setOpenId(order);
    if (params.get('pay') === 'card' && id) {
      if (params.get('fail')) toast(params.get('message') || '결제를 마치지 못했어요.', 'error');
      else {
        void confirmCard(id, params).then(
          () => { toast('결제했어요. 담당자에게 접수됐어요.', 'success'); setOpenId(id); void load(); },
          e => toast(errorMessage(e), 'error'),
        );
      }
    }
    setParams({}, { replace: true });
  }, [params]); // eslint-disable-line react-hooks/exhaustive-deps

  const open = state?.items.find(o => o.id === openId) ?? null;
  const replace = (o: AddonOrder) => setState(s => (s ? { ...s, items: s.items.some(x => x.id === o.id) ? s.items.map(x => (x.id === o.id ? o : x)) : [o, ...s.items] } : s));
  const waiting = state?.items.filter(o => o.status === 'AWAITING_PAYMENT') ?? [];

  return (
    <div className="view" id="view-services">
      <PageHeader eyebrow="내 작업실" title="부가서비스"
        sub="뮤직비디오 배급, 신속 발매, 싱크 가사까지 — 필요한 것만 골라 정산금이나 카드로 결제해요." />
      {error && <Callout tone="danger" role="alert">{error}</Callout>}

      {waiting.length > 0 && (
        <Callout tone="warn" action={<button type="button" className="ui-btn is-sm" onClick={() => setPaying(waiting[0])}>결제하기</button>}>
          결제를 기다리는 신청이 {waiting.length}건 있어요. 결제하면 바로 담당자에게 접수돼요.
        </Callout>
      )}

      <section className="aq-svc-grid" aria-label="유료 부가서비스">
        <ServiceCard icon="release" name="뮤직비디오 배급·심의" price={state ? `${money(state.prices.mv[0].price)}부터` : '19,000원부터'}
          desc="멜론·지니·벅스·FLO·바이브·유튜브에 뮤직비디오를 보내요. 영상물등급위원회 심의 신청도 대신해요."
          action={<button type="button" className="ui-btn is-primary is-sm" onClick={() => setForm('mv')}>신청하기</button>} />
        <ServiceCard icon="clock" name="신속 발매" price={state ? money(state.prices.express_release) : '10,000원'}
          desc="심사와 플랫폼 전송을 먼저 처리해 신청 후 3영업일 안에 발매해요. 발매 신청서의 ‘신속 발매’로 신청해요."
          action={<Link className="ui-btn is-sm" to="/upload">발매 신청하기</Link>} />
        <ServiceCard icon="music" name="AI 가사 싱크" price={state ? `곡당 ${money(state.prices.lyric_sync)}` : '곡당 5,000원'}
          desc="가사에 노래 시간을 맞춘 싱크 가사(LRC)를 만들어 Musixmatch·국내 플랫폼에 등록해요."
          action={<button type="button" className="ui-btn is-primary is-sm" onClick={() => setForm('lyrics')}>신청하기</button>} />
      </section>

      <section className="aq-svc-mine" aria-labelledby="svcMine">
        <div className="aq-svc-head">
          <h2 id="svcMine">내 신청</h2>
          {state && <span className="aq-svc-balance">출금 가능한 정산금 <b>{money(state.balance)}</b></span>}
        </div>
        {!state ? <SkeletonList rows={2} /> : state.items.length ? (
          <div className="aq-catalog-cards">
            {state.items.map(o => (
              <button key={o.id} type="button" className="ui-row aq-ticket-card" onClick={() => setOpenId(o.id)}>
                <span className="aq-ticket-icon" aria-hidden="true"><Glyph name={ICON[o.service]} size={18} /></span>
                <span className="min-0">
                  <span className="ui-row-title">{o.title}</span>
                  <span className="ui-row-meta">{[SERVICE_LABEL[o.service], o.tier ? TIER[o.tier].label : '', money(o.amount), niceDate(o.created_at)].filter(Boolean).join(' · ')}</span>
                </span>
                <Badge tone={STATUS_TONE[o.status]}>{STATUS_LABEL[o.status]}</Badge>
              </button>
            ))}
          </div>
        ) : (
          <EmptyState icon={<Glyph name="layers" size={22} />} title="아직 신청한 부가서비스가 없어요" as="h3">
            신청하면 결제부터 완료까지 여기서 볼 수 있어요.
          </EmptyState>
        )}
      </section>

      <section className="aq-set-card aq-svc-prices" aria-labelledby="svcPrices">
        <h2 id="svcPrices">전체 요금</h2>
        <p className="ui-field-help aq-set-help">부가세 포함 가격이에요. 무료 서비스는 신청 없이 바로 써요.</p>
        {PRICE_TABLE.map(p => {
          const body = (
            <>
              <span className="min-0 aq-svc-price-name">
                <strong>{p.name}</strong>
                <small>{p.note}</small>
              </span>
              <span className={`aq-svc-price${p.tag === 'free' ? ' is-free' : p.tag === 'soon' ? ' is-soon' : ''}`}>{p.price}</span>
              {p.to && <Glyph name="chevron-right" size={16} className="aq-set-chev" />}
            </>
          );
          return p.to
            ? <Link key={p.name} to={p.to} className="aq-set-row aq-svc-price-row">{body}</Link>
            : <div key={p.name} className="aq-set-row aq-svc-price-row is-static">{body}</div>;
        })}
      </section>

      {form === 'mv' && state && (
        <Modal title="뮤직비디오 배급·심의" onClose={() => setForm(null)} dismissible={false}>
          <MvForm state={state} releases={submitted(releases)} onMade={o => { replace(o); setPaying(o); }} />
        </Modal>
      )}
      {form === 'lyrics' && state && (
        <Modal title="AI 가사 싱크" onClose={() => setForm(null)} dismissible={false}>
          <LyricForm price={state.prices.lyric_sync} releases={submitted(releases)} onMade={o => { replace(o); setPaying(o); }} />
        </Modal>
      )}
      {form === 'lrc' && (
        <Modal title="싱크 가사 올리기 (무료)" onClose={() => setForm(null)}>
          <LrcForm releases={submitted(releases)} />
        </Modal>
      )}
      {open && (
        <Modal title={open.title} onClose={() => setOpenId(null)}>
          <OrderDetail o={open} onPay={() => setPaying(open)} onChanged={o => { replace(o); void load(); }} />
        </Modal>
      )}
      {paying && state && (
        <Modal title="결제" onClose={() => setPaying(null)}>
          <PayForm o={paying} balance={state.balance} card={state.card}
            onPaid={o => { replace(o); setPaying(null); setOpenId(o.id); void load(); }} />
        </Modal>
      )}
    </div>
  );
}

function ServiceCard({ icon, name, price, desc, action }: { icon: GlyphName; name: string; price: string; desc: string; action: React.ReactNode }) {
  return (
    <article className="aq-svc-card">
      <span className="aq-svc-icon" aria-hidden="true"><Glyph name={icon} size={20} /></span>
      <h3>{name}</h3>
      <p className="aq-svc-card-price">{price}</p>
      <p className="aq-svc-card-desc">{desc}</p>
      <div className="aq-svc-card-action">{action}</div>
    </article>
  );
}

// ---------------------------------------------------------------------------
// 신청서
// ---------------------------------------------------------------------------
function ReleasePick({ releases, value, onChange }: { releases: Release[]; value: string; onChange: (id: string) => void }) {
  return (
    <div className="ui-field">
      <label htmlFor="svRelease">발매</label>
      <select id="svRelease" value={value} onChange={e => onChange(e.target.value)}>
        <option value="">발매 고르기</option>
        {releases.map(r => <option key={r.id} value={r.id}>{r.title || '제목 없는 발매'}</option>)}
      </select>
      {!releases.length && <small className="ui-field-help">신청한 발매가 있어야 신청할 수 있어요. <Link to="/upload">발매 신청하기</Link></small>}
    </div>
  );
}

function useTracks(releaseId: string) {
  const [tracks, setTracks] = useState<TrackRow[] | null>(null);
  useEffect(() => {
    if (!releaseId) { setTracks(null); return; }
    let live = true;
    setTracks(null);
    fetchTracks(releaseId).then(t => { if (live) setTracks(t); }, () => { if (live) setTracks([]); });
    return () => { live = false; };
  }, [releaseId]);
  return tracks;
}

function MvForm({ state, releases, onMade }: { state: AddonState; releases: Release[]; onMade: (o: AddonOrder) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [tier, setTier] = useState<MvTier>('REVIEW');
  const [releaseId, setReleaseId] = useState('');
  const tracks = useTracks(releaseId);
  const [trackId, setTrackId] = useState('');
  const [d, setD] = useState({ video_url: '', mv_title: '', duration: '', director: '', production: '', cast: '', synopsis: '', release_date: '', notes: '', rating: '', rating_no: '' });
  const [platforms, setPlatforms] = useState<string[]>(state.mv_platforms);
  const [busy, setBusy] = useState(false);
  const set = (k: keyof typeof d, v: string) => setD(x => ({ ...x, [k]: v }));
  const price = state.prices.mv.find(t => t.tier === tier)?.price ?? 0;
  useEffect(() => { setTrackId(tracks?.[0]?.id ?? ''); }, [tracks]);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    const fail = (m: string, id?: string) => { toast(m, 'error'); if (id) document.getElementById(id)?.focus(); };
    if (!releaseId || !trackId) return fail('발매와 곡을 골라 주세요.', 'svRelease');
    if (!/^https:\/\/\S+$/i.test(d.video_url.trim())) return fail('영상 파일 링크를 https 주소로 적어 주세요.', 'mvUrl');
    if (!d.mv_title.trim()) return fail('뮤직비디오 제목을 적어 주세요.', 'mvTitle');
    if (!/^\d{1,2}:[0-5]\d$/.test(d.duration.trim())) return fail('영상 길이를 3:45처럼 적어 주세요.', 'mvDuration');
    if (!platforms.length) return fail('보낼 플랫폼을 하나 이상 골라 주세요.');
    if (tier === 'BASIC' && (!d.rating || !d.rating_no.trim())) return fail('받은 등급과 등급분류번호를 적어 주세요.', 'mvRating');
    setBusy(true);
    try {
      const rel = releases.find(r => r.id === releaseId);
      const o = await createOrder({ service: 'MV_DISTRIBUTION', tier, release_id: releaseId, track_ids: [trackId], detail: { ...d, platforms } },
        rel?.title ?? '', tracks ?? []);
      close();
      onMade(o);
    } catch (err) { toast(errorMessage(err), 'error'); } finally { setBusy(false); }
  };

  return (
    <form className="aq-svc-form" onSubmit={submit} noValidate>
      <div className="ui-field">
        <span className="ui-field-label">어떻게 보낼까요</span>
        <div className="ui-choices" role="radiogroup" aria-label="뮤직비디오 상품">
          {state.prices.mv.map(t => (
            <button key={t.tier} type="button" role="radio" aria-checked={tier === t.tier} className="ui-choice aq-svc-tier" onClick={() => setTier(t.tier)}>
              <span>
                <b>{TIER[t.tier].label} · {money(t.price)}</b>
                <small>{TIER[t.tier].desc}</small>
              </span>
              <span className="ui-choice-mark" aria-hidden="true">{tier === t.tier && <CheckIcon size={11} />}</span>
            </button>
          ))}
        </div>
      </div>
      <ReleasePick releases={releases} value={releaseId} onChange={setReleaseId} />
      {releaseId && (
        <div className="ui-field">
          <label htmlFor="mvTrack">뮤직비디오 곡</label>
          <select id="mvTrack" value={trackId} onChange={e => setTrackId(e.target.value)} disabled={!tracks}>
            {(tracks ?? []).map(t => <option key={t.id} value={t.id}>{t.track_number}. {t.title}</option>)}
          </select>
        </div>
      )}
      <div className="ui-field">
        <label htmlFor="mvUrl">영상 파일 링크</label>
        <input id="mvUrl" type="url" inputMode="url" maxLength={500} placeholder="https:// (구글 드라이브·드롭박스 등 내려받을 수 있는 링크)"
          value={d.video_url} onChange={e => set('video_url', e.target.value)} />
        <small className="ui-field-help">원본 화질(1080p 이상, MP4·MOV)로 올려 주세요. 링크는 누구나 내려받을 수 있게 열어 두세요.</small>
      </div>
      <div className="form-grid">
        <div className="ui-field">
          <label htmlFor="mvTitle">뮤직비디오 제목</label>
          <input id="mvTitle" maxLength={200} placeholder="예: 여름의 끝 MV" value={d.mv_title} onChange={e => set('mv_title', e.target.value)} />
        </div>
        <div className="ui-field">
          <label htmlFor="mvDuration">영상 길이</label>
          <input id="mvDuration" inputMode="numeric" maxLength={5} placeholder="3:45" value={d.duration} onChange={e => set('duration', e.target.value)} />
        </div>
        <div className="ui-field">
          <label htmlFor="mvDirector">감독<span className="aq-optional">선택</span></label>
          <input id="mvDirector" maxLength={200} value={d.director} onChange={e => set('director', e.target.value)} />
        </div>
        <div className="ui-field">
          <label htmlFor="mvProduction">제작사<span className="aq-optional">선택</span></label>
          <input id="mvProduction" maxLength={200} value={d.production} onChange={e => set('production', e.target.value)} />
        </div>
      </div>
      {tier === 'BASIC' ? (
        <div className="form-grid">
          <div className="ui-field">
            <label htmlFor="mvRating">받은 등급</label>
            <select id="mvRating" value={d.rating} onChange={e => set('rating', e.target.value)}>
              <option value="">고르기</option>
              {Object.entries(RATING_LABEL).map(([k, v]) => <option key={k} value={k}>{v}</option>)}
            </select>
          </div>
          <div className="ui-field">
            <label htmlFor="mvRatingNo">등급분류번호</label>
            <input id="mvRatingNo" maxLength={60} placeholder="영상물등급위원회 결정서의 번호" value={d.rating_no} onChange={e => set('rating_no', e.target.value)} />
          </div>
        </div>
      ) : (
        <>
          <div className="ui-field">
            <label htmlFor="mvCast">출연자<span className="aq-optional">선택</span></label>
            <input id="mvCast" maxLength={500} placeholder="심의 신청서에 들어가요" value={d.cast} onChange={e => set('cast', e.target.value)} />
          </div>
          <div className="ui-field">
            <label htmlFor="mvSynopsis">영상 내용<span className="aq-optional">선택</span></label>
            <textarea id="mvSynopsis" rows={3} maxLength={2000} placeholder="어떤 장면이 나오는지 두세 문장으로 (심의에 도움이 돼요)" value={d.synopsis} onChange={e => set('synopsis', e.target.value)} />
          </div>
        </>
      )}
      <div className="ui-field">
        <span className="ui-field-label">보낼 플랫폼</span>
        <div className="aq-svc-platforms">
          {state.mv_platforms.map(p => (
            <label key={p} className="ui-check">
              <input type="checkbox" checked={platforms.includes(p)} onChange={e => setPlatforms(l => (e.target.checked ? [...l, p] : l.filter(x => x !== p)))} />
              <span>{PLATFORM_NAMES[p] ?? p}</span>
            </label>
          ))}
        </div>
      </div>
      <div className="ui-field">
        <label htmlFor="mvDate">공개 희망일<span className="aq-optional">선택</span></label>
        <input id="mvDate" type="date" value={d.release_date} onChange={e => set('release_date', e.target.value)} />
        <small className="ui-field-help">{tier === 'BASIC' ? '보통 영업일 기준 5일 뒤부터 공개할 수 있어요.' : tier === 'REVIEW' ? '심의에 보통 1~2주 걸려요. 등급이 나오면 바로 보내요.' : '심의와 배급을 가장 먼저 처리해요.'}</small>
      </div>
      <div className="aq-sticky-foot">
        <button type="submit" className={`ui-btn is-primary studio-submit-wide${busy ? ' is-busy' : ''}`} disabled={busy}>{money(price)} 결제하고 신청</button>
      </div>
    </form>
  );
}

function LyricForm({ price, releases, onMade }: { price: number; releases: Release[]; onMade: (o: AddonOrder) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [releaseId, setReleaseId] = useState('');
  const tracks = useTracks(releaseId);
  const [pick, setPick] = useState<string[]>([]);
  const [notes, setNotes] = useState('');
  const [busy, setBusy] = useState(false);
  // 가사가 있고 아직 싱크 가사가 없는 곡을 미리 골라 둔다
  useEffect(() => { setPick((tracks ?? []).filter(t => t.has_lyrics && !t.lrc).map(t => t.id)); }, [tracks]);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!releaseId || !pick.length) { toast('발매와 곡을 골라 주세요.', 'error'); return; }
    setBusy(true);
    try {
      const rel = releases.find(r => r.id === releaseId);
      const o = await createOrder({ service: 'LYRIC_SYNC', release_id: releaseId, track_ids: pick, detail: { notes } }, rel?.title ?? '', tracks ?? []);
      close();
      onMade(o);
    } catch (err) { toast(errorMessage(err), 'error'); } finally { setBusy(false); }
  };

  return (
    <form className="aq-svc-form" onSubmit={submit} noValidate>
      <p className="small muted">발매 신청 때 넣은 가사에 AI가 노래 시간을 맞추고, 담당자가 확인해 Musixmatch(Spotify·Apple Music·Instagram 가사)와 국내 플랫폼에 등록해요.</p>
      <ReleasePick releases={releases} value={releaseId} onChange={setReleaseId} />
      {releaseId && (
        <div className="ui-field">
          <span className="ui-field-label">곡</span>
          {!tracks ? <SkeletonList rows={2} /> : (
            <div className="aq-svc-tracks">
              {tracks.map(t => (
                <label key={t.id} className={`ui-check${t.has_lyrics ? '' : ' is-disabled'}`}>
                  <input type="checkbox" disabled={!t.has_lyrics} checked={pick.includes(t.id)}
                    onChange={e => setPick(l => (e.target.checked ? [...l, t.id] : l.filter(x => x !== t.id)))} />
                  <span className="min-0">
                    {t.track_number}. {t.title}
                    <small>{!t.has_lyrics ? '가사가 없어요 (연주곡이거나 가사를 넣지 않았어요)' : t.lrc ? '싱크 가사가 이미 있어요 — 다시 만들 수 있어요' : '가사 있음'}</small>
                  </span>
                </label>
              ))}
            </div>
          )}
        </div>
      )}
      <div className="ui-field">
        <label htmlFor="lsNotes">전할 말<span className="aq-optional">선택</span></label>
        <textarea id="lsNotes" rows={2} maxLength={2000} placeholder="예: 2절 랩 부분이 빨라요" value={notes} onChange={e => setNotes(e.target.value)} />
      </div>
      <div className="aq-sticky-foot">
        <button type="submit" className={`ui-btn is-primary studio-submit-wide${busy ? ' is-busy' : ''}`} disabled={busy || !pick.length}>
          {pick.length ? `${pick.length}곡 · ${money(price * pick.length)} 결제하고 신청` : '곡을 골라 주세요'}
        </button>
      </div>
    </form>
  );
}

function LrcForm({ releases }: { releases: Release[] }) {
  const close = useModalClose();
  const toast = useToast();
  const [releaseId, setReleaseId] = useState('');
  const tracks = useTracks(releaseId);
  const [trackId, setTrackId] = useState('');
  const [lrc, setLrc] = useState('');
  const [busy, setBusy] = useState(false);
  useEffect(() => { setTrackId(tracks?.[0]?.id ?? ''); }, [tracks]);
  const lines = useMemo(() => (lrc.trim() ? lrcLines(lrc) : null), [lrc]);
  const readFile = async (f: File | undefined) => {
    if (!f) return;
    if (f.size > 200_000) { toast('LRC 파일이 너무 커요.', 'error'); return; }
    setLrc(await f.text());
  };
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!trackId) { toast('곡을 골라 주세요.', 'error'); return; }
    if (lines === null) { toast('[00:12.30]처럼 시간이 붙은 싱크 가사(LRC)를 넣어 주세요.', 'error'); return; }
    setBusy(true);
    try { const n = await putLrc(releaseId, trackId, lrc); toast(`싱크 가사 ${n}줄을 올렸어요.`, 'success'); close(); }
    catch (err) { toast(errorMessage(err), 'error'); } finally { setBusy(false); }
  };
  return (
    <form className="aq-svc-form" onSubmit={submit} noValidate>
      <p className="small muted">직접 만든 싱크 가사(LRC)를 올리면 발매와 함께 가사 서비스에 등록해요. 만들기 어렵다면 AI 가사 싱크(곡당 5,000원)를 신청하세요.</p>
      <ReleasePick releases={releases} value={releaseId} onChange={setReleaseId} />
      {releaseId && (
        <div className="ui-field">
          <label htmlFor="lrcTrack">곡</label>
          <select id="lrcTrack" value={trackId} onChange={e => setTrackId(e.target.value)} disabled={!tracks}>
            {(tracks ?? []).map(t => <option key={t.id} value={t.id}>{t.track_number}. {t.title}{t.lrc ? ' (싱크 가사 있음)' : ''}</option>)}
          </select>
        </div>
      )}
      <div className="ui-field">
        <div className="aq-al-section-head">
          <label htmlFor="lrcText" className="ui-field-label">싱크 가사 (LRC)</label>
          <label className="ui-btn is-sm aq-svc-file">
            <input type="file" accept=".lrc,text/plain" className="sr-only" onChange={e => void readFile(e.target.files?.[0])} />
            <Glyph name="download" size={14} />파일에서
          </label>
        </div>
        <textarea id="lrcText" rows={8} maxLength={60000} className="aq-svc-lrc" placeholder={'[00:12.30]첫 줄 가사\n[00:15.80]둘째 줄 가사'} value={lrc} onChange={e => setLrc(e.target.value)} />
        <small className={`ui-field-help${lrc.trim() && lines === null ? ' aq-help-error' : ''}`}>
          {!lrc.trim() ? '줄마다 [분:초.백분의1초] 시간이 있어야 해요.' : lines === null ? '시간이 없는 줄이 있거나 형식이 맞지 않아요.' : `시간이 붙은 줄 ${lines}개`}
        </small>
      </div>
      <div className="aq-sticky-foot">
        <button type="submit" className={`ui-btn is-primary studio-submit-wide${busy ? ' is-busy' : ''}`} disabled={busy}>올리기</button>
      </div>
    </form>
  );
}

// ---------------------------------------------------------------------------
// 결제 · 신청 상세
// ---------------------------------------------------------------------------
function PayForm({ o, balance, card, onPaid }: { o: AddonOrder; balance: number; card: boolean; onPaid: (o: AddonOrder) => void }) {
  const toast = useToast();
  const enough = balance >= o.amount;
  const [method, setMethod] = useState<'SETTLEMENT' | 'CARD'>(enough || !card ? 'SETTLEMENT' : 'CARD');
  const [busy, setBusy] = useState(false);
  const pay = async () => {
    setBusy(true);
    try {
      if (method === 'SETTLEMENT') { onPaid(await paySettlement(o.id)); toast('정산금에서 결제했어요. 담당자에게 접수됐어요.', 'success'); }
      else { await payCard(o.id); /* 체험 모드는 바로, 실서버는 결제창으로 넘어간다 */ onPaid({ ...o, status: 'SUBMITTED', pay_method: 'CARD', paid_at: new Date().toISOString() }); }
    } catch (err) { toast(errorMessage(err), 'error'); } finally { setBusy(false); }
  };
  return (
    <div className="aq-svc-pay">
      <div className="aq-svc-pay-sum">
        <span className="min-0">
          <small>{SERVICE_LABEL[o.service]}{o.tier ? ` · ${TIER[o.tier].label}` : ''}{o.quantity > 1 ? ` · ${o.quantity}곡` : ''}</small>
          <strong>{o.title}</strong>
        </span>
        <b>{money(o.amount)}</b>
      </div>
      <div className="ui-choices" role="radiogroup" aria-label="결제 방법">
        <button type="button" role="radio" aria-checked={method === 'SETTLEMENT'} disabled={!enough} className="ui-choice aq-svc-tier" onClick={() => setMethod('SETTLEMENT')}>
          <span>
            <b>정산금으로 결제</b>
            <small>{enough ? `출금 가능한 정산금 ${money(balance)}에서 바로 빠져요.` : `출금 가능한 정산금이 ${money(balance)}이라 모자라요.`}</small>
          </span>
          <span className="ui-choice-mark" aria-hidden="true">{method === 'SETTLEMENT' && <CheckIcon size={11} />}</span>
        </button>
        <button type="button" role="radio" aria-checked={method === 'CARD'} disabled={!card} className="ui-choice aq-svc-tier" onClick={() => setMethod('CARD')}>
          <span>
            <b>카드 결제</b>
            <small>{card ? '토스페이먼츠 결제창에서 카드·간편결제로 결제해요.' : '카드 결제는 준비 중이에요.'}</small>
          </span>
          <span className="ui-choice-mark" aria-hidden="true">{method === 'CARD' && <CheckIcon size={11} />}</span>
        </button>
      </div>
      <p className="aq-studio-note">담당자가 작업을 시작하기 전에는 언제든 취소할 수 있고, 결제한 방법으로 전액 환불돼요.</p>
      <button type="button" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy || (method === 'SETTLEMENT' && !enough)} onClick={() => void pay()}>
        {money(o.amount)} 결제하기
      </button>
    </div>
  );
}

function OrderDetail({ o, onPay, onChanged }: { o: AddonOrder; onPay: () => void; onChanged: (o: AddonOrder) => void }) {
  const toast = useToast();
  const confirm = useConfirm();
  const close = useModalClose();
  const st = orderStage(o);
  const d = o.detail as Record<string, string | string[]>;
  const facts: [string, string][] = [
    ['서비스', `${SERVICE_LABEL[o.service]}${o.tier ? ` · ${TIER[o.tier].label}` : ''}`],
    ['발매', o.release_title ?? ''],
    ['곡', o.tracks.map(t => t.title).join(', ')],
    ['금액', `${money(o.amount)}${o.quantity > 1 ? ` (${o.quantity}곡 × ${money(o.unit_price)})` : ''}`],
    ['결제', o.pay_method ? `${o.pay_method === 'CARD' ? '카드' : '정산금'} · ${niceDate(o.paid_at ?? '')}${o.refunded_at ? ' · 환불됨' : ''}` : '결제 전'],
  ];
  if (o.service === 'MV_DISTRIBUTION') {
    facts.push(['영상', String(d.mv_title ?? '')], ['길이', String(d.duration ?? '')],
      ['플랫폼', (Array.isArray(d.platforms) ? d.platforms : []).map(p => PLATFORM_NAMES[p] ?? p).join(', ')]);
  }
  const rating = o.result.rating || (typeof d.rating === 'string' ? d.rating : '');
  const ratingNo = o.result.rating_no || (typeof d.rating_no === 'string' ? d.rating_no : '');
  const links = Object.entries(o.result.links ?? {});
  const cancel = async () => {
    const ok = await confirm({
      title: '신청을 취소할까요?',
      message: o.paid_at ? `결제한 ${money(o.amount)}은 ${o.pay_method === 'CARD' ? '카드 결제 취소로' : '정산금으로'} 환불돼요.` : '아직 결제 전이라 바로 취소돼요.',
      confirmLabel: '신청 취소', danger: true,
    });
    if (!ok) return;
    try { onChanged(await cancelOrder(o.id)); toast('신청을 취소했어요.'); close(); }
    catch (err) { toast(errorMessage(err), 'error'); }
  };
  return (
    <div className="aq-request-detail">
      <StageCard steps={st.steps} current={st.current} complete={st.complete} tone={st.stopped ? 'error' : undefined} note={statusNote(o)} label="신청 진행 단계" />
      {(rating || links.length > 0) && (
        <section className="aq-request-block">
          <h3>결과</h3>
          {rating && <p><span>영상물 등급</span>{RATING_LABEL[rating] ?? rating}{ratingNo ? ` · ${ratingNo}` : ''}</p>}
          {links.map(([p, u]) => <p key={p}><span>{PLATFORM_NAMES[p] ?? LYRIC_SERVICE_LABEL[p] ?? p}</span><a href={u} target="_blank" rel="noopener noreferrer">{u}</a></p>)}
        </section>
      )}
      <section className="aq-request-block">
        <h3>신청 내용</h3>
        {facts.filter(([, v]) => v).map(([k, v]) => <p key={k}><span>{k}</span>{v}</p>)}
      </section>
      {o.service === 'LYRIC_SYNC' && o.status === 'COMPLETED' && o.release_id && (
        <Callout tone="success">싱크 가사가 등록됐어요. 플랫폼마다 화면에 보이기까지 며칠 걸릴 수 있어요.</Callout>
      )}
      {o.status === 'AWAITING_PAYMENT' && <button type="button" className="ui-btn is-primary is-block" onClick={() => { close(); onPay(); }}>{money(o.amount)} 결제하기</button>}
      {(o.status === 'AWAITING_PAYMENT' || o.status === 'SUBMITTED') && (
        <button type="button" className="ui-btn is-block aq-danger-text aq-request-cancel" onClick={() => void cancel()}>신청 취소</button>
      )}
    </div>
  );
}
