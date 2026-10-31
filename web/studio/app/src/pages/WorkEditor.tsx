// 곡 만들기·고치기 — 기본 정보 · 참여자와 지분 · 권리 정보 · 첨부 자료 (기획안 4.1, 4.2).
// 저작권 관리 계약 없이도 곡만 등록해 둘 수 있고, 지분이 틀려도 임시 저장은 된다(신청할 때만 막는다).
// 발매 곡에서 들어오면(?release=&track=) 곡 정보·크레딧을 미리 채우되 지분은 사용자가 직접 확인해야 저장된다.
// 입력은 이 브라우저에 자동 저장돼 연결이 끊기거나 화면을 떠나도 이어서 쓸 수 있다 (기획안 10.3)
// 배급 신청서처럼 단계별로 한 화면에 한 가지만: 기본 정보 → 참여자·지분 → 권리·자료 → 확인.
// 발매 곡에서 왔으면 배급 신청서를 이미 다 쓴 것으로 본다: 제목·언어·장르·ISRC·크레딧·음원·가사는 다시 묻지 않고 가져와 보여 주기만 하고,
// 퍼블리싱에만 필요한 것(곡 유형·지분·관리 단체·ISWC·기존 계약)만 받는다. 배급 정보를 고치는 건 발매 상세에서
// 새 곡은 먼저 갈래를 고른다(WorkOriginStart): 내가 발매한 곡(발매 곡을 골라 가져오기) · 다른 아티스트가 발매한 곡(발매 정보를 적는다)
// · 발매 전 곡(데모를 붙여 Open A&R로). 마지막 버튼도 갈래를 따른다 — 발매된 곡은 '저장하고 신청', 발매 전 곡은 '저장하고 A&R 제출'
import { useEffect, useMemo, useRef, useState, type ChangeEvent } from 'react';
import { Link, useNavigate, useParams, useSearchParams } from '../lib/router';
import { api } from '../api/client';
import { pubApi, releaseAiUse, type ExternalRelease, type Work, type WorkAttachment, type WorkContributor, type WorkInput, type WorkOrigin, type WorkRecording } from '../api/publishing';
import { errorMessage } from '../api/errors';
import { useAsync } from '../hooks/useAsync';
import { readJSON, removeKey, writeJSON } from '../lib/storage';
import { stampNow } from '../lib/date';
import { localStamp, fileSize } from '../lib/format';
import { uid } from '../lib/store';
import { draftTrackOf } from '../lib/albumPrep';
import { AI_USE_OPTIONS, LANGUAGES, ORIGIN_LABEL, canEditInfo, editMode, WORK_TYPES, pct, similarWorks, splitIssues, splitNames, splitReady, workOrigin } from '../lib/publishing';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { FilePicker } from '../components/FilePicker';
import { SkeletonBlock } from '../components/Skeleton';
import { ReviewCard, WizardShell, type WizardStep } from '../components/WizardShell';
import '../styles/split/Upload.css';
import '../styles/split/Publishing.css';
import { Glyph } from '../components/Glyph';
import { SplitEditor } from '../components/publishing/SplitEditor';
import { reusableWorks } from '../lib/publishingPrep';
import { WorkRow } from '../components/publishing/parts';
import { Badge, Button, Callout, Field, KeyValue, Segmented, inputClass, selectClass } from '../ui';
import { WorkOriginStart } from '../components/publishing/WorkOriginStart';
import { ReleaseCover } from '../components/ReleaseCover';
import { roleText } from '../components/publishing/parts';
import { Reveal } from '../ui/Reveal';

type Form = Omit<WorkInput, 'recording'>;
const EMPTY: Form = {
  title: '', altTitles: [], workType: 'original', language: 'ko', genre: '', description: '', visibility: 'private',
  iswc: null, hasPriorPublisher: false, priorPublisher: null, contributors: [], attachments: [], origin: 'unreleased', externalRelease: null, aiUse: 'none',
};
const NO_RELEASE: ExternalRelease = { artist: '', title: '', releaseDate: null, isrc: null, label: null };
const ISRC_RE = /^[A-Z]{2}-?[A-Z0-9]{3}-?\d{2}-?\d{5}$/i;
const ORIGIN_TYPES: readonly WorkOrigin[] = ['unreleased', 'other_release'];
const ATTACH_KINDS: [WorkAttachment['kind'], string, string][] = [
  ['demo', '데모 음원', 'audio/*'], ['lyrics', '가사', '.txt,.pdf,.doc,.docx'], ['score', '악보', '.pdf,image/*'], ['proof', '권리 증빙 자료', '.pdf,image/*'],
];
const ISWC_RE = /^T-?\d{3}\.?\d{3}\.?\d{3}-?\d$/i;

// 단계 제목은 배급 신청서와 같은 말투 — 두 줄 큰 제목 + 한 줄 안내
const REST: WizardStep[] = [
  { short: '참여자·지분', title: '참여자와 지분을\n확인해 주세요.', sub: '작사·작곡·편곡에 참여한 사람의 몫을 나눠요.' },
  { short: '권리·자료', title: '권리 정보와\n자료를 확인해 주세요.', sub: '모르는 칸은 비워 둬도 저장돼요.' },
  { short: '최종 확인', title: '곡 정보를\n마지막으로 확인해 주세요.', sub: '수정을 누르면 그 단계로 돌아가요.' },
];
const STEPS: WizardStep[] = [{ short: '기본 정보', title: '어떤 곡을\n등록할까요?', sub: '별표(*) 칸만 채우면 다음 단계로 넘어갈 수 있어요.' }, ...REST];
// 발매 곡에서 오면 첫 단계는 입력이 아니라 '배급 신청서에서 가져온 곡 확인'
const TRACK_STEPS: WizardStep[] = [{ short: '곡 확인', title: '배급 신청서에서\n가져왔어요.', sub: '발매 정보는 다시 적지 않아요. 퍼블리싱에 필요한 것만 더해 주세요.' }, ...REST];
const LAST = STEPS.length - 1;

interface Saved { form: Form; at: string; splitChecked: boolean; step?: number }

export function WorkEditor() {
  const { id } = useParams();
  const editing = !!id;
  const [params, setParams] = useSearchParams();
  const releaseId = params.get('release');
  const trackId = params.get('track');
  // 고른 갈래 — 내가 발매한 곡은 발매 곡(?release=&track=)으로 오니 여기는 다른 아티스트 발매 · 발매 전만
  const typeParam = params.get('type') as WorkOrigin | null;
  const pickedType = typeParam && ORIGIN_TYPES.includes(typeParam) ? typeParam : null;
  const nav = useNavigate();
  const toast = useToast();
  const confirm = useConfirm();
  const draftKey = `work-draft:${id ?? (trackId ? `track-${trackId}` : `new-${pickedType ?? ''}`)}`;

  const { data: existing, error: loadError } = useAsync(() => (id ? pubApi.getWork(id) : Promise.resolve(null)), [id], { key: id ? `work:${id}` : undefined });
  const { data: works } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const { data: release } = useAsync(() => (releaseId ? api.getRelease(releaseId) : Promise.resolve(null)), [releaseId]);
  const track = release?.tracks.find(t => t.id === trackId) ?? null;
  // 내 관리 단체·IPI — 발매 곡에서 가져올 때 내 줄에 바로 채운다 (계정 정보에 한 번 적어 두면 곡마다 다시 적지 않는다)
  const { data: myCreator, loading: creatorLoading } = useAsync(() => (!editing ? pubApi.myCreator().catch(() => null) : Promise.resolve(null)), [editing], { key: !editing ? 'my-creator' : undefined });

  const [form, setForm] = useState<Form | null>(null);
  const [restored, setRestored] = useState<Saved | null>(null);
  const [savedAt, setSavedAt] = useState<string>('');
  const [splitChecked, setSplitChecked] = useState(editing);
  const [busy, setBusy] = useState(false);
  const [uploading, setUploading] = useState<string | null>(null);
  const [tried, setTried] = useState(false);
  const [moreOpen, setMoreOpen] = useState(false);
  const [step, setStep] = useState(0);
  const [reached, setReached] = useState(0);
  const base = useRef<string>('');

  // 처음 채우기: 고치기 → 서버 곡, 발매 곡 → 곡 정보, 그 밖 → 빈 칸. 이 브라우저에 쓰다 만 내용이 있으면 이어 쓰기를 묻는다
  useEffect(() => {
    if (form) return;
    if (editing && !existing) return;
    if (!editing && !releaseId && !pickedType) return; // 갈래를 고른 뒤에 채운다
    if (releaseId && !release) return;
    if (!editing && creatorLoading) return;
    let start: Form = EMPTY;
    if (existing) {
      const { title, altTitles, workType, language, genre, description, visibility, iswc, hasPriorPublisher, priorPublisher, contributors, attachments } = existing;
      start = { title, altTitles, workType, language, genre, description, visibility, iswc, hasPriorPublisher, priorPublisher, contributors, attachments, origin: workOrigin(existing), externalRelease: existing.externalRelease ?? null, aiUse: existing.aiUse ?? 'none' };
    } else if (track && release) {
      const d = draftTrackOf(track, release.draft);
      const people = new Map<string, WorkContributor>();
      const add = (names: string[], role: WorkContributor['roles'][number]) => {
        for (const n of names) {
          const cur = people.get(n);
          if (cur) { if (!cur.roles.includes(role)) cur.roles.push(role); continue; }
          // 크레딧은 가져오되 지분은 0 — 사용자가 직접 나눠야 한다 (기획안 4.2)
          const isMe = n === release.artist || (!!myCreator && n === myCreator.name);
          people.set(n, { id: uid(), creatorId: null, name: n, roles: [role], society: isMe ? myCreator?.society ?? null : null, ipi: isMe ? myCreator?.ipi ?? null : null, share: 0, publisherShare: 0, confirm: 'pending', external: true, isMe });
        }
      };
      add(splitNames(track.composers ?? d?.composers), 'composer');
      if (!d?.instrumental) add(splitNames(track.lyricists ?? d?.lyricists), 'lyricist');
      add(splitNames(d?.arrangers), 'arranger');
      // 배급 신청서에 올린 음원·가사는 첨부 자료로 미리 붙인다 — 같은 파일(asset)과 가사 본문 그대로, 다시 올리지 않게
      const audioName = track.audioName || d?.audioName;
      const fromRelease: WorkAttachment[] = [
        ...(audioName ? [{ id: uid(), kind: 'demo' as const, name: audioName, size: d?.audioSize || undefined, at: stampNow(), assetId: track.assetId ?? d?.assetId ?? null }] : []),
        ...(d?.lyrics?.trim() && !d.instrumental ? [{ id: uid(), kind: 'lyrics' as const, name: `${track.title} 가사 (배급 신청서)`, at: stampNow(), text: d.lyrics }] : []),
      ];
      start = {
        // 발매 신청서의 AI 답을 그대로 — 전체 AI 제작이면 '만들었어요', 작곡·작사 도움이면 '도움을 받았어요'
        ...EMPTY, origin: 'own_release', aiUse: releaseAiUse(release.draft?.options), title: track.title, genre: release.draft?.genreCustom?.trim() || release.draft?.genre || '', language: release.draft?.language ?? 'ko', attachments: fromRelease,
        // 크레딧에 한 사람뿐이면 그 사람이 100% — 나눌 것이 없는데 0%에서 다시 치게 하지 않는다
        workType: d?.instrumental ? 'instrumental' : 'original', contributors: [...people.values()].map(c => ({ ...(c.isMe ? { ...c, confirm: 'confirmed' as const, external: false } : c), share: people.size === 1 ? 100 : 0 })),
      };
    } else if (pickedType) {
      // 직접 등록하는 새 곡은 나부터 넣어 둔다 — 혼자 쓴 곡이 많으니 100%, 역할·관리 단체·IPI는 작가 프로필 그대로
      const roles = (myCreator?.roles ?? []).flatMap(r => (r === 'composer' || r === 'lyricist' || r === 'arranger' ? [r] : []));
      const me: WorkContributor[] = myCreator ? [{
        id: uid(), creatorId: myCreator.id, name: myCreator.name, roles: roles.length ? roles : ['composer'], society: myCreator.society ?? null, ipi: myCreator.ipi ?? null,
        share: 100, publisherShare: 0, confirm: 'confirmed', external: false, isMe: true,
      }] : [];
      start = { ...EMPTY, origin: pickedType, externalRelease: pickedType === 'other_release' ? NO_RELEASE : null, contributors: me };
    }
    base.current = JSON.stringify(start);
    const saved = readJSON<Saved | null>(draftKey, null);
    if (saved && JSON.stringify(saved.form) !== base.current) setRestored(saved);
    setForm(start);
  }, [existing, release, track, pickedType, creatorLoading]); // eslint-disable-line react-hooks/exhaustive-deps

  // 자동 저장 — 손을 멈추고 0.8초 뒤 이 브라우저에
  useEffect(() => {
    if (!form || restored) return;
    if (JSON.stringify(form) === base.current) return;
    const t = window.setTimeout(() => {
      const at = stampNow();
      if (writeJSON(draftKey, { form, at, splitChecked, step } satisfies Saved)) setSavedAt(at);
    }, 800);
    return () => window.clearTimeout(t);
  }, [form, splitChecked, step]); // eslint-disable-line react-hooks/exhaustive-deps

  // 저장하지 않은 채 창을 닫으려 하면 브라우저가 한 번 묻는다
  const dirty = !!form && JSON.stringify(form) !== base.current;
  useEffect(() => {
    if (!dirty) return;
    const on = (e: BeforeUnloadEvent) => { e.preventDefault(); };
    window.addEventListener('beforeunload', on);
    return () => window.removeEventListener('beforeunload', on);
  }, [dirty]);

  // 단계를 옮기면 화면 맨 위(단계 막대)부터 — 긴 단계 끝에서 다음 단계 중간으로 떨어지지 않게
  const first = useRef(true);
  useEffect(() => {
    setReached(r => Math.max(r, step));
    if (first.current) { first.current = false; return; }
    window.scrollTo({ top: 0, behavior: 'smooth' });
  }, [step]);

  const dupes = useMemo(() => (form && works ? similarWorks(form.title, works, id) : []), [form?.title, works, id]); // eslint-disable-line react-hooks/exhaustive-deps

  if (loadError) {
    return (
      <div className="view">
        <div className="ui-empty"><h2>곡을 불러오지 못했어요.</h2><p>{loadError}</p><Link to="/releases?tab=works" className="ui-btn is-soft">곡 목록으로</Link></div>
      </div>
    );
  }
  // 새 곡인데 발매 상태를 아직 고르지 않았으면 그것부터 (고른 뒤 '나가기'는 이 화면으로 돌아와 다시 고를 수 있다) — 내가 발매한 곡은 그 화면에서 발매 곡을 골라 바로 가져온다
  if (!editing && !releaseId && !pickedType) return <WorkOriginStart works={works ?? null} onPick={t => setParams(new URLSearchParams({ type: t }))} />;
  if (!form) return <div className="view"><SkeletonBlock height={120} /><SkeletonBlock height={320} /></div>;
  if (existing && existing.myRole !== 'owner' && existing.myRole !== 'editor') {
    return (
      <div className="view">
        <div className="ui-empty"><h2>이 곡을 고칠 권한이 없어요.</h2><p>공동 참여자는 곡 정보를 볼 수만 있어요. 지분이 다르면 곡 화면에서 변경을 요청해 주세요.</p><Link to={`/works/${existing.id}`} className="ui-btn is-soft">곡 보기</Link></div>
      </div>
    );
  }

  // 심사 중·계약서 서명 대기·등록 진행 중인 곡은 서버가 곡 정보를 받지 않는다 — 고쳐 놓고 저장에서 막히지 않게 먼저 알린다
  if (existing && !canEditInfo(existing)) {
    return (
      <div className="view">
        <div className="ui-empty">
          <h2>지금은 곡 정보를 고칠 수 없어요.</h2>
          <p>{editMode(existing) === 'locked'
            ? '담당자가 심사하거나 계약서 서명을 기다리는 곡이에요. 보완 요청이 오면 고칠 수 있어요.'
            : '관리 단체 등록이 진행 중이에요. 등록이 끝나면 고칠 수 있고, 지분은 곡 화면에서 변경을 요청할 수 있어요.'}</p>
          <Link to={`/works/${existing.id}`} className="ui-btn is-soft">곡 보기</Link>
        </div>
      </div>
    );
  }

  const set = <K extends keyof Form>(k: K, v: Form[K]) => setForm(f => (f ? { ...f, [k]: v } : f));
  const titleErr = tried && !form.title.trim() ? '곡 제목을 입력해 주세요.' : '';
  const iswcErr = form.iswc && !ISWC_RE.test(form.iswc) ? 'T-123.456.789-0 모양이에요. 모르면 비워 두세요 — 내부 번호로 관리해요.' : '';
  const issues = splitIssues(form.contributors);
  const fromTrack = !editing && !!track;
  const fromDraft = track && release ? draftTrackOf(track, release.draft) : undefined;
  // 여럿이 나누는 곡만 직접 확인을 받는다 — 혼자 쓴 곡은 100%라 확인할 것이 없다
  const needsSplitCheck = fromTrack && !splitChecked && form.contributors.length > 1;
  // 등록이 끝난 곡 — 곡 정보는 바로 고치고, 지분은 변경 요청으로 남는다 (참여자 확인·담당자 검토 뒤 반영)
  const locked = !!existing && editMode(existing) === 'review';
  const origin = form.origin ?? 'unreleased';
  const released = origin !== 'unreleased';
  const ext = form.externalRelease ?? NO_RELEASE;
  const setExt = (patch: Partial<ExternalRelease>) => set('externalRelease', { ...ext, ...patch });
  const artistErr = tried && origin === 'other_release' && !ext.artist.trim() ? '발매한 아티스트를 적어 주세요.' : '';
  const isrcErr = ext.isrc && !ISRC_RE.test(ext.isrc) ? 'KR-A26-25-00011 모양이에요. 모르면 비워 두세요.' : '';
  const hasDemo = form.attachments.some(a => a.kind === 'demo');
  // 다음 단계로 — 이 단계에서 꼭 필요한 것만 막는다 (지분이 덜 맞아도 저장은 된다)
  const next = () => {
    if (step === 0 && !form.title.trim()) { setTried(true); document.getElementById('workTitle')?.focus(); return; }
    if (step === 0 && origin === 'other_release' && !ext.artist.trim()) { setTried(true); document.getElementById('extArtist')?.focus(); return; }
    if (step === 1 && needsSplitCheck) { setTried(true); document.getElementById('splitCheck')?.focus(); return; }
    setStep(s => Math.min(LAST, s + 1));
  };
  const back = () => (step ? setStep(step - 1) : void cancel());

  const upload = async (kind: WorkAttachment['kind'], e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = '';
    if (!file) return;
    setUploading(kind);
    try {
      // 올린 파일을 기억해 두었다가 곡을 저장할 때 그 파일로 붙인다 (전에는 이름만 남아 서버에 붙지 않았다)
      const { assetId } = await api.uploadFile(file, kind === 'demo' ? 'AUDIO' : 'DOCUMENT');
      set('attachments', [...form.attachments, { id: uid(), kind, name: file.name, size: file.size, at: stampNow(), assetId }]);
    } catch (err) {
      toast(errorMessage(err, '파일을 올리지 못했어요.'), 'error');
    } finally { setUploading(null); }
  };

  const save = async (then?: 'apply' | 'ar') => {
    setTried(true);
    if (!form.title.trim()) { setStep(0); return; }
    if (origin === 'other_release' && !ext.artist.trim()) { setStep(0); return; }
    if (needsSplitCheck) { setStep(1); toast('가져온 크레딧의 지분을 확인해 주세요.', 'error'); return; }
    if (busy) return;
    setBusy(true);
    const clean: Form = {
      ...form, title: form.title.trim(), altTitles: form.altTitles.map(t => t.trim()).filter(Boolean),
      // 다른 아티스트 발매만 발매 정보를 둔다 — 발매 제목을 비우면 곡 제목과 같은 것으로 (싱글이 대부분)
      externalRelease: origin === 'other_release'
        ? { artist: ext.artist.trim(), title: ext.title.trim() || form.title.trim(), releaseDate: ext.releaseDate || null, isrc: ext.isrc?.trim() || null, label: ext.label?.trim() || null }
        : null,
      // 내가 발매한 곡은 발매 곡 연결로만 정해진다 — 고칠 때 보내지 않는다
      origin: origin === 'own_release' ? undefined : origin,
    };
    try {
      let w: Work;
      if (existing) {
        // 지분을 안 바꿨으면 보내지 않는다 — 관리 중인 곡은 보내기만 해도 같은 값의 '변경 요청'이 생겼다
        const same = JSON.stringify(clean.contributors) === JSON.stringify(existing.pendingChange?.contributors ?? existing.contributors);
        const { contributors, ...meta } = clean;
        w = await pubApi.updateWork(existing.id, same ? meta : { ...meta, contributors }, existing.rowVersion);
      } else {
        const recording: Omit<WorkRecording, 'id'> | null = track && release
          ? { releaseId: release.id, trackId: track.id, title: track.title, releaseTitle: release.title, version: track.version, isrc: track.isrc, status: release.status }
          : null;
        w = await pubApi.createWork({ ...clean, recording });
      }
      removeKey(draftKey);
      base.current = JSON.stringify(form);
      const ok = splitReady(form.contributors);
      toast(editing ? '곡 정보를 저장했어요.' : '곡을 만들었어요.', { tone: 'success', sub: !released ? (hasDemo ? undefined : '데모 음원을 올리면 Open A&R에 제출할 수 있어요.') : ok ? undefined : '지분을 맞추면 저작권 관리를 신청할 수 있어요.' });
      nav(then === 'apply' && ok ? `/publishing/apply?work=${encodeURIComponent(w.id)}`
        : then === 'ar' ? `/ar?work=${encodeURIComponent(w.id)}`
        : `/works/${w.id}${releaseId ? `?release=${encodeURIComponent(releaseId)}` : ''}`, { replace: true });
    } catch (err) {
      toast(errorMessage(err, '저장하지 못했어요. 입력한 내용은 이 브라우저에 남아 있어요.'), { tone: 'error', action: { label: '다시 시도', onClick: h => { h.close(); void save(then); } } });
    } finally { setBusy(false); }
  };

  const linkExisting = async (w: Work) => {
    if (!track || !release) return;
    const ok = await confirm({ title: `‘${w.title}’에 연결할까요?`, message: <>‘{release.title}’의 ‘{track.title}’ 녹음을 이 곡에 연결해요. 새 곡은 만들지 않아요.</>, confirmLabel: '연결하기' });
    if (!ok) return;
    try {
      await pubApi.linkRecording(w.id, { releaseId: release.id, trackId: track.id, title: track.title, releaseTitle: release.title, version: track.version, isrc: track.isrc, status: release.status });
      removeKey(draftKey);
      toast('기존 곡에 녹음을 연결했어요.', 'success');
      nav(`/works/${w.id}?tab=recordings`, { replace: true });
    } catch (err) { toast(errorMessage(err), 'error'); }
  };

  const cancel = async () => {
    if (dirty && !(await confirm({ title: '작성을 그만둘까요?', message: '쓰던 내용은 이 브라우저에 남아 있어 다음에 이어 쓸 수 있어요.', confirmLabel: '나가기' }))) return;
    nav(-1);
  };

  // 첫 단계 제목은 고른 발매 상태대로 — 앞 화면 '이 곡은 지금 어떤 상태인가요?' 다음에 같은 질문을 또 하지 않게
  const steps = fromTrack ? TRACK_STEPS : editing ? STEPS : STEPS.map((x, i) => (i ? x : {
    ...x, title: origin === 'other_release' ? '다른 아티스트가 낸\n곡을 알려 주세요.' : '발매 전 곡을\n알려 주세요.',
    sub: origin === 'other_release' ? '곡 제목과 발매 정보를 적어 주세요. 별표(*) 칸만 채우면 돼요.' : '곡 제목만 적어도 다음으로 넘어가요. 데모는 ‘권리·자료’에서 올려요.',
  }));
  return (
    <WizardShell
      className="aq-pbl-wizard"
      label="곡 정보 등록" title={editing ? '곡 정보 수정' : '새 곡 정보'}
      steps={editing ? steps.map((x, i) => (i ? x : { ...x, title: '곡 정보를\n고쳐 주세요.' })) : steps}
      step={step} reached={editing ? LAST : reached} onStep={setStep} onBack={back}
      saveLabel={savedAt ? `저장됨 ${localStamp(savedAt).slice(-5)}` : undefined}
      summary={{
        cover: release ? <ReleaseCover id={release.id} src={release.coverData} /> : undefined,
        title: form.title.trim() || '제목 없는 곡',
        lines: [fromTrack ? `${release?.title} · ${release?.artist}` : origin === 'other_release' && ext.artist.trim() ? `${ext.artist.trim()} 발매` : ORIGIN_LABEL[origin], `참여자 ${form.contributors.length}명`],
      }}
      railNote="입력 내용은 이 브라우저에 자동으로 임시 저장돼요."
      notice={<>
        {restored && (
          <Callout tone="info" action={
            <span className="row-actions">
              <Button size="sm" variant="white" onClick={() => { removeKey(draftKey); setRestored(null); }}>새로 쓰기</Button>
              <Button size="sm" variant="primary" onClick={() => { setForm(restored.form); setSplitChecked(restored.splitChecked); setSavedAt(restored.at); setStep(Math.min(LAST, restored.step ?? 0)); setRestored(null); }}>이어서 쓰기</Button>
            </span>
          }>
            <b>{localStamp(restored.at)}에 쓰던 내용이 있어요.</b> 저장하기 전에 화면을 떠났어요.
          </Callout>
        )}
        {locked && <Callout tone="warn">관리 중인 곡이에요. 곡 정보는 바로 바뀌고, 지분을 바꾸면 참여자 확인과 담당자 검토가 끝날 때까지 지금 값으로 관리돼요.</Callout>}

      </>}
      actions={<>
        <button type="button" id="wizardBack" className="ui-btn is-soft" onClick={back} disabled={busy}>{step ? '이전' : '나가기'}</button>
        {step < LAST ? (
          <button type="button" id="wizardNext" className="ui-btn is-primary" onClick={next}>다음으로</button>
        ) : !editing && released && splitReady(form.contributors) ? (<>
          <button type="button" id="wizardSaveNow" className="ui-btn is-soft" onClick={() => void save()} disabled={busy}>곡만 저장</button>
          <button type="button" id="wizardNext" className={`ui-btn is-primary${busy ? ' is-busy' : ''}`} onClick={() => void save('apply')} disabled={busy}>{busy ? '저장하는 중' : '저장하고 신청'}</button>
        </>) : !editing && !released && hasDemo ? (<>
          <button type="button" id="wizardSaveNow" className="ui-btn is-soft" onClick={() => void save()} disabled={busy}>곡만 저장</button>
          <button type="button" id="wizardNext" className={`ui-btn is-primary${busy ? ' is-busy' : ''}`} onClick={() => void save('ar')} disabled={busy}>{busy ? '저장하는 중' : '저장하고 A&R 제출'}</button>
        </>) : (
          <button type="button" id="wizardNext" className={`ui-btn is-primary${busy ? ' is-busy' : ''}`} onClick={() => void save()} disabled={busy}>{busy ? '저장하는 중' : editing ? '저장' : '곡 저장'}</button>
        )}
      </>}
    >
      {step === 0 && <>
        {fromTrack && release && track ? (
          <>
            {/* 배급 신청서의 곡 정보 — 읽기만. 퍼블리싱에 필요한 건 아래와 다음 단계에서 */}
            <div className="ui-card aq-work-from">
              <div className="aq-work-from-head">
                <ReleaseCover id={release.id} src={release.coverData} />
                <span className="min-0">
                  <span className="ui-row-title">{track.title}</span>
                  <span className="ui-row-meta"><span>{release.title}</span><span>{release.artist}</span></span>
                </span>
              </div>
              <KeyValue cols={2} items={[
                ['장르', form.genre || '—'],
                ['언어', LANGUAGES.find(x => x[0] === form.language)?.[1] ?? form.language],
                ['ISRC', track.isrc || '발매 때 발급'],
                ['버전', track.version || '원곡'],
                ['작곡', fromDraft?.composers || track.composers || '—'],
                ['작사', fromDraft?.instrumental ? '연주곡' : fromDraft?.lyricists || track.lyricists || '—'],
                ['음원', track.audioName || fromDraft?.audioName || '—'],
                ['가사', fromDraft?.instrumental ? '없음 (연주곡)' : fromDraft?.lyrics?.trim() ? '있음' : '없음'],
              ]} />
            </div>
            <Link to={`/releases/${encodeURIComponent(release.id)}`} className="ui-btn is-text is-sm">발매 정보는 발매 상세에서 고쳐요<Glyph name="chevron-right" size={14} /></Link>
            <h2 className="subhead">퍼블리싱 정보</h2>
          </>
        ) : (<>
          <Field label="곡 제목" htmlFor="workTitle" required error={titleErr}>
            <input id="workTitle" className={inputClass('lg')} value={form.title} maxLength={200} onChange={e => set('title', e.target.value)} placeholder="예: 새벽 네 시" autoComplete="off" />
          </Field>
          {/* 발매 전 ↔ 다른 아티스트 발매 — 피칭하던 곡을 다른 아티스트가 내면 여기서 바꾸고 발매 정보를 적는다.
              내가 발매한 곡은 발매 곡을 연결하면 저절로 바뀐다 (곡 화면 '녹음 연결') */}
          {/* 새 곡은 앞 화면에서 이미 골랐다 — 고칠 때만 (피칭하던 곡을 다른 아티스트가 냈을 때) */}
          {editing && origin !== 'own_release' && (
            <Field label="발매">
              <Segmented fill label="발매" value={origin} onChange={v => setForm(f => (f ? { ...f, origin: v, externalRelease: v === 'other_release' ? (f.externalRelease ?? NO_RELEASE) : f.externalRelease } : f))}
                options={[{ value: 'unreleased', label: '아직 발매 전' }, { value: 'other_release', label: '다른 아티스트가 발매' }]} />
            </Field>
          )}
          <Reveal open={origin === 'other_release'}>
            <section className="track-editor aq-pbl-attach-card" aria-label="발매 정보">
              <p className="ui-field-help">AUDENIQ 밖에서 나온 발매라 직접 적어 주세요. 담당자가 관리 단체에 등록할 때 이 정보로 녹음을 찾아요.</p>
              <div className="form-grid">
                <Field label="발매 아티스트" htmlFor="extArtist" required error={artistErr}>
                  <input id="extArtist" className={inputClass()} value={ext.artist} maxLength={200} onChange={e => setExt({ artist: e.target.value })} placeholder="예: 이온" autoComplete="off" />
                </Field>
                <Field label="발매 제목" htmlFor="extTitle" help="앨범·싱글 이름. 비우면 곡 제목과 같아요.">
                  <input id="extTitle" className={inputClass()} value={ext.title} maxLength={300} onChange={e => setExt({ title: e.target.value })} placeholder={form.title || '예: 겨울 편지'} autoComplete="off" />
                </Field>
                <Field label="발매일" htmlFor="extDate">
                  <input id="extDate" type="date" className={inputClass()} value={ext.releaseDate ?? ''} onChange={e => setExt({ releaseDate: e.target.value || null })} />
                </Field>
                <Field label="ISRC" htmlFor="extIsrc" warn={isrcErr} help={!isrcErr ? '음원 사이트·유통사에서 볼 수 있어요. 모르면 비워 두세요.' : undefined}>
                  <input id="extIsrc" className={inputClass()} value={ext.isrc ?? ''} maxLength={15} onChange={e => setExt({ isrc: e.target.value.trim() || null })} placeholder="KR-A26-25-00011" />
                </Field>
                <Field label="레이블·유통사" htmlFor="extLabel">
                  <input id="extLabel" className={inputClass()} value={ext.label ?? ''} maxLength={200} onChange={e => setExt({ label: e.target.value || null })} />
                </Field>
              </div>
            </section>
          </Reveal>
        </>)}
        {dupes.length > 0 && (
          <Reveal open>
            <Callout tone="warn">
              <b>비슷한 이름의 곡이 이미 있어요.</b> {fromTrack ? '같은 곡이면 새로 만들지 말고 연결해 주세요.' : '같은 곡이면 그 곡을 고쳐 주세요.'}
            </Callout>
            <div role="note">
              <div className="ui-list">
                {dupes.slice(0, 3).map(w => (
                  <WorkRow key={w.id} work={w} onClick={() => (fromTrack ? void linkExisting(w) : nav(`/works/${w.id}`))}
                    end={<span className="ui-btn is-xs is-white">{fromTrack ? '이 곡에 연결' : '열기'}</span>} />
                ))}
              </div>
            </div>
          </Reveal>
        )}
        <div className="form-grid">
          <Field label="곡 유형" htmlFor="workType">
            <select id="workType" className={selectClass()} value={form.workType} onChange={e => set('workType', e.target.value)}>
              {WORK_TYPES.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
            </select>
          </Field>
          {!fromTrack && <>
            <Field label="주요 언어" htmlFor="workLang">
              <select id="workLang" className={selectClass()} value={form.language} onChange={e => set('language', e.target.value)}>
                {LANGUAGES.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
              </select>
            </Field>
            <Field label="장르" htmlFor="workGenre">
              <input id="workGenre" className={inputClass()} value={form.genre} onChange={e => set('genre', e.target.value)} placeholder="예: Indie Pop" />
            </Field>
          </>}
          <Field label="공개 범위" htmlFor="workVis" help={form.visibility === 'shared' ? '참여자와 담당자가 볼 수 있어요.' : '나와 작업 공간 멤버만 볼 수 있어요.'}>
            <select id="workVis" className={selectClass()} value={form.visibility} onChange={e => set('visibility', e.target.value as Form['visibility'])}>
              <option value="private">비공개</option>
              <option value="shared">참여자에게 공개</option>
            </select>
          </Field>
        </div>
        {/* 신청 자격 — AI로만 만든 곡은 관리 단체가 등록하지 않는다 (DistroKid·TuneCore도 같은 기준) */}
        <Field label="AI 사용" help={AI_USE_OPTIONS.find(x => x[0] === form.aiUse)?.[2] || '곡(멜로디·가사)을 쓸 때 AI를 썼는지 알려 주세요. 보컬·믹싱·커버에만 썼다면 ‘AI 없이’예요.'}>
          <Segmented fill label="AI 사용" value={form.aiUse ?? 'none'} onChange={v => set('aiUse', v)}
            options={AI_USE_OPTIONS.map(([value, label]) => ({ value, label }))} />
        </Field>
        <Field label="곡 설명" htmlFor="workDesc" help={released ? '곡의 분위기·만든 배경.' : '곡의 분위기·만든 배경 — A&R 제출 때 그대로 가져가요.'}>
          <textarea id="workDesc" className="ui-textarea" rows={3} value={form.description} maxLength={2000} onChange={e => set('description', e.target.value)} />
        </Field>
        <button type="button" className="ui-btn is-text is-sm" aria-expanded={moreOpen} onClick={() => setMoreOpen(v => !v)}>
          대체 제목 {form.altTitles.filter(t => t.trim()).length > 0 && <Badge tone="blue">{form.altTitles.filter(t => t.trim()).length}</Badge>}<Glyph name="chevron-down" size={14} />
        </button>
        <Reveal open={moreOpen}>
          <Field label="대체 제목" htmlFor="workAlt" help="영문·번역 제목 등. 쉼표로 나눠 적어요.">
            <input id="workAlt" className={inputClass()} value={form.altTitles.join(',')} onChange={e => set('altTitles', e.target.value ? e.target.value.split(',') : [])} placeholder="예: 4 AM" />
          </Field>
        </Reveal>
      </>}

      {step === 1 && <>
        {fromTrack && <p className="ui-field-help aq-pbl-import-note"><Glyph name="check" size={16} />{form.contributors.length === 1 ? '발매 크레딧을 가져왔어요. 혼자 쓴 곡이라 100%로 채워 두었어요.' : '발매 크레딧을 가져왔어요. 각자의 지분만 입력해 주세요.'}</p>}
        <SplitEditor value={form.contributors} onChange={v => set('contributors', v)} showErrors={tried} reuse={reusableWorks(works ?? [], id)} />
        {fromTrack && form.contributors.length > 1 && (
          <label className={`ui-check aq-pbl-confirm${needsSplitCheck && tried ? ' aq-shake' : ''}`}>
            <input id="splitCheck" type="checkbox" checked={splitChecked} onChange={e => setSplitChecked(e.target.checked)} />
            <span><b>참여자와 지분을 직접 확인했어요.</b><small>크레딧은 가져오지만 지분은 자동으로 정하지 않아요.</small></span>
          </label>
        )}
      </>}

      {step === 2 && <>
        <h2 className="subhead">권리 정보</h2>
        <div className="form-grid">
          <Field label="ISWC" htmlFor="workIswc" warn={iswcErr} help={!iswcErr ? (existing ? `없으면 내부 번호 ${existing.internalCode}로 관리해요.` : '발급받은 경우에만. 없으면 내부 번호로 관리해요.') : undefined}>
            <input id="workIswc" className={inputClass()} value={form.iswc ?? ''} placeholder="T-123.456.789-0" onChange={e => set('iswc', e.target.value.trim() || null)} />
          </Field>
          <Field label="기존 저작권 관리 계약" htmlFor="workPrior">
            <select id="workPrior" className={selectClass()} value={form.hasPriorPublisher ? 'yes' : 'no'} onChange={e => set('hasPriorPublisher', e.target.value === 'yes')}>
              <option value="no">없어요</option>
              <option value="yes">다른 퍼블리셔와 계약이 있어요</option>
            </select>
          </Field>
        </div>
        <Reveal open={form.hasPriorPublisher}>
          <Field label="기존 퍼블리셔" htmlFor="workPriorName" help="계약이 겹치면 담당자가 먼저 확인해요. 신청은 할 수 있어요.">
            <input id="workPriorName" className={inputClass()} value={form.priorPublisher ?? ''} onChange={e => set('priorPublisher', e.target.value || null)} placeholder="퍼블리셔 이름" />
          </Field>
        </Reveal>
        <p className="ui-field-help">관리 단체·IPI 번호는 ‘참여자·지분’의 ‘관리 범위·작가 정보’에서 입력할 수 있어요.</p>

        <h2 className="subhead">첨부 자료</h2>
        <p className="ui-field-help">{fromTrack && form.attachments.length
          ? '배급 신청서의 음원·가사를 미리 붙여 두었어요. 악보나 권리 증빙이 있으면 더해 주세요.'
          : released ? '없어도 저장돼요. 권리 증빙·악보가 있으면 저작권 관리 심사가 빨라요.'
          : '데모 음원이 있어야 Open A&R에 제출할 수 있어요. 가사·악보는 있으면 더해 주세요.'}</p>
        {/* 배급 신청서의 곡 카드(track-editor)처럼 자료 칸을 카드 한 장으로 감싼다 */}
        <section className="track-editor aq-pbl-attach-card" aria-label="첨부 자료">
        <div className="form-grid">
          {ATTACH_KINDS.map(([kind, label, accept]) => {
            const mine = form.attachments.filter(a => a.kind === kind);
            return (
              <div key={kind} className="ui-field">
                <label htmlFor={`attach-${kind}`}>{label}</label>
                <FilePicker id={`attach-${kind}`} accept={accept} busy={uploading === kind} onChange={e => void upload(kind, e)} />
                {/* 가사는 붙여 넣어도 된다 — 파일 없이 본문으로 (곡 화면에서 바로 읽힌다) */}
                {kind === 'lyrics' && !mine.some(a => a.text) && (
                  <details className="aq-pbl-lyrics-paste">
                    <summary>파일 없이 붙여 넣기</summary>
                    <textarea className="ui-textarea" rows={6} maxLength={20000} aria-label="가사 붙여 넣기" placeholder={'1절\n…\n\n후렴\n…'}
                      onBlur={e => {
                        const text = e.target.value.trim();
                        if (text) set('attachments', [...form.attachments, { id: uid(), kind: 'lyrics', name: `${form.title.trim() || '곡'} 가사`, size: new Blob([text]).size, at: stampNow(), text }]);
                      }} />
                    <small className="ui-field-help">칸을 벗어나면 가사로 붙어요.</small>
                  </details>
                )}
                {mine.map(a => (
                  <small key={a.id} className="ui-field-help">
                    {a.name}{a.size ? ` · ${fileSize(a.size)}` : ''}{' '}
                    {/* 이미 곡에 붙은 자료는 여기서 빼지 않는다(서버는 더하기만) — 이번에 더한 것만 */}
                    {!existing?.attachments.some(x => x.id === a.id) && (
                      <button type="button" className="ui-btn is-text is-sm aq-danger-text" onClick={() => set('attachments', form.attachments.filter(x => x.id !== a.id))}>빼기</button>
                    )}
                  </small>
                ))}
              </div>
            );
          })}
        </div>
        </section>
      </>}

      {/* 최종 확인 — 배급 신청서 '최종 확인'과 같은 카드. 수정을 누르면 그 단계로 */}
      {step === LAST && (<>
        <div className="aq-catalog-cards aq-stagger">
          <ReviewCard head={fromTrack ? '곡 확인' : '기본 정보'} missing={!form.title.trim()} onEdit={() => setStep(0)}>
            {form.title.trim() || '제목 없음'} · {[WORK_TYPES.find(x => x[0] === form.workType)?.[1], LANGUAGES.find(x => x[0] === form.language)?.[1], form.genre].filter(Boolean).join(' · ')}
            {origin === 'other_release' && <><br />{[`${ext.artist.trim() || '아티스트 입력 전'} 발매`, ext.title.trim() || form.title.trim(), ext.releaseDate, ext.isrc].filter(Boolean).join(' · ')}</>}
          </ReviewCard>
          <ReviewCard head="참여자·지분" missing={!form.contributors.length} onEdit={() => setStep(1)}>
            {form.contributors.map(c => `${c.name} ${roleText(c)} ${pct(c.share)}`).join(' · ') || '참여자를 추가하면 저작권 관리를 신청할 수 있어요'}
          </ReviewCard>
          <ReviewCard head="권리·자료" onEdit={() => setStep(2)}>
            {[form.iswc || 'ISWC 없음', AI_USE_OPTIONS.find(x => x[0] === form.aiUse)?.[1], form.hasPriorPublisher ? `기존 퍼블리셔 ${form.priorPublisher || '있음'}` : '기존 계약 없음', `첨부 ${form.attachments.length}개`].join(' · ')}
          </ReviewCard>
        </div>
        <div className="ui-callout" style={{ marginTop: 24 }}>
          {!released
            ? (hasDemo && !editing ? '‘저장하고 A&R 제출’을 누르면 바로 Open A&R 제출로 이어져요. 발매되면 저작권 관리를 신청할 수 있어요.' : '발매 전 곡은 데모를 Open A&R에 보내 피칭해요. 발매되면 저작권 관리를 신청할 수 있어요.')
            : issues.some(i => i.level === 'error')
            ? '지분이 아직 맞지 않아요. 저장은 되고, 저작권 관리 신청은 지분을 맞춘 뒤에 할 수 있어요.'
            : form.contributors.length && !editing ? '지분이 맞아요. ‘저장하고 신청’을 누르면 바로 저작권 관리 신청으로 이어져요.' : '저장하면 곡 화면에서 언제든 다시 고칠 수 있어요.'}
        </div>
      </>)}
    </WizardShell>
  );
}
