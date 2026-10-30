// 곡 상세 — 한 화면 안의 탭: 개요 · 참여자·권리 · 등록·계약 · 녹음·발매 · 수익 · 자료·활동 (기획안 4.4).
// 발매 상세와 같은 틀(#view-release): 뒤로 버튼, 커버·제목·버튼이 있는 머리 카드, 붙는 탭, 정보 카드(aq-relinfo-card),
// 진행 단계(StageCard), 기록(ui-timeline), 보완은 계약 묶음과 같은 번호 줄. 저작권 관리만의 새 모양은 두지 않는다.
// 곡 상세 → 권리 확인 → 저작권 관리 신청 → 계약 → 진행 상태까지 다른 화면을 거의 거치지 않고 이어진다 (기획안 10.2)
import { useEffect, useState, type ChangeEvent } from 'react';
import { Link, useNavigate, useParams, useSearchParams } from '../lib/router';
import { api } from '../api/client';
import { pubApi, type Work, type WorkAttachment, type WorkContributor } from '../api/publishing';
import { errorMessage } from '../api/errors';
import { useAsync } from '../hooks/useAsync';
import { MOCK } from '../lib/mode';
import { APP_LABEL, APP_TONE, CONTRACT_LABEL, CONTRACT_TONE, LANGUAGES, REG_SENTENCE, REG_STEPS, WORK_TYPES, isManaged, pct, regStep, splitIssues, splitReady } from '../lib/publishing';
import { StageCard } from '../components/StageCard';
import { STATUS_LABEL, fileSize, localStamp, statusTone } from '../lib/format';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { Modal } from '../components/Modal';
import { FilePicker } from '../components/FilePicker';
import { SkeletonBlock } from '../components/Skeleton';
import { ReleaseCover } from '../components/ReleaseCover';
import { CheckIcon } from '../components/Check';
import { Glyph } from '../components/Glyph';
import { RegBadge, RegProgress, roleText } from '../components/publishing/parts';
import { SplitEditor } from '../components/publishing/SplitEditor';
import { RoyaltyTable } from '../components/publishing/Royalties';
import { ContractManage } from '../components/publishing/PublishingContracts';
import { Badge, Callout, EmptyState, StatGrid, StatTile, Tabs } from '../ui';
import { Money } from '../components/Money';
import { useReportRows } from '../hooks/useReportRows';
import '../styles/split/ReleaseDetail.css';

type Tab = 'overview' | 'rights' | 'register' | 'recordings' | 'revenue' | 'files';
// 탭은 발매 상세와 같은 순서·이름으로 — 정보 · 성과 · (배급·계약 자리에) 저작권·계약 · 기록.
// 예전 '참여자·권리'·'녹음·발매' 탭은 정보 탭 안의 칸이 됐다 (주소로 오면 그 칸으로 내려간다)
const TABS: { value: Tab; label: string }[] = [
  { value: 'overview', label: '정보' }, { value: 'revenue', label: '성과' }, { value: 'register', label: '저작권·계약' }, { value: 'files', label: '기록' },
];
const SECTION: Partial<Record<Tab, string>> = { rights: 'workRights', recordings: 'workRecordings' };
const KIND_LABEL: Record<WorkAttachment['kind'], string> = { demo: '데모 음원', lyrics: '가사', score: '악보', proof: '권리 증빙' };
const label = <T extends readonly (readonly [string, string])[]>(list: T, v: string) => list.find(([k]) => k === v)?.[1] ?? v;
const Empty = ({ children }: { children: string }) => <span className="aq-empty-val">{children}</span>;

/** 다음에 할 일 — 개요 탭 맨 위 알림 상자 하나 */
function nextAction(w: Work): { text: string; tab?: Tab; to?: string; btn: string; tone: 'warn' | 'info' } | null {
  const canEdit = w.myRole === 'owner' || w.myRole === 'editor';
  if (w.fix) return { text: `보완 요청이 왔어요. ${w.fix.reason}`, tab: 'register', btn: '보완하기', tone: 'warn' };
  const mine = w.contributors.find(c => c.isMe && c.confirm === 'pending');
  if (mine) return { text: `내 지분 ${pct(mine.share)}를 확인해 주세요. 맞으면 확인, 다르면 변경을 요청해요.`, tab: 'rights', btn: '확인하기', tone: 'warn' };
  if (!canEdit) return null;
  if (!splitReady(w.contributors)) return { text: splitIssues(w.contributors).find(i => i.level === 'error')?.text ?? '지분을 맞춰 주세요.', tab: 'rights', btn: '지분 고치기', tone: 'warn' };
  if (w.registration === 'none' && (!w.application || w.application.status === 'withdrawn'))
    return { text: '이 곡의 저작권도 관리할까요? 신청하면 AUDENIQ이 관리 단체 등록과 사용료 징수를 맡아요.', to: `/publishing/apply?work=${w.id}`, btn: '저작권 관리 시작', tone: 'info' };
  return null;
}

/** 보완 요청 — 계약 묶음(aq-bundle)과 같은 번호 줄로 할 일을 차례대로 (기획안 7.3) */
function FixPanel({ work, onDone, goRights }: { work: Work; onDone: (w: Work) => void; goRights: () => void }) {
  const toast = useToast();
  const [note, setNote] = useState('');
  const [files, setFiles] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [up, setUp] = useState(false);
  const fix = work.fix!;
  const needsDocs = (fix.documents?.length ?? 0) > 0;
  const infoOk = splitReady(work.contributors) && work.contributors.every(c => c.ipi || c.isMe);
  const add = async (e: ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0];
    e.target.value = '';
    if (!f) return;
    setUp(true);
    try { await api.uploadFile(f, 'DOCUMENT'); setFiles(x => [...x, f.name]); } catch (err) { toast(errorMessage(err, '파일을 올리지 못했어요.'), 'error'); } finally { setUp(false); }
  };
  const send = async () => {
    if (busy) return;
    setBusy(true);
    try { const w = await pubApi.resubmitFix(work.id, note.trim(), files); toast('보완 자료를 냈어요. 담당자가 다시 확인해요.', 'success'); onDone(w); }
    catch (err) { toast(errorMessage(err, '보내지 못했어요. 다시 시도해 주세요.'), 'error'); }
    finally { setBusy(false); }
  };
  return (
    <article className="aq-bundle is-needs" aria-label="보완 요청">
      <header className="aq-bundle-head">
        <div className="min-0"><strong>보완 요청</strong><span>{localStamp(fix.requestedAt)} · AUDENIQ 담당자</span></div>
        <Badge tone="amber" className="aq-bundle-state">보완 필요</Badge>
      </header>
      <Callout tone="warn">{fix.reason}</Callout>
      <ol className="aq-bundle-docs">
        <li className={infoOk ? 'is-done' : 'is-current'}>
          <span className="aq-bundle-no" aria-hidden="true">{infoOk ? <CheckIcon size={14} /> : 1}</span>
          <div className="min-0"><strong>빠진 정보 고치기</strong><span>원래 정보는 담당자 확인 전까지 그대로 남아요</span></div>
          <button type="button" className="ui-btn is-soft is-sm" onClick={goRights}>참여자·권리</button>
        </li>
        {needsDocs && (
          <li className={files.length ? 'is-done' : 'is-current'}>
            <span className="aq-bundle-no" aria-hidden="true">{files.length ? <CheckIcon size={14} /> : 2}</span>
            <div className="min-0">
              <strong>{fix.documents!.join(', ')}</strong>
              <span>{files.length ? files.join(', ') : '요청한 서류를 올려 주세요'}</span>
              <FilePicker id="fixFile" accept=".pdf,image/*" busy={up} onChange={e => void add(e)} />
            </div>
          </li>
        )}
        <li>
          <span className="aq-bundle-no" aria-hidden="true">{needsDocs ? 3 : 2}</span>
          <div className="min-0">
            <strong>담당자에게 한마디 (선택)</strong>
            <textarea className="ui-textarea" rows={2} value={note} onChange={e => setNote(e.target.value)} placeholder="예: 김하늘 님 IPI 번호를 추가했어요." />
          </div>
        </li>
      </ol>
      <div className="row-actions">
        <button type="button" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy || (needsDocs && !files.length)} onClick={() => void send()}>
          {busy ? '보내는 중' : '보완 자료 다시 제출'}
        </button>
      </div>
    </article>
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
  const [uploading, setUploading] = useState(false);

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

  const canEdit = w.myRole === 'owner' || w.myRole === 'editor';
  const next = nextAction(w);
  const distRows = reportRows.filter(r => w.recordings.some(rec => (r.releaseId ? r.releaseId === rec.releaseId : r.release === rec.releaseTitle) && r.track === rec.title));
  const distRevenue = distRows.reduce((n, r) => n + r.revenue, 0);
  const app = w.application;
  const contract = contracts?.find(c => c.id === w.contractId) ?? null;
  const application = apps?.find(a => a.id === app?.id) ?? null;
  const update = (nw: Work) => setWork(nw);
  const canApply = canEdit && w.registration === 'none' && (!app || app.status === 'withdrawn');
  const me = w.contributors.find(c => c.isMe);

  const saveSplit = async () => {
    if (!splitDraft || savingSplit) return;
    setSavingSplit(true);
    try {
      const nw = await pubApi.updateWork(w.id, { contributors: splitDraft }, w.rowVersion);
      update(nw);
      setSplitDraft(null);
      toast('지분을 저장했어요.', { tone: 'success', sub: splitReady(nw.contributors) ? '바뀐 지분은 참여자에게 확인을 요청해요.' : '합계가 100%가 아니에요. 신청 전에 맞춰 주세요.' });
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

  const upload = async (e: ChangeEvent<HTMLInputElement>, kind: WorkAttachment['kind']) => {
    const f = e.target.files?.[0];
    e.target.value = '';
    if (!f) return;
    setUploading(true);
    try { await api.uploadFile(f, kind === 'demo' ? 'AUDIO' : 'DOCUMENT'); update(await pubApi.addAttachment(w.id, kind, f.name, f.size)); toast('파일을 올렸어요.', 'success'); }
    catch (err) { toast(errorMessage(err, '파일을 올리지 못했어요.'), 'error'); } finally { setUploading(false); }
  };

  const remove = async () => {
    const ok = await confirm({ title: '이 곡을 지울까요?', message: '곡 정보·지분·첨부 자료가 모두 지워져요. 발매와 녹음은 그대로예요.', confirmLabel: '지우기', danger: true });
    if (!ok) return;
    try { await pubApi.deleteWork(w.id); toast('곡을 지웠어요.', 'success'); nav('/publishing?tab=songs', { replace: true }); } catch (err) { toast(errorMessage(err), 'error'); }
  };

  const buttons = canEdit && (
    <>
      <Link to={`/works/${w.id}/edit`} className="ui-btn is-soft">정보 고치기</Link>
      {canApply ? <Link to={`/publishing/apply?work=${w.id}`} className="ui-btn is-primary">저작권 관리 신청</Link>
        : <Link to={`/ar?work=${w.id}`} className="ui-btn is-soft">A&R 제출</Link>}
    </>
  );

  return (
    <div id="view-release" className="view">
      <div className="spaced-actions">
        <button type="button" className="ui-btn is-white is-sm aq-back-link" onClick={() => nav('/publishing?tab=songs')}><Glyph name="arrow-left" size={15} className="aq-glyph-lead" />곡 목록</button>
      </div>

      <div className="aq-detail-hero" aria-label="곡 정보">
        <ReleaseCover id={w.id} className="cover aq-detail-cover" />
        <div className="aq-detail-info">
          <div className="aq-detail-top">
            <span className="eyebrow">곡{w.recordings.length === 0 && ' · 미발매'}</span>
            <RegBadge status={w.registration} />
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
          <StageCard steps={REG_STEPS} current={Math.min(regStep(w.registration), REG_STEPS.length - 1)}
            tone={w.registration === 'rejected' ? 'error' : w.registration === 'needs_fix' ? 'warn' : undefined}
            note={REG_SENTENCE[w.registration]} label="저작권 등록 진행 단계" />
        )}
        <div className="aq-detail-actions is-narrow">{buttons}</div>
      </div>

      <div className="aq-detail-tabs" style={{ top: 'var(--aq-header-h)' }}>
        <Tabs label="곡 상세 메뉴" value={tab} onChange={setTab} className="is-fill"
          items={TABS.map(t => ({ ...t, count: t.value === 'register' && w.fix ? 1 : undefined }))} />
      </div>

      <div className="ui-tab-panel" key={tab}>
        {tab === 'overview' && (
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
              <dl className="ui-kv is-2">
                <div><dt>곡 유형</dt><dd>{label(WORK_TYPES, w.workType)}</dd></div>
                <div><dt>주요 언어</dt><dd>{label(LANGUAGES, w.language)}</dd></div>
                <div><dt>장르</dt><dd>{w.genre || <Empty>입력 전</Empty>}</dd></div>
                <div><dt>공개 범위</dt><dd>{w.visibility === 'shared' ? '참여자에게 공개' : '비공개'}</dd></div>
                <div><dt>ISWC</dt><dd>{w.iswc ?? <Empty>{`발급 전 · ${w.internalCode}`}</Empty>}</dd></div>
                <div><dt>기존 퍼블리셔</dt><dd>{w.hasPriorPublisher ? (w.priorPublisher || '있음') : '없음'}</dd></div>
                <div><dt>내 역할</dt><dd>{me ? `${roleText(me)} · ${pct(me.share)}` : <Empty>참여하지 않음</Empty>}</dd></div>
                <div><dt>연결된 녹음</dt><dd>{w.recordings.length ? `${w.recordings.length}개` : <Empty>미공개</Empty>}</dd></div>
                {w.altTitles.length > 0 && <div className="is-wide"><dt>대체 제목</dt><dd>{w.altTitles.join(' · ')}</dd></div>}
              </dl>
              <h2 className="subhead">곡 설명</h2>
              <p className="muted small break">{w.description || '등록된 설명이 없어요.'}</p>
            </div>
          </div>
        )}

        {tab === 'overview' && (
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
                {canEdit && !splitDraft && <button type="button" className="ui-btn is-soft is-sm" onClick={() => setSplitDraft(structuredClone(w.contributors))}>지분 고치기</button>}
              </div>
              {splitDraft ? (
                <>
                  <SplitEditor value={splitDraft} onChange={setSplitDraft} />
                  {isManaged(w) && <p className="ui-field-help">관리 중인 곡의 지분 변경은 모든 참여자가 확인하고 담당자가 검토한 뒤 반영돼요.</p>}
                  <div className="ui-actions">
                    <button type="button" className="ui-btn is-soft" onClick={() => setSplitDraft(null)} disabled={savingSplit}>취소</button>
                    <button type="button" className={`ui-btn is-primary${savingSplit ? ' is-busy' : ''}`} disabled={savingSplit} onClick={() => void saveSplit()}>{savingSplit ? '저장 중' : '지분 저장'}</button>
                  </div>
                </>
              ) : (
                <SplitEditor value={w.contributors} onChange={() => {}} readOnly
                  onRequestConfirm={canEdit ? c => void pubApi.requestSplitConfirm(w.id, c.id).then(nw => { update(nw); toast(`${c.name} 님에게 확인을 요청했어요.`, 'success'); }).catch(err => toast(errorMessage(err), 'error')) : undefined}
                  onSimulate={MOCK && canEdit ? c => void pubApi.simulateConfirm(w.id, c.id).then(update) : undefined} />
              )}
            </div>
            <div className="aq-relinfo-card">
              <h2 className="subhead">지분 변경 기록</h2>
              <ActivityList items={w.activity.filter(a => a.kind === 'rights')} empty="아직 지분을 바꾼 적이 없어요." />
            </div>
          </div>
        )}

        {tab === 'register' && (
          <div className="aq-detail-stack">
            {w.fix && canEdit && <FixPanel work={w} onDone={update} goRights={() => setTab('rights')} />}
            <div className="aq-relinfo-card">
              <h2 className="subhead">저작권 등록 현황</h2>
              <RegProgress status={w.registration} log={w.registrationLog} />
              <p className="ui-field-help">신청이 승인돼도 관리 단체 등록은 따로 진행돼요. 등록이 끝나야 사용료를 받을 수 있어요.</p>
            </div>
            <div className="aq-relinfo-card">
              <div className="ui-section-head"><h2 className="subhead">저작권 관리 신청</h2>{app && <Badge tone={APP_TONE[app.status]}>{APP_LABEL[app.status]}</Badge>}</div>
              {app && app.status !== 'withdrawn' ? (
                <>
                  <dl className="ui-kv is-2">
                    <div><dt>신청 번호</dt><dd>{app.number}</dd></div>
                    <div><dt>제출</dt><dd>{application?.submittedAt ? localStamp(application.submittedAt) : '—'}</dd></div>
                  </dl>
                  {application && (
                    <ol className="ui-timeline is-latest-first">
                      {[...application.history].reverse().map((h, i) => <li key={i}><strong>{h.text}</strong><small>{localStamp(h.at)}</small></li>)}
                    </ol>
                  )}
                  {canEdit && ['submitted', 'in_review', 'needs_fix'].includes(app.status) && (
                    <div className="row-actions"><button type="button" className="ui-btn is-text is-sm aq-danger-text" onClick={() => void withdraw()}>신청 철회</button></div>
                  )}
                </>
              ) : (
                <div className="empty-note">
                  {app?.status === 'withdrawn' ? '신청을 철회했어요.' : '아직 신청하지 않았어요.'}<br />저작권 관리를 쓰지 않아도 곡 정보는 계속 관리할 수 있어요.<br />
                  {canEdit && <Link to={`/publishing/apply?work=${w.id}`} className="ui-btn is-white is-sm">저작권 관리 시작 <Glyph name="chevron-right" size={13} /></Link>}
                </div>
              )}
            </div>
            {w.contractId && (
              <div className="aq-relinfo-card">
                <div className="ui-section-head"><h2 className="subhead">계약</h2>{contract && <Badge tone={CONTRACT_TONE[contract.status]}>{CONTRACT_LABEL[contract.status]}</Badge>}</div>
                {contract ? (
                  <>
                    <ContractManage contract={contract} canEdit={canEdit} />
                    <div className="row-actions"><Link to={`/contracts?pub=${contract.id}`} className="ui-btn is-text is-sm">계약서 보기</Link></div>
                  </>
                ) : w.myRole === 'contributor' ? <p className="muted small">곡 소유자의 계약으로 관리되고 있어요.</p> : <SkeletonBlock height={64} />}
              </div>
            )}
          </div>
        )}

        {tab === 'overview' && (
          <div className="aq-relinfo-card" id="workRecordings">
            <div className="ui-section-head">
              <h2 className="subhead">연결된 녹음·발매</h2>
              {canEdit && <button type="button" className="ui-btn is-soft is-sm" onClick={() => setLinking(true)}>녹음 연결</button>}
            </div>
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
                      {canEdit && <button type="button" className="ui-btn is-text is-sm aq-danger-text" onClick={() => void unlink(r.id, r.title)}>연결 풀기</button>}
                    </span>
                  </div>
                ))}
              </div>
            ) : (
              <div className="empty-note">
                아직 발매되지 않은 곡이에요.<br />녹음해서 발매하면 그 녹음을 이 곡에 연결해 주세요. 한 곡에 여러 버전을 연결할 수 있어요.<br />
                {canEdit && <button type="button" className="ui-btn is-white is-sm" onClick={() => setLinking(true)}>발매 곡 연결 <Glyph name="chevron-right" size={13} /></button>}
              </div>
            )}
            {linking && <LinkRecordingModal work={w} onClose={() => setLinking(false)} onLinked={update} />}
          </div>
        )}

        {tab === 'revenue' && (
          <div className="aq-detail-stack">
            {/* 한 곡이 번 돈을 한 화면에 — 녹음 배급 수익과 저작권료는 다른 돈이라 나란히 두되 더하지 않는다 */}
            <StatGrid cols={2}>
              <StatTile label="녹음 배급 수익" value={<Money value={distRevenue} />} hint={`연결된 녹음 ${w.recordings.length}개 · 스트리밍·다운로드 (리포트 기준)`} to={w.recordings[0] ? `/releases/${w.recordings[0].releaseId}?tab=stats` : '/reports'} />
              <StatTile label="저작권료 (확정·지급)" value={<Money value={(royalties ?? []).filter(r => r.status !== 'estimated').reduce((n, r) => n + r.net, 0)} />} hint="작사·작곡 몫 · 관리 단체 정산" />
            </StatGrid>
            <div className="aq-relinfo-card">
              <h2 className="subhead">저작권료 내역</h2>
              {royalties ? <RoyaltyTable rows={royalties} showWork={false} emptyText={w.registration === 'registered' ? undefined : '저작권 등록이 끝나면 관리 단체가 나눠 주는 사용료가 여기에 들어와요.'} /> : <SkeletonBlock height={160} />}
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

        {tab === 'files' && (
          <div className="aq-detail-stack">
            <div className="aq-relinfo-card">
              <h2 className="subhead">자료</h2>
              {w.attachments.length ? (
                <dl className="ui-kv is-2">
                  {w.attachments.map(a => (
                    <div key={a.id}><dt>{KIND_LABEL[a.kind]}</dt><dd className="break">{a.name}{a.size ? <span className="aq-empty-val"> · {fileSize(a.size)}</span> : null}</dd></div>
                  ))}
                </dl>
              ) : <p className="muted small">아직 올린 자료가 없어요. 데모·가사·악보·권리 증빙을 모아 둘 수 있어요.</p>}
              {canEdit && (
                <div className="form-grid">
                  {(['demo', 'lyrics', 'score', 'proof'] as const).map(k => (
                    <div key={k} className="ui-field">
                      <label htmlFor={`up-${k}`}>{KIND_LABEL[k]} 올리기</label>
                      <FilePicker id={`up-${k}`} busy={uploading} onChange={e => void upload(e, k)} />
                    </div>
                  ))}
                </div>
              )}
            </div>
            <div className="aq-relinfo-card">
              <h2 className="subhead">변경 기록</h2>
              <ActivityList items={w.activity} empty="등록된 변경 기록이 없어요." />
            </div>
            {w.myRole === 'owner' && (!app || app.status === 'withdrawn') && (
              <div className="row-actions"><button type="button" className="ui-btn is-text is-sm aq-danger-text" onClick={() => void remove()}>이 곡 지우기</button></div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

/** 발매 상세 '기록' 탭과 같은 타임라인 */
function ActivityList({ items, empty }: { items: Work['activity']; empty: string }) {
  if (!items.length) return <div className="empty-note">{empty}</div>;
  return (
    <ol className="ui-timeline is-latest-first">
      {items.map((a, i) => (
        <li key={i}><strong>{a.text}</strong><small>{[a.who, localStamp(a.at)].filter(Boolean).join(' · ')}</small></li>
      ))}
    </ol>
  );
}
