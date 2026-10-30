// 곡 만들기·고치기 — 기본 정보 · 참여자와 지분 · 권리 정보 · 첨부 자료 (기획안 4.1, 4.2).
// 저작권 관리 계약 없이도 곡만 등록해 둘 수 있고, 지분이 틀려도 임시 저장은 된다(신청할 때만 막는다).
// 발매 곡에서 들어오면(?release=&track=) 곡 정보·크레딧을 미리 채우되 지분은 사용자가 직접 확인해야 저장된다.
// 입력은 이 브라우저에 자동 저장돼 연결이 끊기거나 화면을 떠나도 이어서 쓸 수 있다 (기획안 10.3)
import { useEffect, useMemo, useRef, useState, type ChangeEvent } from 'react';
import { Link, useNavigate, useParams, useSearchParams } from '../lib/router';
import { api } from '../api/client';
import { pubApi, type Work, type WorkAttachment, type WorkContributor, type WorkInput, type WorkRecording } from '../api/publishing';
import { errorMessage } from '../api/errors';
import { useAsync } from '../hooks/useAsync';
import { readJSON, removeKey, writeJSON } from '../lib/storage';
import { stampNow } from '../lib/date';
import { localStamp, fileSize } from '../lib/format';
import { uid } from '../lib/store';
import { draftTrackOf } from '../lib/albumPrep';
import { LANGUAGES, SOCIETIES, WORK_TYPES, similarWorks, splitIssues, splitNames, splitReady } from '../lib/publishing';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { FilePicker } from '../components/FilePicker';
import { SkeletonBlock } from '../components/Skeleton';
import { Glyph } from '../components/Glyph';
import { SplitEditor } from '../components/publishing/SplitEditor';
import { WorkRow } from '../components/publishing/parts';
import { Badge, Button, Callout, Field, inputClass, selectClass } from '../ui';
import { Reveal } from '../ui/Reveal';

type Form = Omit<WorkInput, 'recording'>;
const EMPTY: Form = {
  title: '', altTitles: [], workType: 'original', language: 'ko', genre: '', description: '', visibility: 'private',
  iswc: null, hasPriorPublisher: false, priorPublisher: null, contributors: [], attachments: [],
};
const ATTACH_KINDS: [WorkAttachment['kind'], string, string][] = [
  ['demo', '데모 음원', 'audio/*'], ['lyrics', '가사', '.txt,.pdf,.doc,.docx'], ['score', '악보', '.pdf,image/*'], ['proof', '권리 증빙 자료', '.pdf,image/*'],
];
const ISWC_RE = /^T-?\d{3}\.?\d{3}\.?\d{3}-?\d$/i;

interface Saved { form: Form; at: string; splitChecked: boolean }

export function WorkEditor() {
  const { id } = useParams();
  const editing = !!id;
  const [params] = useSearchParams();
  const releaseId = params.get('release');
  const trackId = params.get('track');
  const nav = useNavigate();
  const toast = useToast();
  const confirm = useConfirm();
  const draftKey = `work-draft:${id ?? (trackId ? `track-${trackId}` : 'new')}`;

  const { data: existing, error: loadError } = useAsync(() => (id ? pubApi.getWork(id) : Promise.resolve(null)), [id], { key: id ? `work:${id}` : undefined });
  const { data: works } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const { data: release } = useAsync(() => (releaseId ? api.getRelease(releaseId) : Promise.resolve(null)), [releaseId]);
  const track = release?.tracks.find(t => t.id === trackId) ?? null;

  const [form, setForm] = useState<Form | null>(null);
  const [restored, setRestored] = useState<Saved | null>(null);
  const [savedAt, setSavedAt] = useState<string>('');
  const [splitChecked, setSplitChecked] = useState(editing);
  const [busy, setBusy] = useState(false);
  const [uploading, setUploading] = useState<string | null>(null);
  const [tried, setTried] = useState(false);
  const [moreOpen, setMoreOpen] = useState(false);
  const base = useRef<string>('');

  // 처음 채우기: 고치기 → 서버 곡, 발매 곡 → 곡 정보, 그 밖 → 빈 칸. 이 브라우저에 쓰다 만 내용이 있으면 이어 쓰기를 묻는다
  useEffect(() => {
    if (form) return;
    if (editing && !existing) return;
    if (releaseId && !release) return;
    let start: Form = EMPTY;
    if (existing) {
      const { title, altTitles, workType, language, genre, description, visibility, iswc, hasPriorPublisher, priorPublisher, contributors, attachments } = existing;
      start = { title, altTitles, workType, language, genre, description, visibility, iswc, hasPriorPublisher, priorPublisher, contributors, attachments };
    } else if (track && release) {
      const d = draftTrackOf(track, release.draft);
      const people = new Map<string, WorkContributor>();
      const add = (names: string[], role: WorkContributor['roles'][number]) => {
        for (const n of names) {
          const cur = people.get(n);
          if (cur) { if (!cur.roles.includes(role)) cur.roles.push(role); continue; }
          // 크레딧은 가져오되 지분은 0 — 사용자가 직접 나눠야 한다 (기획안 4.2)
          people.set(n, { id: uid(), creatorId: null, name: n, roles: [role], society: null, ipi: null, share: 0, publisherShare: 0, confirm: 'pending', external: true, isMe: n === release.artist });
        }
      };
      add(splitNames(track.composers ?? d?.composers), 'composer');
      if (!d?.instrumental) add(splitNames(track.lyricists ?? d?.lyricists), 'lyricist');
      add(splitNames(d?.arrangers), 'arranger');
      start = {
        ...EMPTY, title: track.title, genre: release.draft?.genre ?? '', language: release.draft?.language ?? 'ko',
        workType: d?.instrumental ? 'instrumental' : 'original', contributors: [...people.values()].map(c => (c.isMe ? { ...c, confirm: 'confirmed', external: false } : c)),
      };
    }
    base.current = JSON.stringify(start);
    const saved = readJSON<Saved | null>(draftKey, null);
    if (saved && JSON.stringify(saved.form) !== base.current) setRestored(saved);
    setForm(start);
  }, [existing, release, track]); // eslint-disable-line react-hooks/exhaustive-deps

  // 자동 저장 — 손을 멈추고 0.8초 뒤 이 브라우저에
  useEffect(() => {
    if (!form || restored) return;
    if (JSON.stringify(form) === base.current) return;
    const t = window.setTimeout(() => {
      const at = stampNow();
      if (writeJSON(draftKey, { form, at, splitChecked } satisfies Saved)) setSavedAt(at);
    }, 800);
    return () => window.clearTimeout(t);
  }, [form, splitChecked]); // eslint-disable-line react-hooks/exhaustive-deps

  // 저장하지 않은 채 창을 닫으려 하면 브라우저가 한 번 묻는다
  const dirty = !!form && JSON.stringify(form) !== base.current;
  useEffect(() => {
    if (!dirty) return;
    const on = (e: BeforeUnloadEvent) => { e.preventDefault(); };
    window.addEventListener('beforeunload', on);
    return () => window.removeEventListener('beforeunload', on);
  }, [dirty]);

  const dupes = useMemo(() => (form && works ? similarWorks(form.title, works, id) : []), [form?.title, works, id]); // eslint-disable-line react-hooks/exhaustive-deps

  if (loadError) {
    return (
      <div className="view">
        <div className="ui-empty"><h2>곡을 불러오지 못했어요.</h2><p>{loadError}</p><Link to="/publishing?tab=songs" className="ui-btn is-soft">곡 목록으로</Link></div>
      </div>
    );
  }
  if (!form) return <div className="view"><SkeletonBlock height={120} /><SkeletonBlock height={320} /></div>;
  if (existing && existing.myRole !== 'owner' && existing.myRole !== 'editor') {
    return (
      <div className="view">
        <div className="ui-empty"><h2>이 곡을 고칠 권한이 없어요.</h2><p>공동 참여자는 곡 정보를 볼 수만 있어요. 지분이 다르면 곡 화면에서 변경을 요청해 주세요.</p><Link to={`/works/${existing.id}`} className="ui-btn is-soft">곡 보기</Link></div>
      </div>
    );
  }

  const set = <K extends keyof Form>(k: K, v: Form[K]) => setForm(f => (f ? { ...f, [k]: v } : f));
  const titleErr = tried && !form.title.trim() ? '곡 제목을 입력해 주세요.' : '';
  const iswcErr = form.iswc && !ISWC_RE.test(form.iswc) ? 'T-123.456.789-0 모양이에요. 모르면 비워 두세요 — 내부 번호로 관리해요.' : '';
  const issues = splitIssues(form.contributors);
  const fromTrack = !editing && !!track;
  const needsSplitCheck = fromTrack && !splitChecked && form.contributors.length > 0;
  const locked = existing && existing.application && ['submitted', 'in_review', 'approved'].includes(existing.application.status);

  const upload = async (kind: WorkAttachment['kind'], e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = '';
    if (!file) return;
    setUploading(kind);
    try {
      await api.uploadFile(file, kind === 'demo' ? 'AUDIO' : 'DOCUMENT');
      set('attachments', [...form.attachments, { id: uid(), kind, name: file.name, size: file.size, at: stampNow() }]);
    } catch (err) {
      toast(errorMessage(err, '파일을 올리지 못했어요.'), 'error');
    } finally { setUploading(null); }
  };

  const save = async (then?: 'apply') => {
    setTried(true);
    if (!form.title.trim()) { document.getElementById('workTitle')?.focus(); return; }
    if (needsSplitCheck) { document.getElementById('splitCheck')?.focus(); toast('가져온 크레딧의 지분을 확인해 주세요.', 'error'); return; }
    if (busy) return;
    setBusy(true);
    const clean: Form = { ...form, title: form.title.trim(), altTitles: form.altTitles.map(t => t.trim()).filter(Boolean) };
    try {
      let w: Work;
      if (existing) {
        w = await pubApi.updateWork(existing.id, clean, existing.rowVersion);
      } else {
        const recording: Omit<WorkRecording, 'id'> | null = track && release
          ? { releaseId: release.id, trackId: track.id, title: track.title, releaseTitle: release.title, version: track.version, isrc: track.isrc, status: release.status }
          : null;
        w = await pubApi.createWork({ ...clean, recording });
      }
      removeKey(draftKey);
      base.current = JSON.stringify(form);
      const ok = splitReady(form.contributors);
      toast(editing ? '곡 정보를 저장했어요.' : '곡을 만들었어요.', { tone: 'success', sub: ok ? undefined : '지분을 맞추면 저작권 관리를 신청할 수 있어요.' });
      nav(then === 'apply' && ok ? `/publishing/apply?work=${encodeURIComponent(w.id)}` : `/works/${w.id}`, { replace: true });
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

  return (
    <div className="view">
      <div className="spaced-actions">
        <button type="button" className="ui-btn is-white is-sm aq-back-link" onClick={() => void cancel()}><Glyph name="arrow-left" size={15} className="aq-glyph-lead" />{editing ? '곡으로' : '돌아가기'}</button>
      </div>
      <div className="ui-page-head">
        <div>
          <p className="eyebrow">퍼블리싱</p>
          <h1>{editing ? '곡 정보 고치기' : fromTrack ? '발매 곡으로 곡 정보 만들기' : '새 곡 등록'}</h1>
          <p>{fromTrack ? <>‘{release?.title}’의 곡 정보를 미리 채웠어요. 확인하고 고쳐 주세요.</> : '저작권 관리 계약 없이도 곡 정보와 지분을 정리해 둘 수 있어요.'}</p>
          <p className="muted small" aria-live="polite">{savedAt ? `이 브라우저에 자동 저장했어요 · ${localStamp(savedAt)}` : dirty ? '입력 중이에요' : ''}</p>
        </div>
      </div>

      {restored && (
        <Callout tone="info" action={
          <span className="row-actions">
            <Button size="sm" variant="white" onClick={() => { removeKey(draftKey); setRestored(null); }}>새로 쓰기</Button>
            <Button size="sm" variant="primary" onClick={() => { setForm(restored.form); setSplitChecked(restored.splitChecked); setSavedAt(restored.at); setRestored(null); }}>이어서 쓰기</Button>
          </span>
        }>
          <b>{localStamp(restored.at)}에 쓰던 내용이 있어요.</b> 저장하기 전에 화면을 떠났어요.
        </Callout>
      )}
      {locked && <Callout tone="warn">저작권 관리 심사 중이거나 관리 중인 곡이에요. 지금 고친 내용은 담당자가 확인한 뒤 반영돼요.</Callout>}

      <section className="ui-card" aria-labelledby="secBasic">
        <h2 id="secBasic">기본 정보</h2>
        <Field label="곡 제목" htmlFor="workTitle" required error={titleErr}>
          <input id="workTitle" className={inputClass('lg')} value={form.title} maxLength={200} onChange={e => set('title', e.target.value)} placeholder="예: 새벽 네 시" autoComplete="off" />
        </Field>
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
          <Field label="주요 언어" htmlFor="workLang">
            <select id="workLang" className={selectClass()} value={form.language} onChange={e => set('language', e.target.value)}>
              {LANGUAGES.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
            </select>
          </Field>
          <Field label="장르" htmlFor="workGenre">
            <input id="workGenre" className={inputClass()} value={form.genre} onChange={e => set('genre', e.target.value)} placeholder="예: Indie Pop" />
          </Field>
          <Field label="공개 범위" htmlFor="workVis" help={form.visibility === 'shared' ? '참여자와 담당자가 볼 수 있어요.' : '나와 작업 공간 멤버만 볼 수 있어요.'}>
            <select id="workVis" className={selectClass()} value={form.visibility} onChange={e => set('visibility', e.target.value as Form['visibility'])}>
              <option value="private">비공개</option>
              <option value="shared">참여자에게 공개</option>
            </select>
          </Field>
        </div>
        <Field label="곡 설명" htmlFor="workDesc" help="곡의 분위기·만든 배경 — A&R 제출 때도 다시 쓸 수 있어요.">
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
      </section>

      <section className="ui-card" aria-labelledby="secPeople">
        <h2 id="secPeople">참여자·지분</h2>
        <p className="ui-field-help">작사·작곡·편곡에 참여한 사람과 각자의 몫을 적어요. AUDENIQ 작가는 찾아서 추가하고, 없으면 이름만으로 외부 참여자를 추가해요.</p>
        <SplitEditor value={form.contributors} onChange={v => set('contributors', v)} />
        {fromTrack && form.contributors.length > 0 && (
          <label className={`ui-check${needsSplitCheck && tried ? ' aq-shake' : ''}`}>
            <input id="splitCheck" type="checkbox" checked={splitChecked} onChange={e => setSplitChecked(e.target.checked)} />
            <span><b>지분을 직접 확인했어요.</b> 크레딧은 발매 정보에서 가져왔지만, 저작권 지분은 자동으로 정하지 않아요.</span>
          </label>
        )}
      </section>

      <section className="ui-card" aria-labelledby="secRights">
        <h2 id="secRights">권리 정보</h2>
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
        <p className="ui-field-help">관리 단체 가입 여부와 IPI/CAE 번호는 참여자마다 위에서 적어요. 모르는 칸은 비워 둬도 저장돼요. 아는 관리 단체: {SOCIETIES.slice(0, 5).join(' · ')} 등</p>
      </section>

      <section className="ui-card" aria-labelledby="secFiles">
        <h2 id="secFiles">첨부 자료</h2>
        <div className="form-grid">
          {ATTACH_KINDS.map(([kind, label, accept]) => {
            const mine = form.attachments.filter(a => a.kind === kind);
            return (
              <div key={kind} className="ui-field">
                <label htmlFor={`attach-${kind}`}>{label}</label>
                <FilePicker id={`attach-${kind}`} accept={accept} busy={uploading === kind} onChange={e => void upload(kind, e)} />
                {mine.map(a => (
                  <small key={a.id} className="ui-field-help">
                    {a.name}{a.size ? ` · ${fileSize(a.size)}` : ''}{' '}
                    <button type="button" className="ui-btn is-text is-sm aq-danger-text" onClick={() => set('attachments', form.attachments.filter(x => x.id !== a.id))}>빼기</button>
                  </small>
                ))}
              </div>
            );
          })}
        </div>
      </section>

      <p className="ui-field-help" aria-live="polite">
        {issues.some(i => i.level === 'error')
          ? '지분을 맞추지 않아도 저장돼요. 저작권 관리 신청은 맞춘 뒤에 할 수 있어요.'
          : form.contributors.length ? '지분이 맞아요. 저장하고 바로 신청할 수 있어요.' : '참여자를 추가해 주세요.'}
      </p>
      <div className="ui-actions">
        <button type="button" className="ui-btn is-soft" onClick={() => void cancel()} disabled={busy}>나가기</button>
        <button type="button" className={`ui-btn ${!editing && splitReady(form.contributors) ? 'is-soft' : 'is-primary'}${busy ? ' is-busy' : ''}`} onClick={() => void save()} disabled={busy}>
          {busy ? '저장하는 중' : editing ? '저장' : '곡 저장'}
        </button>
        {!editing && splitReady(form.contributors) && (
          <button type="button" className="ui-btn is-primary" onClick={() => void save('apply')} disabled={busy}>저장하고 신청</button>
        )}
      </div>
    </div>
  );
}
