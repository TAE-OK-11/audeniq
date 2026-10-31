// 곡 상세 — 한 화면 안의 탭: 개요 · 참여자·권리 · 등록·계약 · 녹음·발매 · 수익 · 자료·활동 (기획안 4.4).
// 발매 상세와 같은 틀(#view-release): 뒤로 버튼, 커버·제목·버튼이 있는 머리 카드, 붙는 탭, 정보 카드(aq-relinfo-card),
// 진행 단계(StageCard), 기록(ui-timeline), 보완은 계약 묶음과 같은 번호 줄. 저작권 관리만의 새 모양은 두지 않는다.
// 곡 상세 → 권리 확인 → 저작권 관리 신청 → 계약 → 진행 상태까지 다른 화면을 거의 거치지 않고 이어진다 (기획안 10.2)
import { useEffect, useState } from 'react';
import { Link, useNavigate, useParams, useSearchParams } from '../lib/router';
import { api } from '../api/client';
import { pubApi, type Work, type WorkContributor } from '../api/publishing';
import { errorMessage } from '../api/errors';
import { useAsync } from '../hooks/useAsync';
import { MOCK } from '../lib/mode';
import { APP_LABEL, APP_TONE, AR_LABEL, AR_TONE, CONTRACT_LABEL, CONTRACT_TONE, FIX_FIELDS, LANGUAGES, ORIGIN_LABEL, fixNote, isFixField, type FixField, REG_LABEL, REG_SENTENCE, REG_STEPS, WORK_TYPES, pct, regStep, splitIssues, splitReady, workOrigin } from '../lib/publishing';
import { StageCard } from '../components/StageCard';
import { STATUS_LABEL, localStamp, statusTone } from '../lib/format';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { Modal } from '../components/Modal';
import { SkeletonBlock } from '../components/Skeleton';
import { ReleaseCover } from '../components/ReleaseCover';
import { ReviewNote } from '../components/CorrectionList';
import { Glyph } from '../components/Glyph';
import { RegBadge, RegProgress, roleText } from '../components/publishing/parts';
import { SplitEditor } from '../components/publishing/SplitEditor';
import { RoyaltyTable } from '../components/publishing/Royalties';
import { ContractManage } from '../components/publishing/PublishingContracts';
import { AttachmentList, AttachmentModal } from '../components/publishing/Attachments';
import { Badge, Callout, EmptyState, KeyValue, Segmented, StatGrid, StatTile, Tabs } from '../ui';
import { SplitDiffList } from '../components/publishing/WorkSheet';
import { changeNeedsReview, reusableWorks, splitDiff } from '../lib/publishingPrep';
import { Money } from '../components/Money';
import { useReportRows } from '../hooks/useReportRows';
import '../styles/split/ReleaseDetail.css';
import '../styles/split/Publishing.css';

type Tab = 'overview' | 'rights' | 'register' | 'recordings' | 'revenue' | 'files';
// 탭은 발매 상세와 같은 순서·이름으로 — 정보 · 성과 · (배급·계약 자리에) 저작권·계약 (변경 기록은 사용자에게 보여 주지 않는다).
// 예전 '참여자·권리'·'녹음·발매'·'자료' 탭은 정보 탭 안의 칸이 됐다 (주소로 오면 그 칸으로 내려간다).
// 자료는 첨부 목록 하나뿐이라 탭 하나를 차지할 만큼이 아니었다
const TABS: { value: Tab; label: string }[] = [
  { value: 'overview', label: '정보' }, { value: 'revenue', label: '성과' }, { value: 'register', label: '저작권·계약' },
];
const SECTION: Partial<Record<Tab, string>> = { rights: 'workRights', recordings: 'workRecordings', files: 'workFiles' };
const label = <T extends readonly (readonly [string, string])[]>(list: T, v: string) => list.find(([k]) => k === v)?.[1] ?? v;
const Empty = ({ children }: { children: string }) => <span className="aq-empty-val">{children}</span>;

/** 다음에 할 일 — 개요 탭 맨 위 알림 상자 하나 */
function nextAction(w: Work): { text: string; tab?: Tab; to?: string; btn: string; tone: 'warn' | 'info' } | null {
  const canEdit = w.myRole === 'owner' || w.myRole === 'editor';
  // 보완 요청은 탭 위 보완 카드(FixPanel)가 맡는다 — 여기서 한 번 더 알리지 않는다
  if (w.fix) return null;
  const mine = w.contributors.find(c => c.isMe && c.confirm === 'pending');
  if (mine) return { text: `내 지분 ${pct(mine.share)}를 확인해 주세요. 맞으면 확인, 다르면 변경을 요청해요.`, tab: 'rights', btn: '확인하기', tone: 'warn' };
  if (!canEdit) return null;
  if (!splitReady(w.contributors)) return { text: splitIssues(w.contributors).find(i => i.level === 'error')?.text ?? '지분을 맞춰 주세요.', tab: 'rights', btn: '지분 고치기', tone: 'warn' };
  // 맡기기 전 안내('저작권 관리 시작')는 두지 않는다 — 머리 카드의 '저작권 관리 신청' 버튼과 같은 일을 한 화면에 두 번 권했다
  return null;
}

/** 보완 요청 — 발매 상세의 보완 카드와 같은 모양(aq-fix-panel): 담당자가 고른 항목·서류를 할 일 줄로(누르면 고칠 자리로),
 *  맨 아래 다시 심사받기 하나. 담당자가 따로 의견을 남겼을 때만 검토팀 말풍선을 보인다 (발매의 REVIEW_NOTE와 같다).
 *  아티스트가 담당자에게 적는 칸은 두지 않는다 — 고친 내용이 곧 답이다.
 *  요청한 서류는 발매 보완과 같은 '권리·보완 서류'에서 낸다 — 같은 제출 화면·알림·담당자 확인을 쓴다 */
function FixPanel({ work, onDone, goRights }: { work: Work; onDone: (w: Work) => void; goRights: () => void }) {
  const toast = useToast();
  const nav = useNavigate();
  const [busy, setBusy] = useState(false);
  const fix = work.fix!;
  const fields = fix.fields.filter(isFixField);
  const docs = fix.documents ?? [];
  const note = fixNote(fix);
  const open = fields.length + docs.length;
  const goField = (f: FixField) => (f.startsWith('contributors.') ? goRights() : nav(`/works/${work.id}/edit`));
  const send = async () => {
    if (busy) return;
    setBusy(true);
    try { const w = await pubApi.resubmitFix(work.id, '', []); toast('보완했어요. 담당자가 다시 확인해요.', 'success'); onDone(w); }
    catch (err) { toast(errorMessage(err, '보내지 못했어요. 다시 시도해 주세요.'), 'error'); }
    finally { setBusy(false); }
  };
  return (
    <section className="aq-fix-panel ui-tone-warn" aria-labelledby="workFixHead">
      <p className="eyebrow">{open ? `보완 요청 ${open}건` : '보완 필요'}</p>
      <h2 id="workFixHead" className="aq-fix-title">
        {open ? `${open}가지 고치면 다시 심사받을 수 있어요` : '검토팀 의견을 확인하고 고쳐 주세요'}
      </h2>
      <div className="aq-fix-body">
        {note && <ReviewNote text={note} />}
        {open > 0 && (
          <ul className="aq-task-list aq-fix-tasks">
            {fields.map(f => (
              <li key={f}>
                <button type="button" className="aq-task is-warn" onClick={() => goField(f)}>
                  <span className="ui-icon" aria-hidden="true"><Glyph name="pencil" size={18} /></span>
                  <span className="aq-task-text">
                    <strong>{FIX_FIELDS[f]}</strong>
                    <small>{f.startsWith('contributors.') ? '참여자·지분에서 고쳐요 · 원래 정보는 담당자 확인 전까지 그대로 남아요' : '곡 정보 고치기에서 고쳐요'}</small>
                  </span>
                  <span className="aq-task-go">고치러 가기<i aria-hidden="true"><Glyph name="chevron-right" size={14} /></i></span>
                </button>
              </li>
            ))}
            {docs.map(d => (
              <li key={d}>
                <button type="button" className="aq-task is-warn" onClick={() => nav('/rights')}>
                  <span className="ui-icon" aria-hidden="true"><Glyph name="doc" size={18} /></span>
                  <span className="aq-task-text"><strong>{d}</strong><small>권리·보완 서류에서 파일을 올려 주세요</small></span>
                  <span className="aq-task-go">서류 첨부하기<i aria-hidden="true"><Glyph name="chevron-right" size={14} /></i></span>
                </button>
              </li>
            ))}
          </ul>
        )}
        <button type="button" className={`ui-btn is-primary aq-fix-cta${busy ? ' is-busy' : ''}`} disabled={busy} onClick={() => void send()}>
          {busy ? '보내는 중' : '보완 완료하고 다시 심사받기'}
        </button>
      </div>
    </section>
  );
}

function LinkRecordingModal({ work, onClose, onLinked }: { work: Work; onClose: () => void; onLinked: (w: Work) => void }) {
  const toast = useToast();
  const { data, loading } = useAsync(async () => {
    const rels = await api.listReleases();
    const details = await Promise.all(rels.map(r => api.getRelease(r.id).catch(() => null)));
    return details.filter(Boolean).flatMap(d => d!.tracks.map(t => ({ t, d: d! })));
  }, [], { key: 'release-tracks-for-works' });
  const [busy, setBusy] = useState<string | null>(null);
  const have = new Set(work.recordings.map(r => r.trackId));
  const link = async (row: NonNullable<typeof data>[number]) => {
    setBusy(row.t.id);
    try {
      const w = await pubApi.linkRecording(work.id, { releaseId: row.d.id, trackId: row.t.id, title: row.t.title, releaseTitle: row.d.title, version: row.t.version, isrc: row.t.isrc, status: row.d.status });
      toast(`‘${row.t.title}’ 녹음을 연결했어요.`, 'success');
      onLinked(w);
      onClose();
    } catch (err) { toast(errorMessage(err), 'error'); } finally { setBusy(null); }
  };
  return (
    <Modal title="녹음 연결" onClose={onClose}>
      <p className="ui-field-help">이 곡을 부른 녹음(원곡·어쿠스틱·라이브·리믹스 등)을 골라요. 한 녹음에 여러 곡이 들어 있어도 각각 연결할 수 있어요.</p>
      {loading ? <SkeletonBlock height={160} /> : !data?.length ? (
        <EmptyState compact title="연결할 발매 곡이 없어요">발매를 등록하면 녹음을 이 곡에 연결할 수 있어요.</EmptyState>
      ) : (
        <div className="ui-list">
          {data.map(row => (
            <button key={row.d.id + row.t.id} type="button" className="ui-row" disabled={have.has(row.t.id) || !!busy} onClick={() => void link(row)}>
              <ReleaseCover id={row.d.id} src={row.d.draft?.coverData} />
              <span className="min-0">
                <span className="ui-row-title">{row.t.title || '제목 없는 곡'}{row.t.version && ` (${row.t.version})`}</span>
                <span className="ui-row-meta"><span>{row.d.title}</span>{row.t.isrc && <span>{row.t.isrc}</span>}</span>
              </span>
              {have.has(row.t.id) ? <Badge tone="green">연결됨</Badge> : busy === row.t.id ? <Badge tone="blue">연결 중</Badge> : <Glyph name="plus" size={16} />}
            </button>
          ))}
        </div>
      )}
    </Modal>
  );
}

export function WorkDetail() {
  const { id = '' } = useParams();
  const nav = useNavigate();
  const toast = useToast();
  const confirm = useConfirm();
  const [params, setParams] = useSearchParams();
  const asked = params.get('tab') as Tab | null;
  const tab = (asked && TABS.some(t => t.value === asked) ? asked : 'overview') as Tab;
  const setTab = (v: Tab) => {
    const p = new URLSearchParams(params);
    if (v === 'overview' || SECTION[v]) p.delete('tab'); else p.set('tab', v);
    setParams(p, { replace: true });
    const sec = SECTION[v];
    if (sec) window.requestAnimationFrame(() => document.getElementById(sec)?.scrollIntoView({ behavior: 'smooth', block: 'start' }));
  };
  // ?tab=rights·recordings 같은 예전 주소 — 정보 탭의 그 칸으로
  useEffect(() => { if (asked && SECTION[asked]) setTab(asked); }, [asked]); // eslint-disable-line react-hooks/exhaustive-deps

  const { data, error, loading, reload } = useAsync(() => pubApi.getWork(id), [id], { key: `work:${id}` });
  const [work, setWork] = useState<Work | null>(null);
  useEffect(() => { if (data) setWork(data); }, [data]);
  const w = work ?? data ?? null;
  const { data: royalties } = useAsync(() => pubApi.listRoyalties(id), [id], { key: `royalties:${id}` });
  const { data: contracts } = useAsync(() => (tab === 'register' ? pubApi.listContracts() : Promise.resolve(null)), [tab === 'register'], { key: 'pub-contracts' });
  const { data: apps } = useAsync(() => (tab === 'register' ? pubApi.listApplications() : Promise.resolve(null)), [tab === 'register'], { key: 'pub-applications' });

  // 연결된 녹음의 배급 수익 — 음악 리포트와 같은 행(곡 이름이 맞는 행만)
  const { rows: reportRows } = useReportRows();
  const [splitDraft, setSplitDraft] = useState<WorkContributor[] | null>(null);
  const [savingSplit, setSavingSplit] = useState(false);
  const [linking, setLinking] = useState(false);
  const [attaching, setAttaching] = useState(false);
  const [splitView, setSplitView] = useState<'now' | 'pending'>('pending');
  const { data: allWorks } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  // 발매 전 곡의 A&R 제출 — 진행 중이면 '제출' 대신 진행 상황으로 (같은 곡을 두 번 내지 않게)
  const { data: arList } = useAsync(() => pubApi.listAr(), [], { key: 'ar-submissions' });

  if (loading && !w) return <div id="view-release" className="view"><SkeletonBlock height={220} /><SkeletonBlock height={280} /></div>;
  if (error || !w) {
    return (
      <div className="view">
        <div className="ui-empty">
          <h2>곡을 열 수 없어요.</h2>
          <p>{error || '삭제됐거나 볼 수 있는 권한이 없는 곡이에요.'}</p>
          <button type="button" className="ui-btn is-soft" onClick={reload}>다시 불러오기</button>
        </div>
      </div>
    );
  }

  // 할 말이 있는 탭만 — 발매도 저작권료도 없는 곡의 '성과'(0원 두 칸), 신청 전 곡의 '저작권·계약'(신청 안 했어요 두 번)은 빈 화면이었다.
  // 신청은 머리의 '저작권 관리 신청' 버튼이 입구
  const applied = !!w.application || !!w.contractId || w.registration !== 'none' || !!w.fix;
  const shownTabs = TABS.filter(t => (t.value !== 'revenue' || w.recordings.length > 0 || !!royalties?.length) && (t.value !== 'register' || applied));
  const view: Tab = shownTabs.some(t => t.value === tab) ? tab : 'overview';
  const canEdit = w.myRole === 'owner' || w.myRole === 'editor';
  const next = nextAction(w);
  const distRows = reportRows.filter(r => w.recordings.some(rec => (r.releaseId ? r.releaseId === rec.releaseId : r.release === rec.releaseTitle) && r.track === rec.title));
  const distRevenue = distRows.reduce((n, r) => n + r.revenue, 0);
  const app = w.application;
  const contract = contracts?.find(c => c.id === w.contractId) ?? null;
  const application = apps?.find(a => a.id === app?.id) ?? null;
  const update = (nw: Work) => setWork(nw);
  // 세 갈래(lib/publishing WORK_ORIGINS): 발매 전 곡은 A&R 피칭, 발매된 곡(내 발매·다른 아티스트 발매)은 저작권 관리 신청
  const origin = workOrigin(w);
  const released = origin !== 'unreleased';
  const canApply = canEdit && released && w.registration === 'none' && (!app || app.status === 'withdrawn');
  const ar = arList?.find(a => a.workId === w.id && a.status !== 'closed') ?? null;
  const me = w.contributors.find(c => c.isMe);

  const saveSplit = async () => {
    if (!splitDraft || savingSplit) return;
    setSavingSplit(true);
    try {
      const nw = await pubApi.updateWork(w.id, { contributors: splitDraft }, w.rowVersion);
      update(nw);
      setSplitDraft(null);
      toast(nw.pendingChange ? '변경을 요청했어요.' : '지분을 저장했어요.', { tone: 'success', sub: nw.pendingChange ? '참여자 확인과 담당자 검토가 끝날 때까지 지금 값으로 관리돼요.' : splitReady(nw.contributors) ? '바뀐 지분은 참여자에게 확인을 요청해요.' : '합계가 100%가 아니에요. 신청 전에 맞춰 주세요.' });
    } catch (err) {
      toast(errorMessage(err, '지분을 저장하지 못했어요.'), { tone: 'error', action: { label: '다시 시도', onClick: h => { h.close(); void saveSplit(); } } });
      if (err && typeof err === 'object' && 'status' in err && (err as { status: number }).status === 409) reload();
    } finally { setSavingSplit(false); }
  };

  const confirmMine = async (agree: boolean) => {
    if (!me) return;
    if (!agree && !(await confirm({ title: '지분 변경을 요청할까요?', message: '곡 소유자에게 변경 요청을 보내요. 서로 맞출 때까지 저작권 관리 신청은 멈춰요.', confirmLabel: '변경 요청' }))) return;
    try {
      update(await pubApi.respondSplit(w.id, agree));
      toast(agree ? `내 지분 ${pct(me.share)}를 확인했어요.` : '변경을 요청했어요.', 'success');
    } catch (err) { toast(errorMessage(err), 'error'); }
  };

  const withdraw = async () => {
    if (!app) return;
    const ok = await confirm({ title: '저작권 관리 신청을 철회할까요?', message: '철회해도 곡 정보와 지분은 그대로 남아요. 나중에 다시 신청할 수 있어요.', confirmLabel: '철회하기', danger: true });
    if (!ok) return;
    try { await pubApi.withdrawApplication(app.id); toast('신청을 철회했어요.', 'success'); reload(); } catch (err) { toast(errorMessage(err), 'error'); }
  };

  const unlink = async (recId: string, title: string) => {
    const ok = await confirm({ title: `‘${title}’ 연결을 풀까요?`, message: '발매와 녹음은 그대로예요. 이 곡과의 연결만 풀어요.', confirmLabel: '연결 풀기', danger: true });
    if (!ok) return;
    try { update(await pubApi.unlinkRecording(w.id, recId)); toast('연결을 풀었어요.', 'success'); } catch (err) { toast(errorMessage(err), 'error'); }
  };

  const remove = async () => {
    const ok = await confirm({ title: '이 곡을 지울까요?', message: '곡 정보·지분·첨부 자료가 모두 지워져요. 발매와 녹음은 그대로예요.', confirmLabel: '지우기', danger: true });
    if (!ok) return;
    try { await pubApi.deleteWork(w.id); toast('곡을 지웠어요.', 'success'); nav('/releases?tab=works', { replace: true }); } catch (err) { toast(errorMessage(err), 'error'); }
  };

  const fromRelease = params.get('release');
  const backToAlbum = !!fromRelease && w.recordings.some(r => r.releaseId === fromRelease);
  const backTo = backToAlbum ? `/releases/${encodeURIComponent(fromRelease!)}?tab=publishing` : '/releases?tab=works';

  // 정보 고치기 옆 버튼은 갈래마다 하나 — 발매된 곡은 피칭할 일이 없으니 A&R을 두지 않고, 발매 전 곡은 신청 대신 A&R.
  // 이미 신청·관리 중인 곡에는 옆 버튼이 없다 (진행은 아래 단계와 '저작권·계약' 탭)
  const buttons = canEdit && (
    <>
      <Link to={`/works/${w.id}/edit${backToAlbum ? `?release=${encodeURIComponent(fromRelease!)}` : ''}`} className="ui-btn is-soft">정보 고치기</Link>
      {/* 지분이 맞지 않으면 신청이 막힌다 — 그때는 진한 버튼을 신청에 두지 않는다 (위 안내의 '지분 고치기'가 할 일) */}
      {canApply && <Link to={`/publishing/apply?work=${w.id}`} className={`ui-btn ${splitReady(w.contributors) ? 'is-primary' : 'is-soft'}`}>저작권 관리 신청</Link>}
      {!released && (ar
        ? <Link to="/ar" className="ui-btn is-soft">A&R 진행 보기</Link>
        : <Link to={`/ar?work=${w.id}`} className="ui-btn is-primary">A&R 제출</Link>)}
    </>
  );

  return (
    <div id="view-release" className="view aq-pbl-work">
      <div className="spaced-actions">
        <button type="button" className="ui-btn is-white is-sm aq-back-link" onClick={() => nav(backTo)}><Glyph name="arrow-left" size={15} className="aq-glyph-lead" />{backToAlbum ? '앨범으로 돌아가기' : '곡 목록'}</button>
      </div>

      <div className="aq-detail-hero" aria-label="곡 정보">
        <ReleaseCover id={w.recordings[0]?.releaseId ?? w.id} className="cover aq-detail-cover" />
        <div className="aq-detail-info">
          <div className="aq-detail-top">
            <span className="eyebrow">곡 · {ORIGIN_LABEL[origin]}</span>
            {released ? <RegBadge status={w.registration} /> : ar && <Badge tone={AR_TONE[ar.status]}>A&R {AR_LABEL[ar.status]}</Badge>}
          </div>
          <h1>{w.title}</h1>
          {/* 발매 상세처럼 이름만 — 지분은 정보 탭의 참여자·지분에서 */}
          <p className="aq-detail-artist">{w.contributors.map(c => c.name).join(' · ') || '참여자 없음'}</p>
          {/* 항목 단위로만 줄을 바꾼다 — ISWC 번호가 'T-' / '345…'로 쪼개지지 않게 */}
          <p className="aq-detail-meta">{[label(WORK_TYPES, w.workType), w.genre, w.iswc && `ISWC ${w.iswc}`, w.myRole === 'contributor' && '공동 참여 곡'].filter(Boolean)
            .map((x, i) => <span key={i}>{i > 0 && ' · '}<span className="aq-nowrap">{x}</span></span>)}</p>
        </div>
        <div className="aq-detail-actions is-wide">{buttons}</div>
        {/* 진행 단계는 발매 상세처럼 머리 카드 안 아래쪽 흰 판으로 — 맡기기 전과 등록이 끝난 뒤에는 배지만 */}
        {regStep(w.registration) >= 0 && w.registration !== 'registered' && (
          <StageCard steps={REG_STEPS.map((x, i) => (i === regStep(w.registration) && (w.registration === 'needs_fix' || w.registration === 'rejected') ? REG_LABEL[w.registration] : x))} current={Math.min(regStep(w.registration), REG_STEPS.length - 1)}
            tone={w.registration === 'rejected' ? 'error' : w.registration === 'needs_fix' ? 'warn' : undefined}
            note={REG_SENTENCE[w.registration]} label="저작권 등록 진행 단계" />
        )}
        <div className="aq-detail-actions is-narrow">{buttons}</div>
      </div>

      {/* 보완 요청은 발매 상세처럼 머리 카드와 탭 사이 — 어느 탭을 보고 있어도 먼저 보인다 */}
      {w.fix && canEdit && <FixPanel work={w} onDone={update} goRights={() => setTab('rights')} />}

      {/* 고를 탭이 '정보' 하나뿐이면(맡기기 전 · 미발매) 탭 줄을 그리지 않는다 — 혼자 화면 폭을 차지한 탭은 누를 일이 없다 */}
      {shownTabs.length > 1 && (
        <div className="aq-detail-tabs" style={{ top: 'var(--aq-header-h)' }}>
          <Tabs label="곡 상세 메뉴" value={view} onChange={setTab} className="is-fill"
            items={shownTabs} />
        </div>
      )}

      <div className="ui-tab-panel" key={view}>
        {view === 'overview' && (
          <div className="aq-detail-stack">
            {next && (
              <Callout tone={next.tone} action={next.to
                ? <Link to={next.to} className="ui-btn is-primary is-sm">{next.btn}</Link>
                : <button type="button" className="ui-btn is-white is-sm" onClick={() => setTab(next.tab!)}>{next.btn}</button>}>
                {next.text}
              </Callout>
            )}
            <div className="aq-relinfo-card">
              <h2 className="subhead">곡 정보</h2>
              <KeyValue cols={2} items={[
                ['곡 유형', label(WORK_TYPES, w.workType)],
                ['주요 언어', label(LANGUAGES, w.language)],
                ['장르', w.genre || <Empty>입력 전</Empty>],
                ['공개 범위', w.visibility === 'shared' ? '참여자에게 공개' : '비공개'],
                ['ISWC', w.iswc ?? <Empty>{`발급 전 · ${w.internalCode}`}</Empty>],
                ['기존 퍼블리셔', w.hasPriorPublisher ? (w.priorPublisher || '있음') : '없음'],
                ['내 역할', me ? `${roleText(me)} · ${pct(me.share)}` : <Empty>참여하지 않음</Empty>],
                ['발매', origin === 'other_release' ? `${w.externalRelease?.artist ?? '다른 아티스트'} 발매` : w.recordings.length ? `녹음 ${w.recordings.length}개` : <Empty>발매 전</Empty>],
              ]} />
              {w.altTitles.length > 0 && <KeyValue items={[[ '대체 제목', w.altTitles.join(' · ') ]]} />}
              {w.description && <><h2 className="subhead">곡 설명</h2><p className="muted small break">{w.description}</p></>}
            </div>
          </div>
        )}

        {view === 'overview' && (
          <div className="aq-detail-stack" id="workRights">
            {me && me.confirm !== 'confirmed' && w.myRole === 'contributor' && (
              <Callout tone={me.confirm === 'disputed' ? 'danger' : 'warn'} action={me.confirm === 'pending' && (
                <span className="row-actions">
                  <button type="button" className="ui-btn is-white is-sm" onClick={() => void confirmMine(false)}>변경 요청</button>
                  <button type="button" className="ui-btn is-primary is-sm" onClick={() => void confirmMine(true)}>맞아요, 확인</button>
                </span>
              )}>
                {me.confirm === 'disputed' ? '변경을 요청했어요. 곡 소유자가 지분을 다시 나누면 알려 드려요.' : <>이 곡에서 내 몫은 <b>{pct(me.share)}</b>({roleText(me)})예요. 맞나요?</>}
              </Callout>
            )}
            <div className="aq-relinfo-card">
              <div className="ui-section-head">
                <h2 className="subhead">참여자·지분</h2>
                {canEdit && !splitDraft && <button type="button" className="ui-btn is-soft is-sm" onClick={() => setSplitDraft(structuredClone(w.pendingChange?.contributors ?? w.contributors))}>{w.pendingChange ? '변경값 고치기' : '지분 고치기'}</button>}
              </div>
              {splitDraft ? (
                <>
                  <SplitEditor value={splitDraft} onChange={setSplitDraft} reuse={reusableWorks(allWorks ?? [], w.id)} />
                  <SplitDiffList title="바뀌는 내용" changes={splitDiff(w.pendingChange?.contributors ?? w.contributors, splitDraft)} />
                  {changeNeedsReview(w) && <p className="ui-field-help">관리 중인 곡이라 지금 적용값은 그대로 두고, 모든 참여자가 확인하고 담당자가 검토한 뒤 바뀐 값으로 관리해요.</p>}
                  <div className="ui-actions">
                    <button type="button" className="ui-btn is-soft" onClick={() => setSplitDraft(null)} disabled={savingSplit}>취소</button>
                    <button type="button" className={`ui-btn is-primary${savingSplit ? ' is-busy' : ''}`} disabled={savingSplit} onClick={() => void saveSplit()}>{savingSplit ? '저장 중' : changeNeedsReview(w) ? '변경 요청' : '지분 저장'}</button>
                  </div>
                </>
              ) : (
                <>
                {w.pendingChange && (
                  <>
                    <Segmented label="볼 지분" value={splitView} onChange={setSplitView} fill size="sm"
                      options={[{ value: 'now', label: '지금 적용값' }, { value: 'pending', label: '검토 중인 변경값' }]} />
                    {splitView === 'pending' && <SplitDiffList title={`${localStamp(w.pendingChange.requestedAt)} 요청 · 검토 중`} changes={splitDiff(w.contributors, w.pendingChange.contributors)} />}
                  </>
                )}
                <SplitEditor value={w.pendingChange && splitView === 'pending' ? w.pendingChange.contributors : w.contributors} onChange={() => {}} readOnly
                  onRequestConfirm={canEdit ? c => void pubApi.requestSplitConfirm(w.id, c.id).then(nw => { update(nw); toast(`${c.name} 님에게 확인을 요청했어요.`, 'success'); }).catch(err => toast(errorMessage(err), 'error')) : undefined}
                  onSimulate={MOCK && canEdit ? c => void pubApi.simulateConfirm(w.id, c.id).then(update) : undefined} />
                </>
              )}
            </div>

          </div>
        )}

        {view === 'register' && (
          <div className="aq-detail-stack">
            <div className="aq-relinfo-card">
              <h2 className="subhead">저작권 등록 현황</h2>
              <RegProgress status={w.registration} log={w.registrationLog} stage={regStep(w.registration) < 0 || w.registration === 'registered'} />
              <p className="ui-field-help">신청이 승인돼도 관리 단체 등록은 따로 진행돼요. 등록이 끝나야 사용료를 받을 수 있어요.</p>
            </div>
            <div className="aq-relinfo-card">
              <div className="ui-section-head"><h2 className="subhead">저작권 관리 신청</h2>{app && <Badge tone={APP_TONE[app.status]}>{APP_LABEL[app.status]}</Badge>}</div>
              {app && app.status !== 'withdrawn' ? (
                <>
                  <dl className="ui-kv is-2">
                    <div className="aq-kv-code"><dt>신청 번호</dt><dd className="aq-nowrap">{app.number}</dd></div>
                    <div className="aq-kv-code"><dt>제출</dt><dd>{application?.submittedAt ? localStamp(application.submittedAt) : '—'}</dd></div>
                  </dl>
                  {application && (
                    <ol className="ui-timeline is-latest-first">
                      {[...application.history].reverse().map((h, i) => <li key={i}><strong>{h.text}</strong><small>{localStamp(h.at)}</small></li>)}
                    </ol>
                  )}
                  {/* 신청서와 계약서는 따로 — 승인되면 계약서가 만들어지고 계약서 화면에서 서명한다 */}
                  {application?.contractId && !w.contractId && (
                    <Callout tone="info" action={<Link to={`/contracts/publishing/${encodeURIComponent(application.contractId)}`} className="ui-btn is-primary is-sm">확인하고 서명</Link>}>
                      담당자가 승인했어요. 저작권 관리 계약서에 서명하면 계약이 맺어지고 관리 단체 등록이 시작돼요.
                    </Callout>
                  )}
                  <div className="row-actions">
                    <Link to={`/publishing/applications/${encodeURIComponent(app.id)}`} className="ui-btn is-soft is-sm">신청서 보기</Link>
                    {canEdit && ['submitted', 'in_review', 'needs_fix'].includes(app.status) && (
                      <button type="button" className="ui-btn is-text is-sm aq-danger-text" onClick={() => void withdraw()}>신청 철회</button>
                    )}
                  </div>
                </>
              ) : (
                <div className="empty-note">
                  {app?.status === 'withdrawn' ? '신청을 철회했어요.' : '아직 신청하지 않았어요.'}<br />저작권 관리를 쓰지 않아도 곡 정보는 계속 관리할 수 있어요.<br />
                  {canApply && <Link to={`/publishing/apply?work=${w.id}`} className="ui-btn is-white is-sm">저작권 관리 시작 <Glyph name="chevron-right" size={13} /></Link>}
                </div>
              )}
            </div>
            {w.contractId && (
              <div className="aq-relinfo-card">
                <div className="ui-section-head"><h2 className="subhead">계약</h2>{contract && <Badge tone={CONTRACT_TONE[contract.status]}>{CONTRACT_LABEL[contract.status]}</Badge>}</div>
                {contract ? (
                  <>
                    <ContractManage contract={contract} canEdit={canEdit} />
                    <div className="row-actions"><Link to={`/contracts/publishing/${contract.id}`} className="ui-btn is-text is-sm">계약서 보기</Link></div>
                  </>
                ) : w.myRole === 'contributor' ? <p className="muted small">곡 소유자의 계약으로 관리되고 있어요.</p> : <SkeletonBlock height={64} />}
              </div>
            )}
          </div>
        )}

        {view === 'overview' && (
          <div className="aq-relinfo-card" id="workRecordings">
            <div className="ui-section-head">
              <h2 className="subhead">{origin === 'other_release' ? '발매 정보' : '연결된 녹음·발매'}</h2>
              {canEdit && origin !== 'other_release' && <button type="button" className="ui-btn is-soft is-sm" onClick={() => setLinking(true)}>녹음 연결</button>}
            </div>
            {/* 다른 아티스트가 낸 발매는 AUDENIQ 카탈로그에 없다 — 곡을 등록할 때 적은 발매 정보를 그대로 보여 준다 */}
            {origin === 'other_release' && w.externalRelease && (
              <KeyValue cols={2} items={[
                ['발매 아티스트', w.externalRelease.artist],
                ['발매 제목', w.externalRelease.title],
                ['발매일', w.externalRelease.releaseDate || <Empty>모름</Empty>],
                ['ISRC', w.externalRelease.isrc || <Empty>모름</Empty>],
                ['레이블·유통사', w.externalRelease.label || <Empty>모름</Empty>],
              ]} />
            )}
            {w.recordings.length ? (
              <div className="ui-list aq-stagger">
                {w.recordings.map(r => (
                  <div key={r.id} className="ui-row">
                    <ReleaseCover id={r.releaseId} />
                    <Link to={`/releases/${r.releaseId}?track=${encodeURIComponent(r.trackId)}`} className="min-0">
                      <span className="ui-row-title">{r.title}{r.version && ` (${r.version})`}</span>
                      <span className="ui-row-meta"><span>{r.releaseTitle}</span><span>{r.isrc ?? 'ISRC 발급 전'}</span></span>
                    </Link>
                    <span className="ui-row-end">
                      {r.status && <Badge tone={statusTone(r.status)}>{STATUS_LABEL[r.status] ?? r.status}</Badge>}
                      {canEdit && <button type="button" className="ui-btn is-text is-sm" onClick={() => void unlink(r.id, r.title)}>연결 풀기</button>}
                    </span>
                  </div>
                ))}
              </div>
            ) : origin !== 'other_release' && (
              <div className="empty-note">
                아직 발매되지 않은 곡이에요.<br />직접 발매했으면 그 녹음을 연결하고, 다른 아티스트가 발매했으면 ‘정보 고치기’에서 발매 정보를 적어 주세요. 발매되면 저작권 관리를 신청할 수 있어요.<br />
                {canEdit && <button type="button" className="ui-btn is-white is-sm" onClick={() => setLinking(true)}>발매 곡 연결 <Glyph name="chevron-right" size={13} /></button>}
              </div>
            )}
            {linking && <LinkRecordingModal work={w} onClose={() => setLinking(false)} onLinked={update} />}
          </div>
        )}

        {view === 'revenue' && (
          <div className="aq-detail-stack">
            {/* 한 곡이 번 돈을 한 화면에 — 녹음 배급 수익과 저작권료는 다른 돈이라 나란히 두되 더하지 않는다 */}
            <StatGrid cols={2}>
              <StatTile label="녹음 배급 수익" value={<Money value={distRevenue} />} hint={`연결된 녹음 ${w.recordings.length}개 · 스트리밍·다운로드 (리포트 기준)`} to={w.recordings[0] ? `/releases/${w.recordings[0].releaseId}?tab=stats` : '/reports'} />
              <StatTile label="저작권료 (확정·지급)" value={<Money value={(royalties ?? []).filter(r => r.status !== 'estimated').reduce((n, r) => n + r.net, 0)} />} hint="작사·작곡 몫 · 관리 단체 정산" />
            </StatGrid>
            <div className="aq-relinfo-card">
              <h2 className="subhead">저작권료 내역</h2>
              {royalties ? <RoyaltyTable rows={royalties} showWork={false} summary={false} emptyText={w.registration === 'registered' ? undefined : '저작권 등록이 끝나면 관리 단체가 나눠 주는 사용료가 여기에 들어와요.'} /> : <SkeletonBlock height={160} />}
            </div>
            {distRows.length > 0 && (
              <div className="aq-relinfo-card">
                <h2 className="subhead">녹음 배급 수익</h2>
                <div className="ui-list is-inset">
                  {distRows.map((r, i) => (
                    <div key={i} className="ui-row is-flat">
                      <span className="min-0"><span className="ui-row-title">{r.platform}</span><span className="ui-row-meta"><span>{r.release}</span><span>{r.period}</span><span>{r.plays.toLocaleString('ko-KR')}회</span></span></span>
                      <Money value={r.revenue} />
                    </div>
                  ))}
                </div>
                <p className="ui-field-help">두 수익 모두 같은 정산 잔액에 쌓여 한 번에 받아요. <Link to="/settlement" className="aq-inline-link">정산·지급 보기</Link></p>
              </div>
            )}
          </div>
        )}

        {view === 'overview' && (
          <div className="aq-detail-stack">
            <div className="aq-relinfo-card" id="workFiles">
              <div className="ui-section-head">
                <h2 className="subhead">자료</h2>
                {canEdit && <button type="button" className="ui-btn is-soft is-sm" onClick={() => setAttaching(true)}><Glyph name="clip" size={14} className="aq-glyph-lead" />자료 첨부</button>}
              </div>
              <AttachmentList items={w.attachments} />
            </div>
            {w.myRole === 'owner' && (!app || app.status === 'withdrawn') && (
              <div className="row-actions"><button type="button" className="ui-btn is-text is-sm aq-danger-text" onClick={() => void remove()}>이 곡 지우기</button></div>
            )}
            {attaching && <AttachmentModal works={[w]} workId={w.id} onClose={() => setAttaching(false)} onAdded={update} />}
          </div>
        )}
      </div>
    </div>
  );
}
