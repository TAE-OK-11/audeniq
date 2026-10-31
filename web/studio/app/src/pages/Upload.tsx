import { Suspense, useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { flushSync } from 'react-dom';
import { preorderChoices } from '../lib/preRelease';
import { useNavigate, useSearchParams } from '../lib/router';
import { useToast } from '../components/Toast';
import { SignaturePad, type SignaturePadHandle } from '../components/SignaturePad';
import { useConfirm } from '../components/Confirm';
import { useProgressFill } from '../hooks/useAnimations';
import { ApiError, MOCK, api, type DspAvailability } from '../api/client';
import { PartialSaveError, errorMessage } from '../api/errors';
import { useDocs } from '../store/docs';
import { RightsDocumentModal } from '../components/RightsDocumentModal';
import { GuardianConsentPanel } from '../components/GuardianConsent';
import { orgId } from '../api/http';
import { useProfanity } from '../lib/profanity';
import { DocumentModal } from '../components/DocumentModal';
import { type RightsDocumentContext, type RightsDocumentKind } from '../lib/rightsDocument';
import { rereleaseIssue } from '../lib/rerelease';
import { pushNotice } from '../store/support';
import { useProfile } from '../store/profile';
import { fileSize } from '../lib/format';
import { GENRES, KINDS, LANGUAGES, dspLabel, kindLabel } from '../lib/catalog';
import { formatKoreanDate, stampNow, todayStr } from '../lib/date';
import { fixCountWord, fixPath, isDocumentFix, resolveCorrection, splitCorrections, type ResolvedCorrection } from '../lib/corrections';
import { checkAudioFile, formatDuration, specLabel } from '../lib/audioSpec';
import { AGREEMENTS, SIGNER_ROLES, compactSignature, createApplication } from '../lib/application';
import { artistIssue, COVER_MAX, COVER_MIN, coverIssue, expectedReleaseType, isrcValid, minReleaseDate, mmssToSeconds, normalizeIsrc, profileLinkIssue, PROFILE_LINKS, releaseTypeIssue, rightsLineIssue, titleIssue, upcValid } from '../lib/dsp';
import { uid } from '../lib/store';
import { CheckIcon } from '../components/Check';
import { AddonSummary, addonProblem } from '../components/AddonOptions';
import { PublishingAlongside, applyAlongRelease, takePublishingIntent } from '../components/publishing/ReleasePublishing';
import { APPLICATION_AGREE } from '../lib/publishing';
import { pubApi } from '../api/publishing';
import { mockOrdersFromApplication } from '../lib/addons';
import { Glyph } from '../components/Glyph';
import { Badge, IconTile } from '../ui';
import { DateField } from '../components/Picker';
import { BrandMark } from '../components/BrandMark';
import { SplitsCard } from '../components/SplitsCard';
import { fetchArtistPages, type ArtistPages } from '../lib/artistPages';
import { parseTerritories, territorySummary } from '../lib/territories';
import { revealField } from '../lib/motion';
import { maskPreviewInput, previewLabel, previewSecs, settlePreviewInput } from '../lib/previewTime';
import { type CreditHistory } from '../lib/credits';
import { AUDIO_RE, DSP_COLOR, DSP_GROUPS, EMPTY, EMPTY_OPTIONS, MORE_INFO_FIELDS, RELEASE_TIMES, RIGHTS_CHECKS, type ReleaseOptions, STEPS, type SaveState, type Track, type UploadState, type WizardForm, applicableConditionals, clearFieldError, ensureReleaseDocuments, isContentId, makeCoverThumbnail, newTrack, previewOk, rightsOk, scaleCover, shiftDate, showFieldError, toPayload, uploadCoverPreview } from './upload/model';
import { BackIcon, KoreanDateField, breakAtDots } from './upload/fields';
import { lazyComponent } from '../lib/lazyComponent';
import { TrackEditor, UploadStatus } from './upload/tracks';
import { SkeletonBlock } from '../components/Skeleton';
import '../styles/split/Upload.css';

// 배급 설정 단계부터 쓰는 조각 — 신청서를 처음 열 때는 받지 않고, 한가할 때 미리 받아 둔다 (아래 Upload의 useEffect)
const loadOptions = () => import('./upload/options');
const OptionsSection = lazyComponent(loadOptions, m => m.OptionsSection);
const TerritoryPicker = lazyComponent(loadOptions, m => m.TerritoryPicker);
const OptionDocsBanner = lazyComponent(loadOptions, m => m.OptionDocsBanner);
const FinalReviewBanner = lazyComponent(loadOptions, m => m.FinalReviewBanner);
/** 그 조각을 받는 동안 그 자리만 비워 둔다 — 단계 전체가 자리표시로 바뀌지 않게 */
const Later = ({ children, h = 0 }: { children: ReactNode; h?: number }) => (
  <Suspense fallback={h ? <SkeletonBlock height={h} /> : null}>{children}</Suspense>
);



export function Upload() {
  const nav = useNavigate();
  const toast = useToast();
  const confirm = useConfirm();
  const profile = useProfile();
  const allDocs = useDocs();
  const [electronic, setElectronic] = useState<{ kind: RightsDocumentKind; context: RightsDocumentContext } | null>(null);
  const [openDocId, setOpenDocId] = useState<string | null>(null);
  const openDoc = allDocs.find(d => d.id === openDocId);
  const [searchParams] = useSearchParams();
  const editId = searchParams.get('edit');
  const fixCode = searchParams.get('fix');
  const fixTrack = searchParams.get('track');
  const [step, setStep] = useState(0);
  // 보완 요청 패널 — 단계마다 위에 크게 떠 있으니 한 줄로 접어 둘 수 있게
  const [fixOpen, setFixOpen] = useState(true);
  // 처음 연 단계에서는 펼쳐 보여 주고, 다른 단계로 넘어가면 자동으로 접는다(접힌 상태에선 이 단계 항목만 보임)
  const fixFirstStep = useRef<number | null>(null);
  useEffect(() => {
    if (fixFirstStep.current === null) { fixFirstStep.current = step; return; }
    if (step !== fixFirstStep.current) setFixOpen(false);
  }, [step]);
  // 지금까지 열어 본 가장 먼 단계 (단계 목록에서 바로 이동 가능)
  const [reached, setReached] = useState(0);
  // 보완 요청 — 수정 모드에서 불러온 발매의 요청 항목과 신청서 위치
  const [fixes, setFixes] = useState<ResolvedCorrection[]>([]);
  // 담당자 전체 의견 — 고칠 항목과 따로 보여 준다
  const [reviewNote, setReviewNote] = useState('');
  const [focusField, setFocusField] = useState<{ id: string; n: number } | null>(null);
  // 신청인 서명 (마지막 단계)
  const signRef = useRef<SignaturePadHandle>(null);
  const [signed, setSigned] = useState(false);
  const [signer, setSigner] = useState({ name: '', role: SIGNER_ROLES[0], touched: false });
  const [agreed, setAgreed] = useState<Record<string, boolean>>({});
  // 발매 신청서 서명 한 번으로 저작권 관리 신청서도 함께 (켜면 그 확인 항목도 함께 체크)
  const [pubAlong, setPubAlong] = useState(false);
  const [pubAgreed, setPubAgreed] = useState<string[]>([]);
  const [form, setForm] = useState<WizardForm>(() => ({ ...EMPTY, artist: profile.name || '', tracks: [newTrack()] }));
  // 예전 발매에 쓴 크레딧 이름 — 곡 정보 칸 아래 제안 칩 (못 불러오면 칩만 없다)
  const [creditNames, setCreditNames] = useState<CreditHistory | undefined>();
  useEffect(() => { api.creditHistory().then(setCreditNames, () => undefined); }, []);
  const [dsps, setDsps] = useState<DspAvailability[] | null>(null);
  const [dspError, setDspError] = useState('');
  useEffect(() => {
    let alive = true;
    api.listDsps().then(items => {
      if (!alive) return;
      setDsps(items);
      setDspError('');
      // 플랫폼을 고르지 않은 발매(새 발매·대량 등록한 임시 저장)는 이용 가능한 플랫폼 전체가 기본값 (Content ID는 따로 켠다)
      setForm(f => f.platforms.length ? f : ({ ...f, platforms: items.filter(d => d.available && !isContentId(d.slug)).map(d => d.slug) }));
    }).catch(e => { if (alive) { setDsps(null); setDspError(errorMessage(e, '플랫폼 상태를 확인하지 못했어요.')); } });
    return () => { alive = false; };
  }, [editId]);
  const [error, setError] = useState('');
  // 접수 요청 자체가 실패했을 때 (입력 오류와 따로, 다시 시도 버튼과 함께 보여 준다)
  const [submitError, setSubmitError] = useState('');
  const [submitting, setSubmitting] = useState(false);
  // 접수 직전 서버가 올린 파일을 마무리하는 동안
  const [finishing, setFinishing] = useState(false);
  // 중복 제출 방지용 ref (비동기 경계에서도 동작)
  const submittingRef = useRef(false);
  const [loadingEdit, setLoadingEdit] = useState(!!editId);
  const [origStatus, setOrigStatus] = useState<string>('draft');
  // 이미 신청서를 낸 발매를 보완·수정해 다시 접수할 때는 신청서(서명)를 새로 받지 않는다 — 신규 발매만 신청서를 쓴다
  const [hasApplication, setHasApplication] = useState(false);
  const [origDate, setOrigDate] = useState('');
  const [expandedTracks, setExpandedTracks] = useState<Set<string>>(new Set());
  // 레이블 표기·앨범 소개는 선택 항목이라 접어 둔다 — 첫 단계에 칸이 여덟 개라 무엇이 꼭 필요한지 흐렸다
  const [moreInfo, setMoreInfo] = useState(false);
  const [typeNudge, setTypeNudge] = useState(false);
  // 트랙 단계: 곡이 여럿이면 하나만 펼친다 (null = 다 접음)
  const [openTrack, setOpenTrack] = useState<string | null | undefined>(undefined);
  const [save, setSave] = useState<SaveState>({ kind: 'idle' });
  const [dragOver, setDragOver] = useState(false);
  // 업로드 진행 상태 — 키: 트랙 ID 또는 'cover'
  const [uploads, setUploads] = useState<Record<string, UploadState>>({});
  const uploadAborts = useRef(new Map<string, AbortController>());
  // 서버가 마무리 중인 업로드 — 접수할 때 이것만 기다린다
  const uploadPending = useRef(new Map<string, Promise<string | null>>());
  const setUpload = useCallback((key: string, u: UploadState | null) => {
    setUploads(prev => {
      const next = { ...prev };
      if (u) next[key] = u; else delete next[key];
      return next;
    });
  }, []);
  // 파일을 고르면 바로 저장소에 올린다 (실서버: R2 서명 URL로 직접, 체험: 진행률만)
  const startUpload = useCallback(async (key: string, file: File, kind: 'AUDIO' | 'IMAGE' | 'DOCUMENT'): Promise<string | null> => {
    uploadAborts.current.get(key)?.abort();
    const ctrl = new AbortController();
    uploadAborts.current.set(key, ctrl);
    setUpload(key, { pct: 0, state: 'uploading' });
    const run = (async () => {
      try {
        const r = await api.uploadFile(file, kind, pct => setUpload(key, { pct, state: 'uploading' }), ctrl.signal, () => {
          // 저장소에 다 올라갔다 — 서버의 마무리 확인을 기다리지 않고 '올렸어요'로 두고 다음으로 넘어갈 수 있게
          if (ctrl.signal.aborted) return;
          setUpload(key, { pct: 1, state: 'received' });
          setError(prev => (prev.includes('올리는 중이에요') ? '' : prev));
        });
        if (ctrl.signal.aborted) return null;
        setUpload(key, { pct: 1, state: 'done' });
        // '올리는 중' 안내가 떠 있었다면 끝났으니 지운다
        setError(prev => (prev.includes('올리는 중이에요') ? '' : prev));
        return r.assetId;
      } catch (e) {
        if (ctrl.signal.aborted) return null;
        setUpload(key, { pct: 0, state: 'error', message: errorMessage(e, '파일을 올리지 못했어요. 다시 선택해 주세요.') });
        return null;
      } finally {
        if (uploadAborts.current.get(key) === ctrl) uploadAborts.current.delete(key);
      }
    })();
    uploadPending.current.set(key, run);
    void run.finally(() => { if (uploadPending.current.get(key) === run) uploadPending.current.delete(key); });
    return run;
  }, [setUpload]);
  const attachOptionDocument = useCallback(async (name: keyof ReleaseOptions, asset: keyof ReleaseOptions, file: File) => {
    if (!/\.(pdf|png|jpe?g)$/i.test(file.name) || file.size > 20 * 1024 * 1024) {
      setError('서류는 20MB 이하 PDF, JPG, PNG 파일로 올려 주세요.');
      return;
    }
    const assetId = await startUpload(name, file, 'DOCUMENT');
    if (!assetId) return;
    dirtyRef.current = true;
    setForm(f => ({ ...f, options: { ...f.options, [name]: file.name, [asset]: assetId } }));
  }, [startUpload]);
  useEffect(() => {
    const aborts = uploadAborts.current;
    return () => { aborts.forEach(c => c.abort()); aborts.clear(); };
  }, []);
  const [coverWarn, setCoverWarn] = useState('');
  // 최신 form/step/draftId를 비동기 콜백에서 참조하기 위한 ref (stale closure 방지)
  const formRef = useRef(form);
  formRef.current = form;
  const stepRef = useRef(step);
  stepRef.current = step;
  const draftIdRef = useRef<string | null>(editId);
  const dirtyRef = useRef(false);
  const saveChain = useRef<Promise<unknown>>(Promise.resolve());
  const coverInputRef = useRef<HTMLInputElement>(null);
  const progressRef = useProgressFill(step);

  // 이미 접수된 발매를 수정할 때는 자동 저장하지 않는다 (확인 없이 접수본이 바뀌는 것 방지)
  const canAutoSave = !editId || origStatus === 'draft';

  const toggleTrackExpand = useCallback((id: string) => {
    setExpandedTracks(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  }, []);

  const deleteTrack = useCallback(async (id: string) => {
    const t = formRef.current.tracks.find(x => x.id === id);
    const filled = t && (t.title || t.audioName || t.composers);
    if (filled && !(await confirm({ title: '트랙을 삭제할까요?', message: `‘${t.title || '제목 없는 트랙'}’의 입력 내용이 사라져요.`, confirmLabel: '삭제', danger: true }))) return;
    dirtyRef.current = true;
    uploadAborts.current.get(id)?.abort();
    setUpload(id, null);
    setForm(f => (f.tracks.length > 1 ? {
      ...f,
      tracks: f.tracks.filter(x => x.id !== id),
      options: { ...f.options, coverTracks: f.options.coverTracks.filter(c => c.trackId !== id) },
    } : f));
  }, [confirm, setUpload]);

  // 수정/이어쓰기 모드: 기존 발매 데이터를 불러와 폼에 채움
  useEffect(() => {
    if (!editId) return;
    let cancelled = false;
    api.getRelease(editId).then(rel => {
      if (cancelled) return;
      const d = rel.draft;
      setOrigStatus(rel.status);
      setHasApplication(rel.status !== 'draft' && !!rel.draft?.application);
      setOrigDate(rel.release_date || '');
      const tracks: Track[] = d?.draftTracks?.length
        ? d.draftTracks.map(t => ({
            ...newTrack(), ...t, id: t.id || uid('t'), assetId: t.assetId || '', serverId: t.serverId || '',
            previewStart: previewLabel(t.previewStart),
            featuring: t.featuring || '', instrumental: !!t.instrumental, audioSpec: t.audioSpec || '',
          }))
        : rel.tracks.length
          ? rel.tracks.map(t => ({
              ...newTrack(),
              id: t.id, title: t.title || '', isrc: t.isrc || '',
              version: t.version || '', composers: t.composers || '',
              lyricists: t.lyricists || '', audioName: t.audioName || '',
              explicit: !!t.explicit, assetId: t.assetId || '', serverId: MOCK ? '' : t.id,
              duration: t.duration_ms ? `${String(Math.floor(t.duration_ms / 60000)).padStart(2, '0')}:${String(Math.floor((t.duration_ms % 60000) / 1000)).padStart(2, '0')}` : '',
            }))
          : [newTrack()];
      setForm(f => ({
        ...f,
        artist: d?.artist || rel.artist || '',
        title: rel.title || '',
        type: d?.type || 'single',
        language: d?.language || 'ko',
        genre: d?.genre && !GENRES.some(g => g[0] === d.genre) ? '__other__' : d?.genre || '',
        genreCustom: d?.genreCustom || (d?.genre && !GENRES.some(g => g[0] === d.genre) ? d.genre : ''),
        label: d?.label || '',
        notes: d?.notes || '',
        coverName: d?.coverName || '',
        coverData: d?.coverData || '',
        coverAssetId: d?.coverAssetId || '',
        coverThumbAssetId: d?.coverThumbAssetId || '',
        coverPreviewAssetId: d?.coverPreviewAssetId || '',
        releaseDate: rel.release_date || '',
        releaseTime: d?.release_time || '',
        preorderStart: d?.preorder_start || '',
        originalDate: d?.options?.rerelease && d.options.previousReleaseDate ? d.options.previousReleaseDate : d?.originalDate || '',
        upc: d?.upc || '',
        territories: d?.territories ?? ['WORLD'],
        platforms: d?.platforms?.length ? d.platforms : f.platforms,
        ownership: d?.ownership || '',
        phonogram: d?.phonogram || '',
        copyright: d?.copyright || '',
        rightsChecks: d?.rightsChecks || {},
        options: d?.options ? { ...EMPTY_OPTIONS, ...d.options } : { ...EMPTY_OPTIONS },
        artistProfile: d?.artistProfile ? { ...EMPTY.artistProfile, ...d.artistProfile } : { ...EMPTY.artistProfile },
        tracks,
      }));
      // 작성 중인 발매는 마지막으로 머문 단계에서 이어서 작성, 접수된 발매는 모든 단계를 바로 열 수 있게
      const last = Math.min(STEPS.length - 1, Math.max(0, d?.lastStep ?? 0));
      setReached(rel.status === 'draft' ? last : STEPS.length - 1);
      const split = splitCorrections(rel.corrections);
      const resolved = split.items.map(c => resolveCorrection(c, tracks.map(t => t.serverId || t.id)));
      setFixes(resolved);
      setReviewNote(split.note);
      // 보완하기로 들어왔으면 요청 항목의 단계·입력칸으로 바로 이동
      const target = fixCode
        ? resolved.find(c => c.code === fixCode && (!fixTrack || c.trackId === fixTrack)) ?? resolved[0]
        : undefined;
      if (target) {
        setStep(target.step);
        if (target.field) setFocusField({ id: target.field, n: Date.now() });
      } else if (rel.status === 'draft' && d?.lastStep) {
        setStep(last);
      }
      setLoadingEdit(false);
    }).catch(e => {
      if (cancelled) return;
      setLoadingEdit(false);
      toast(errorMessage(e, '발매 정보를 불러오지 못했어요.'), 'error');
      nav('/releases', { replace: true });
    });
    return () => { cancelled = true; };
  }, [editId, nav, toast]); // eslint-disable-line react-hooks/exhaustive-deps -- fix 파라미터는 처음 불러올 때만 쓴다

  // 배급 설정·권리 확인 조각은 첫 단계를 그린 뒤 한가할 때 받아 둔다 — 그 단계에 가면 바로 보인다
  useEffect(() => {
    const go = () => { void loadOptions(); };
    if ('requestIdleCallback' in window) { const id = window.requestIdleCallback(go, { timeout: 2500 }); return () => window.cancelIdleCallback(id); }
    const t = setTimeout(go, 1500);
    return () => clearTimeout(t);
  }, []);

  // 위자드에서는 헤더 숨김 + 레이아웃 패딩 제거 (그리기 전에 — 머리글이 한 번 보였다 사라지지 않게)
  useLayoutEffect(() => {
    document.body.classList.add('wizard-mode');
    return () => document.body.classList.remove('wizard-mode');
  }, []);

  // 저장되지 않은 입력이 있거나 파일을 올리는 중이면 창 닫기 전에 경고 — 자동 저장이 끝나면 입력 표시는 꺼지지만
  // 그때 음원·커버가 아직 올라가는 중일 수 있다(제목을 고친 뒤 자동 저장 → 업로드 도중 탭을 닫으면 경고 없이 끊겼다)
  useEffect(() => {
    const onBeforeUnload = (e: BeforeUnloadEvent) => {
      if (submittingRef.current || (!dirtyRef.current && !uploadPending.current.size)) return;
      e.preventDefault();
    };
    window.addEventListener('beforeunload', onBeforeUnload);
    return () => window.removeEventListener('beforeunload', onBeforeUnload);
  }, []);

  const set = useCallback(<K extends keyof WizardForm>(key: K, value: WizardForm[K]) => {
    dirtyRef.current = true;
    setForm(f => ({ ...f, [key]: value }));
  }, []);

  // 같은 아티스트로 이전에 기록한 플랫폼 페이지를 불러와 채운다 (0122)
  const [knownPages, setKnownPages] = useState<ArtistPages | null>(null);
  const loadKnownPages = useCallback(async (name: string) => {
    const known = await fetchArtistPages(name);
    setKnownPages(known);
    const linked = Object.entries(known.pages).filter(([, p]) => p.status === 'LINKED' && p.url);
    const pending = Object.values(known.pages).some(p => p.status === 'PENDING');
    if (!linked.length && !pending) return;
    setForm(f => {
      const ap = f.artistProfile;
      const empty = !ap.spotify.trim() && !ap.apple.trim() && !ap.melon.trim();
      if (!empty) return f;
      const next = { ...ap, isNew: false };
      for (const [k, p] of linked) if (k === 'spotify' || k === 'apple' || k === 'melon') next[k] = p.url ?? '';
      return { ...f, artistProfile: next };
    });
  }, []);
  // 이어서 작성하는 임시 저장 발매도 한 번 불러온다
  const knownLoaded = useRef(false);
  useEffect(() => {
    if (knownLoaded.current || !form.artist.trim() || !draftIdRef.current) return;
    knownLoaded.current = true;
    void loadKnownPages(form.artist);
  }, [form.artist, loadKnownPages]);

  const setTrack = useCallback((id: string, key: keyof Track, value: string | boolean | number) => {
    dirtyRef.current = true;
    setForm(f => ({ ...f, tracks: f.tracks.map(t => (t.id === id ? { ...t, [key]: value } : t)) }));
  }, []);

  // 욕설 확인 — 앨범 제목과 곡마다 (제목·버전·가사). 걸리면 그 곡(앨범 제목이면 모든 곡)을 19세 이용가로
  const profane = useProfanity([form.title, ...form.tracks.map(t => [t.title, t.version, t.instrumental ? '' : t.lyrics].join('\n'))]);
  const releaseProfane = profane[0] ?? false;
  const forcedFor = (index: number): 'track' | 'release' | null =>
    releaseProfane ? 'release' : profane[index + 1] ? 'track' : null;
  // 자동으로 켠 곡만 기억해 두었다가, 욕설을 고치면 다시 끈다 (아티스트가 직접 켠 것은 그대로)
  const autoExplicit = useRef(new Set<string>());
  useEffect(() => {
    const on = form.tracks.filter((t, i) => !t.explicit && (releaseProfane || profane[i + 1])).map(t => t.id);
    const off = form.tracks.filter((t, i) => t.explicit && autoExplicit.current.has(t.id) && !releaseProfane && !profane[i + 1]).map(t => t.id);
    if (!on.length && !off.length) return;
    on.forEach(id => autoExplicit.current.add(id));
    off.forEach(id => autoExplicit.current.delete(id));
    dirtyRef.current = true;
    setForm(f => ({ ...f, tracks: f.tracks.map(t => (on.includes(t.id) ? { ...t, explicit: true } : off.includes(t.id) ? { ...t, explicit: false } : t)) }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [profane.join(','), releaseProfane]);

  // 서버가 새로 만든 트랙 ID를 반영해야 다음 저장 때 트랙이 중복 생성되지 않는다
  const adoptTrackIds = useCallback((ids: Record<string, string> | undefined) => {
    if (!ids || !Object.keys(ids).length) return;
    setForm(f => {
      let changed = false;
      const tracks = f.tracks.map(t => {
        const sid = ids[t.id];
        if (sid && sid !== t.serverId) { changed = true; return { ...t, serverId: sid }; }
        return t;
      });
      return changed ? { ...f, tracks } : f;
    });
  }, []);
  // 저장이 중간에 실패해도 이미 반영된 발매·트랙은 다음 저장이 이어서 쓴다 (새 발매를 또 만들지 않음)
  const adoptPartialSave = useCallback((e: unknown) => {
    if (!(e instanceof PartialSaveError)) return;
    draftIdRef.current = e.releaseId;
    adoptTrackIds(e.trackServerIds);
  }, [adoptTrackIds]);

  // 자동 임시 저장 — 저장 요청을 직렬화해 draft가 두 개 생기지 않도록 한다
  // 저장에 성공했으면(또는 저장할 것이 없으면) true
  const autoSave = useCallback((): Promise<boolean> => {
    if (!canAutoSave) return Promise.resolve(true);
    const f = formRef.current;
    if (!f.title.trim() && !f.artist.trim()) return Promise.resolve(true); // 빈 폼은 저장하지 않음
    const run = async (): Promise<boolean> => {
      setSave({ kind: 'saving' });
      // 저장 중에 더 입력한 내용은 아직 저장되지 않았으므로 그대로 '변경 있음'으로 둔다
      const sent = formRef.current;
      try {
        const r = await api.saveDraft(draftIdRef.current, toPayload(sent, stepRef.current));
        draftIdRef.current = r.id;
        adoptTrackIds(r.trackServerIds);
        if (formRef.current === sent) dirtyRef.current = false;
        setSave({ kind: 'saved', at: stampNow().slice(11) });
        return true;
      } catch (e) {
        adoptPartialSave(e);
        setSave({ kind: 'error', message: e instanceof PartialSaveError ? e.message : undefined });
        return false;
      }
    };
    const p = saveChain.current.then(run, run);
    saveChain.current = p;
    return p;
  }, [canAutoSave, adoptTrackIds, adoptPartialSave]);

  const [guardianOpen, setGuardianOpen] = useState(false);
  /** 법정대리인 동의 — 동의서에 들어갈 발매와 법정대리인이 서버에 저장된 뒤 열어요 */
  const openGuardian = async () => {
    await autoSave();
    const f = formRef.current;
    if (!draftIdRef.current || !f.title.trim() || !f.artist.trim()) {
      toast('발매명과 아티스트를 입력하고 임시 저장한 뒤 동의를 받아 주세요.'); return;
    }
    if (!f.options.guardian.trim() || !f.options.guardianRelation) {
      toast('법정대리인 성명과 관계를 먼저 입력해 주세요.'); return;
    }
    setGuardianOpen(true);
  };

  const openElectronic = async (kind: RightsDocumentKind) => {
    await autoSave();
    const f = formRef.current;
    if (!draftIdRef.current || !f.title.trim() || !f.artist.trim()) {
      toast('발매명과 아티스트를 입력하고 임시 저장한 뒤 전자 문서를 작성해 주세요.'); return;
    }
    const tracks = kind === 'composition' && f.options.cover
      ? f.tracks.filter(t => f.options.coverTracks.some(c => c.trackId === t.id)) : f.tracks;
    const source = kind === 'composition' && f.options.cover
      ? f.options.coverTracks.map(c => `${f.tracks.find(t => t.id === c.trackId)?.title || '커버 트랙'} / 원곡: ${c.originalTitle} / 원 아티스트: ${c.originalArtist}${c.originalWriters ? ` / 작사·작곡: ${c.originalWriters}` : ''}`).join('\n')
      : kind === 'sample' ? '' : kind === 'performer' ? tracks.map(t => `${t.title} / 참여자: ${[t.featuring, t.performers].filter(Boolean).join(', ')}`).join('\n') : undefined;
    setElectronic({ kind, context: { releaseId: draftIdRef.current, title: f.title, artist: f.artist,
      tracks: tracks.map(t => ({ title: t.title, isrc: t.isrc })), territories: [...f.territories], platforms: [...f.platforms], source } });
  };

  // 화면을 떠날 때(뒤로 가기·메뉴 이동 등) 아직 저장 안 된 입력을 마지막으로 저장
  const autoSaveRef = useRef(autoSave);
  autoSaveRef.current = autoSave;
  useEffect(() => () => {
    if (dirtyRef.current && !submittingRef.current) void autoSaveRef.current();
  }, []);

  // 입력이 멈추고 2초 뒤 조용히 자동 저장
  useEffect(() => {
    if (!dirtyRef.current || loadingEdit || !canAutoSave) return;
    const t = window.setTimeout(() => { void autoSave(); }, 2000);
    return () => window.clearTimeout(t);
  }, [form, autoSave, loadingEdit, canAutoSave]);

  const fail = (msg: string, sel: string | null): boolean => {
    // 접혀 있는 곡의 칸이면 먼저 펼친다 (접힌 곡은 입력칸이 그려지지 않는다)
    const tm = sel && /#(?:tr|trackFile)-(\d+)/.exec(sel);
    const tid = tm ? form.tracks[+tm[1]]?.id : undefined;
    if (tid && tid !== openTrack) flushSync(() => setOpenTrack(tid));
    // 고칠 칸이 있으면 안내를 그 칸 바로 아래에 두고(폼 맨 아래 한 곳에 두면 휴대폰에서는 붉은 칸만 보이고 이유는 화면 밖이었다),
    // 칸을 고치는 순간 안내도 함께 지운다. 칸이 없는 안내만 폼 아래에 띄운다
    const target = sel ? document.querySelector<HTMLElement>(sel) : null;
    if (target) { setError(''); showFieldError(target, msg); } else setError(msg);
    requestAnimationFrame(() => {
      if (sel) {
        const el = target?.isConnected ? target : document.querySelector<HTMLElement>(sel);
        if (el) {
          // 먼저 focus()하면 화면이 그 칸으로 순간이동해 부드러운 스크롤이 묻힌다 — 굴러간 뒤 커서를 두고 흔든다
          revealField(el);
          el.classList.remove('aq-invalid');
          void el.offsetWidth;
          el.classList.add('aq-invalid');
          // 고치면 바로 표시와 안내를 지운다 (체크한 뒤에도 붉게 남아 있지 않도록)
          const clear = () => { el.classList.remove('aq-invalid'); clearFieldError(); };
          el.addEventListener('change', clear, { once: true });
          el.addEventListener('input', clear, { once: true });
          return;
        }
      }
      document.getElementById('wizardError')?.scrollIntoView({ behavior: 'smooth', block: 'center' });
    });
    return false;
  };

  const validateStep = (i: number): boolean => {
    if (i === 0) {
      if (!form.artist.trim()) return fail('아티스트명을 입력해 주세요.', '#f-artist');
      if (!form.title.trim()) return fail('발매 제목을 입력해 주세요.', '#f-title');
      const genreVal = form.genre === '__other__' ? form.genreCustom : form.genre;
      if (!genreVal.trim()) return fail(form.genre === '__other__' ? '장르를 직접 입력해 주세요.' : '장르를 선택해 주세요.', form.genre === '__other__' ? '#f-genre-custom' : '#f-genre');
      const artistErr = artistIssue(form.artist);
      if (artistErr) return fail(artistErr, '#f-artist');
      const titleErr = titleIssue(form.title, 'release');
      if (titleErr) return fail(titleErr, '#f-title');
      const ap = form.artistProfile;
      if (!ap.isNew) {
        for (const l of PROFILE_LINKS) {
          const err = profileLinkIssue(l.key, ap[l.key]);
          if (err) return fail(err, `#f-link-${l.key}`);
        }
        // 첫 발매로 새 페이지가 만들어지는 중이면(PENDING) 링크 없이도 같은 아티스트로 묶인다
        const pendingFirst = !!knownPages && Object.values(knownPages.pages).some(p => p.status === 'PENDING');
        if (!pendingFirst && !PROFILE_LINKS.some(l => ap[l.key].trim())) {
          return fail('이미 발매한 적이 있다면 기존 아티스트 페이지 주소를 하나 이상 입력해 주세요. 다른 동명 아티스트 페이지로 잘못 올라가는 것을 막아요.', '#f-link-spotify');
        }
      }
    }
    if (i === 1) {
      for (let k = 0; k < form.tracks.length; k++) {
        const t = form.tracks[k];
        if (!t.title.trim()) return fail(`트랙 ${k + 1}의 곡 제목을 입력해 주세요.`, `#tr-${k}-title`);
        const tErr = titleIssue(t.title, 'track');
        if (tErr) return fail(`트랙 ${k + 1}: ${tErr}`, `#tr-${k}-title`);
        if (!t.composers.trim()) return fail(`트랙 ${k + 1}의 작곡자를 입력해 주세요.`, `#tr-${k}-composers`);
        if (!t.instrumental && !t.lyricists.trim()) return fail(`트랙 ${k + 1}의 작사자를 입력해 주세요. 가사 없는 곡이면 ‘연주곡’을 선택해 주세요.`, `#tr-${k}-lyricists`);
        if (!t.audioName) return fail(`트랙 ${k + 1}의 음원 파일을 선택해 주세요.`, `#trackFile-${k}`);
        const up = uploads[t.id];
        if (up?.state === 'uploading') return fail(`트랙 ${k + 1}의 음원을 올리는 중이에요. 업로드가 끝나면 다음으로 넘어갈 수 있어요.`, null);
        if (up?.state === 'error') return fail(`트랙 ${k + 1}의 음원 파일을 올리지 못했어요. 다시 선택해 주세요.`, `#trackFile-${k}`);
        if (!MOCK && !t.assetId && up?.state !== 'received') return fail(`트랙 ${k + 1}의 음원을 올린 기록이 없어요. 파일을 다시 선택해 주세요.`, `#trackFile-${k}`);
        const dupIsrc = t.isrc.trim() && form.tracks.findIndex(o => o.isrc.trim() && normalizeIsrc(o.isrc) === normalizeIsrc(t.isrc)) !== k;
        if (dupIsrc) {
          setExpandedTracks(prev => new Set(prev).add(t.id));
          return fail(`트랙 ${k + 1}의 ISRC가 다른 곡과 같아요. 곡마다 고유한 ISRC를 써야 해요.`, `#tr-${k}-isrc`);
        }
        if (t.isrc.trim() && !isrcValid(t.isrc)) {
          setExpandedTracks(prev => new Set(prev).add(t.id));
          return fail(`트랙 ${k + 1}의 ISRC 형식을 확인해 주세요. (예: KR-ABC-26-00001)`, `#tr-${k}-isrc`);
        }
      }
    }
    if (i === 1 && typeIssue) {
      // 돌아가서 바꾸라고만 하지 않고, 트랙 목록 아래의 '○○로 바꾸기' 버튼을 띄워 그 자리에서 바꾼다
      flushSync(() => setTypeNudge(true));
      return fail(`${kindLabel(suggestedType)}(으)로 바꾸면 바로 넘어갈 수 있어요.`, '#aqTypeFix');
    }
    if (i === 2) {
      if (!form.coverName) return fail('커버아트를 등록해 주세요.', '#coverFile');
      if (uploads.cover?.state === 'uploading') return fail('커버아트를 올리는 중이에요. 업로드가 끝나면 다음으로 넘어갈 수 있어요.', null);
      if (uploads.cover?.state === 'error') return fail('커버아트를 올리지 못했어요. 이미지를 다시 선택해 주세요.', '#coverFile');
      if (!MOCK && !form.coverAssetId && uploads.cover?.state !== 'received') return fail('커버아트를 올린 기록이 없어요. 이미지를 다시 선택해 주세요.', '#coverFile');
    }
    if (i === 3) {
      if (!form.releaseDate) return fail('발매일을 선택해 주세요.', '#f-releaseDate');
      // 수정 모드에서 기존 발매일을 그대로 두는 경우는 과거여도 허용
      if (form.releaseDate < minReleaseDate(form.options.express) && form.releaseDate !== origDate) {
        return fail(form.options.express
          ? '신속 발매도 플랫폼 납품에 최소 3일이 필요해요. 3일 뒤 이후 날짜를 선택해 주세요.'
          : '플랫폼 납품·검수에 2주가 필요해요. 오늘부터 14일 뒤 이후로 선택하거나, 급하면 아래 ‘신속 발매’(10,000원)를 선택해 주세요.', '#f-releaseDate');
      }
      if (form.preorderStart && (form.preorderStart >= form.releaseDate || form.preorderStart < shiftDate(form.releaseDate, -90))) {
        return fail('사전 공개 시작일은 발매일 1–90일 전으로 골라 주세요.', '#f-preorder');
      }
      const named = form.tracks.filter(t => t.title.trim());
      const badPreview = named.findIndex(t => t.previewStart.trim() && !previewOk(t));
      if (badPreview >= 0) {
        const t = named[badPreview];
        return fail(previewSecs(t.previewStart)
          ? `'${t.title}' 미리듣기 시작 지점이 곡 길이(${t.duration})보다 뒤예요. 곡 안의 시간으로 골라 주세요.`
          : `'${t.title}' 미리듣기 시작 지점을 확인해 주세요. 초는 59까지 쓸 수 있어요. (예: 1:05)`, `#f-preview-${badPreview}`);
      }
      const tp = parseTerritories(form.territories);
      if (!tp.world && !tp.included.length) return fail('배급할 지역을 하나 이상 골라 주세요. 모든 지역에 배급하려면 ‘전 세계 배급’을 켜 주세요.', '#aqTerritories');
      if (!dsps) return fail(dspError || '플랫폼 배급 가능 상태를 확인하는 중이에요.', '#aqPlatforms');
      if (!form.platforms.length) return fail('배급할 플랫폼을 하나 이상 선택해 주세요.', '#aqPlatforms');
      if (['coverLicenseFile', 'sampleLicenseFile', 'featuredConsentFile', 'sharedContractFile'].some(k => uploads[k]?.state === 'uploading')) return fail('첨부서류 업로드가 끝나면 계속할 수 있어요.', '#aqSpecialOptions');
      if (form.platforms.some(p => !dsps.some(d => d.slug === p && d.available))) return fail('지금은 배급할 수 없는 플랫폼을 골랐어요. 그 플랫폼을 빼 주세요.', '#aqPlatforms');
      if (form.upc.trim() && !upcValid(form.upc.trim())) return fail('UPC/EAN 번호가 올바르지 않아요. 숫자 12~13자리와 마지막 확인 숫자를 확인해 주세요.', '#f-upc');
      if (form.upc.trim() && !/^0?\d{12}$/.test(form.upc.trim())) return fail('UPC는 12자리(UPC-A)만 받을 수 있어요. 13자리 EAN은 0으로 시작하는 번호만 쓸 수 있어요.', '#f-upc');
      const o = form.options;
      if (o.express && !o.expressAck) return fail('신속 발매 안내를 확인해 주세요.', '#aqExpressAck');
      const addon = addonProblem(o, form.tracks);
      if (addon) return fail(addon[0], `#${addon[1]}`);
    }
    if (i === 4) {
      if (['coverLicenseFile', 'sampleLicenseFile', 'featuredConsentFile', 'sharedContractFile'].some(k => uploads[k]?.state === 'uploading')) return fail('첨부서류 업로드가 끝나면 계속할 수 있어요.', '#aqSpecialOptions');
      const o = form.options;
      if ((o.cover && o.coverLicenseFile && !o.coverLicenseAssetId)
        || (o.sample && o.sampleLicenseFile && !o.sampleLicenseAssetId)
        || (o.featured && o.featuredConsentFile && !o.featuredConsentAssetId)
        || (o.shared && o.sharedContractFile && !o.sharedContractAssetId)) {
        return fail('이전에 선택한 권리 서류의 원본을 다시 첨부해 주세요.', '#aqSpecialOptions');
      }
      if (o.minor) {
        if (!o.guardian.trim()) return fail('법정대리인 성명을 입력해 주세요.', '#aqGuardian');
        if (!o.guardianRelation) return fail('법정대리인과의 관계를 선택해 주세요.', '#aqGuardianRelation');
        if (!o.guardianContact.trim()) return fail('법정대리인의 연락처를 입력해 주세요.', '#aqGuardianContact');
        const g2 = [o.guardian2.trim(), o.guardian2Relation, o.guardian2Contact.trim()];
        if (g2.some(v => v)) {
          if (!o.guardian2.trim()) return fail('두 번째 법정대리인 성명을 입력해 주세요.', '#aqGuardian2');
          if (!o.guardian2Relation) return fail('두 번째 법정대리인과의 관계를 선택해 주세요.', '#aqGuardian2Relation');
          if (!o.guardian2Contact.trim()) return fail('두 번째 법정대리인의 연락처를 입력해 주세요.', '#aqGuardian2Contact');
        }
        if (!o.guardianConsentDone) return fail('법정대리인 동의를 받아 주세요.', null);
      }
      if (o.ai && !o.aiTool.trim()) return fail('AI를 어떻게 활용했는지 골라 주세요.', '#aqAiTool');
      if (form.platforms.some(p => p === 'youtube-cid' || p === 'D-27') && (!o.contentIdExclusiveRightsAck || !o.contentIdOriginalRecordingAck)) {
        return fail('YouTube Content ID의 독점 권리와 원본 녹음을 확인해 주세요. 해당하지 않으면 아래 ‘Content ID 사용 안 함’을 눌러 주세요.', '#aqContentIdRights');
      }
      if (o.cover) {
        const valid = o.coverTracks.filter(c => form.tracks.some(t => t.id === c.trackId));
        if (!valid.length) return fail('커버곡인 트랙을 하나 이상 선택해 주세요.', null);
        for (const c of valid) {
          const idx = form.tracks.findIndex(t => t.id === c.trackId);
          if (!c.originalTitle.trim()) return fail(`트랙 ${idx + 1}의 원곡 제목을 입력해 주세요.`, `#cover-orig-title-${idx}`);
          if (!c.originalArtist.trim()) return fail(`트랙 ${idx + 1}의 원곡 아티스트를 입력해 주세요.`, `#cover-orig-artist-${idx}`);
        }
        if (!o.coverRightsAck) return fail('커버곡 권리 확인을 체크해 주세요.', '#aqCoverRightsAck');
      }
      if (o.rerelease) {
        const issue = rereleaseIssue(o, form.tracks, form.originalDate, form.releaseDate);
        if (issue) return fail(issue.message, `#${issue.field}`);
      }
      if (!form.ownership.trim()) return fail('음원 권리자를 입력해 주세요.', '#f-ownership');
      if (!form.phonogram.trim()) return fail('℗ 표기를 입력해 주세요.', '#f-phonogram');
      if (!form.copyright.trim()) return fail('© 표기를 입력해 주세요.', '#f-copyright');
      const pErr = rightsLineIssue(form.phonogram);
      if (pErr) return fail(`℗ 표기: ${pErr}`, '#f-phonogram');
      const cErr = rightsLineIssue(form.copyright);
      if (cErr) return fail(`© 표기: ${cErr}`, '#f-copyright');
      if (!rightsOk(form)) return fail('권리 확인 항목을 모두 확인해 주세요.', null);
    }
    if (i === 5 && !resubmit) {
      if (!signerName.trim()) return fail('신청인 성명을 입력해 주세요.', '#f-signer');
      if (!signed || signRef.current?.isEmpty()) return fail('신청인 서명을 그려 주세요.', '#aqApplyPad');
      const missing = AGREEMENTS.find(a => !agreed[a.id]);
      if (missing) return fail('신청 확인 항목에 모두 동의해 주세요.', `#agree-${missing.id}`);
      const pubMissing = pubAlong && APPLICATION_AGREE.find(([k]) => !pubAgreed.includes(k));
      if (pubMissing) return fail('저작권 관리 신청 확인 항목에 모두 동의해 주세요.', `#pub-agree-${pubMissing[0]}`);
    }
    setError('');
    clearFieldError();
    document.querySelectorAll('#view-new .aq-invalid').forEach(el => el.classList.remove('aq-invalid'));
    return true;
  };

  // 보완 요청 패널 접기·펼치기 — 지원하는 브라우저에선 패널 높이와 아래 입력 영역이 부드럽게 따라 움직인다
  const toggleFix = () => {
    const doc = document as Document & { startViewTransition?: (cb: () => void) => unknown };
    if (!doc.startViewTransition || window.matchMedia('(prefers-reduced-motion: reduce)').matches) { setFixOpen(v => !v); return; }
    const root = document.documentElement;
    root.classList.add('aq-vt-fix');
    const vt = doc.startViewTransition(() => flushSync(() => setFixOpen(v => !v))) as { finished?: Promise<unknown> } | undefined;
    const done = () => root.classList.remove('aq-vt-fix');
    if (vt?.finished) vt.finished.then(done, done); else done();
  };

  // 수익 분배에 저장하지 않은 입력이 있으면 단계를 옮기기 전에 대신 저장한다 — 예전엔 '다음으로'를 누르면 조용히 사라졌다.
  // 입력이 잘못됐으면(이름·비율) 그 단계에 머물고 이유를 보여 준다
  const splitsSave = useRef<(() => Promise<boolean>) | null>(null);
  const registerSplitsSave = useCallback((fn: (() => Promise<boolean>) | null) => { splitsSave.current = fn; }, []);
  const flushSplits = async () => !splitsSave.current || splitsSave.current();

  const goStep = async (to: number) => {
    if (to !== step && !(await flushSplits())) return;
    setStep(to);
    setReached(r => Math.max(r, to));
    setError('');
    clearFieldError();
    setSubmitError('');
    // 부드러운 스크롤은 새 단계가 나타나는 동안 화면을 끌고 다녀 버벅여 보이므로 바로 맨 위로
    window.scrollTo({ top: 0, left: 0, behavior: 'instant' as ScrollBehavior });
  };

  const submit = async () => {
    // 앞 단계까지 모두 다시 검증 (단계를 건너뛰어 돌아온 경우 대비)
    for (let i = 0; i < STEPS.length; i++) {
      if (!validateStep(i)) { if (i !== step) goStep(i); return; }
    }
    if (submittingRef.current) return;
    submittingRef.current = true;
    setSubmitting(true);
    setSubmitError('');
    try {
      // 서버가 아직 마무리 중인 파일이 있으면 여기서만 기다린다 (단계 이동은 기다리지 않았다)
      if (uploadPending.current.size) {
        setFinishing(true);
        await Promise.all(uploadPending.current.values());
        setFinishing(false);
        // 마무리 중 실패한 파일이 있으면 그 단계로 — 자산 ID가 들어간 뒤 상태로 다시 확인한다
        await new Promise(r => requestAnimationFrame(r));
        const f0 = formRef.current;
        const badTrack = f0.tracks.findIndex(t => !MOCK && !t.assetId);
        if (badTrack >= 0 || (!MOCK && !f0.coverAssetId)) {
          const to = badTrack >= 0 ? 1 : 2;
          goStep(to);
          setError(badTrack >= 0 ? `트랙 ${badTrack + 1}의 음원을 서버가 받지 못했어요. 파일을 다시 선택해 주세요.` : '커버아트를 서버가 받지 못했어요. 이미지를 다시 선택해 주세요.');
          return;
        }
      }
      await saveChain.current.catch(() => {});
      // 기다리는 동안 자동 저장이 서버 트랙 id를 받아 왔을 수 있다 — 클로저의 form이 아니라 최신 상태로 보낸다(트랙 중복 방지)
      const f = formRef.current;
      const payload = toPayload(f, step);
      const signature = resubmit ? '' : await compactSignature(signRef.current?.toDataURL() ?? '');
      const application = resubmit ? undefined : await createApplication({
        payload, signature, signerName, signerRole: signer.role,
        agreements: AGREEMENTS.map(a => a.id),
      });
      const intentDraft = draftIdRef.current;
      const r = await api.submitRelease(draftIdRef.current, { ...payload, application });
      draftIdRef.current = r.id;
      // 신청서에서 고른 대로 곡 크레딧으로 저작물을 만들고(퍼블리싱을 고르면 신청 준비까지) — 실패해도 발매 접수는 그대로
      const intent = takePublishingIntent(intentDraft);
      const coverTitles = f.options.cover ? f.tracks.filter(t => f.options.coverTracks.some(c => c.trackId === t.id || c.trackId === t.serverId)).map(t => t.title) : [];
      if (pubAlong && !resubmit) {
        // 같은 서명으로 저작권 관리 신청서도 — 기다리지 않는다. 낸 곡·남은 곡은 알림(실서버는 토스트)으로 알려 준다
        const tell = (title: string, detail: string, tone: 'success' | 'info' | 'error') => {
          pushNotice({ id: uid('n'), kind: '발매', time: stampNow(), link: `/releases/${r.id}?tab=publishing`, title, detail });
          if (!MOCK) toast(title, tone);
        };
        void applyAlongRelease(r.id, coverTitles, { name: signerName.trim(), data: signature }).then(({ applied, waiting, identity }) => {
          if (!applied && !waiting) return;
          if (identity) {
            tell(`${r.title} 곡 정보를 정리해 두었어요.`, '본인인증을 마치면 앨범 퍼블리싱 탭에서 같은 곡들의 저작권 관리를 바로 신청할 수 있어요.', 'info');
            return;
          }
          tell(
            applied ? `저작권 관리 신청서도 ${applied}곡 함께 접수했어요.` : `${r.title} 곡 정보를 확인하고 저작권 관리를 신청해 주세요.`,
            waiting ? `${waiting}곡은 참여자·지분을 확인한 뒤 앨범 퍼블리싱 탭에서 신청할 수 있어요.` : '담당자 확인이 끝나면 계약서를 보내 드려요.',
            applied ? 'success' : 'info',
          );
        }).catch(() => tell(`${r.title} 저작권 관리 신청을 마치지 못했어요.`, '발매 신청은 접수됐어요. 앨범 퍼블리싱 탭에서 다시 신청해 주세요.', 'error'));
      } else if (intent !== 'none') {
        void pubApi.prepareWorksFromRelease(r.id, intent === 'enroll', coverTitles).catch(() => {});
      }
      dirtyRef.current = false;
      // 실서버는 계약서를 서버가 만든다 (알림·서류는 pushNotice가 새로 받게 한다)
      if (MOCK) { ensureReleaseDocuments(f, r.id); mockOrdersFromApplication(r.id, r.title, { ...f.options }, f.tracks.map(t => t.title)); }
      pushNotice({
        id: uid('n'), kind: '발매', time: stampNow(), link: `/releases/${r.id}`,
        title: editId && origStatus !== 'draft' ? `${r.title} 수정 내용을 보냈어요.` : `${r.title} 발매를 신청했어요.`,
        detail: '담당자 검토가 시작됐어요. 계약서와 권리 서류 메뉴에서 준비된 문서를 확인해 주세요.',
      });
      toast(editId && origStatus !== 'draft' ? '수정 내용을 보냈어요.' : '발매를 신청했어요.', 'success');
      // 서명한 신청서를 정식 서류로 바로 보여 준다 (보완 재접수는 발매 상세로)
      nav(resubmit ? `/releases/${r.id}` : `/releases/${r.id}/application?done=1`, { replace: true });
    } catch (e) {
      adoptPartialSave(e);
      const message = errorMessage(e, '서버와 연결이 원활하지 않아요.');
      if (e instanceof ApiError && e.code === 'DSP_UNAVAILABLE') {
        goStep(3);
        api.listDsps().then(setDsps).catch(err => { setDsps(null); setDspError(errorMessage(err)); });
        setError(message);
      } else {
        // 버튼 위를 가리는 알림 대신, 이유와 다시 시도 버튼을 서명 아래에 펼쳐 보여 준다
        setSubmitError(message);
        requestAnimationFrame(() => document.getElementById('wizardSubmitError')?.scrollIntoView({ behavior: 'smooth', block: 'center' }));
      }
    } finally {
      submittingRef.current = false;
      setSubmitting(false);
      setFinishing(false);
    }
  };

  const next = async () => {
    if (step === STEPS.length - 1) { await submit(); return; }
    if (!validateStep(step)) return;
    void autoSave();
    goStep(step + 1);
  };

  const leave = async () => {
    // 나가면 진행 중인 업로드는 취소된다(아래 정리 effect) — 임시 저장만 하고 조용히 나가면 파일이 빠진 줄 몰랐다
    if (uploadPending.current.size) {
      const ok = await confirm({ title: '파일을 올리는 중이에요', message: '지금 나가면 올리던 파일은 취소돼요. 다시 들어와서 파일을 다시 골라야 해요.', confirmLabel: '나가기', cancelLabel: '기다리기', danger: true });
      if (!ok) return;
    }
    if (dirtyRef.current && !canAutoSave) {
      const ok = await confirm({ title: '수정을 그만둘까요?', message: '저장하지 않은 변경 내용은 사라져요.', confirmLabel: '나가기', danger: true });
      if (!ok) return;
    } else if (dirtyRef.current) {
      if (!(await autoSave())) {
        const ok = await confirm({ title: '임시 저장하지 못했어요', message: '지금 나가면 마지막 저장 이후 입력한 내용이 사라져요. 그래도 나갈까요?', confirmLabel: '나가기', danger: true });
        if (!ok) return;
      } else if (draftIdRef.current) toast('작성 중인 내용을 임시 저장했어요.', 'info');
    }
    dirtyRef.current = false;
    nav(editId ? `/releases/${editId}` : '/');
  };

  const back = () => {
    if (step === 0) { void leave(); return; }
    goStep(step - 1);
  };

  const applyCover = async (file: File | undefined, input?: HTMLInputElement | null) => {
    if (!file) return;
    const okType = /^image\/(jpeg|png)$/i.test(file.type) || /\.(jpe?g|png)$/i.test(file.name);
    if (!okType) {
      toast('커버는 JPG 또는 PNG 파일만 등록할 수 있어요.');
      if (input) input.value = '';
      return;
    }
    if (file.size > 20 * 1024 * 1024) {
      toast('커버 이미지는 20MB 이하로 올려 주세요.');
      if (input) input.value = '';
      return;
    }
    try {
      const thumb = await makeCoverThumbnail(file);
      // 플랫폼 기준(정사각형, 3000~6000px)에 맞지 않으면 받지 않는다
      const issue = coverIssue(thumb.width, thumb.height);
      if (issue) {
        setCoverWarn(issue);
        toast('커버아트 규격을 확인해 주세요.');
        if (input) input.value = '';
        return;
      }
      setCoverWarn('');
      set('coverName', file.name);
      setForm(f => ({ ...f, coverData: thumb.data, coverAssetId: '', coverThumbAssetId: '', coverPreviewAssetId: '' }));
      const assetId = await startUpload('cover', file, 'IMAGE');
      if (assetId) {
        // 원본이 등록되면 바로 반영한다 — 미리보기 두 장까지 기다렸다 넣으면 그동안 '다음'을 눌렀을 때 '올린 기록이 없어요'가 떴다
        dirtyRef.current = true;
        setForm(f => (f.coverName === file.name ? { ...f, coverAssetId: assetId } : f));
        if (!MOCK) {
          // 화면에 보여 줄 미리보기도 서버에 보관한다 — 원본(3000px 이상, 수 MB)은 배급용으로 서버에만 두고 화면엔 쓰지 않는다.
          // 목록용 240px와 상세의 큰 커버용 1080px 두 가지 (실패하면 작은 미리보기로 보여 준다)
          void Promise.all([
            uploadCoverPreview(thumb.data).catch(() => ''),
            scaleCover(file, 1080, 0.86).then(p => uploadCoverPreview(p.data)).catch(() => ''),
          ]).then(([thumbId, previewId]) => {
            dirtyRef.current = true;
            setForm(f => (f.coverAssetId === assetId ? { ...f, coverThumbAssetId: thumbId, coverPreviewAssetId: previewId } : f));
          });
        }
      }
    } catch {
      toast('커버 이미지를 불러오지 못했어요.');
      if (input) input.value = '';
    }
  };

  const onTrackAudio = useCallback((id: string, e: React.ChangeEvent<HTMLInputElement>) => {
    const input = e.target;
    const file = input.files?.[0];
    if (!file) return;
    if (!AUDIO_RE.test(file.name) && !/^audio\/(x-)?(wav|wave|flac|mp4|m4a|aiff|wavpack|tta)$/i.test(file.type)) {
      toast('음원은 무손실 WAV·FLAC·ALAC·AIFF·WavPack·TTA 파일만 올릴 수 있어요.');
      input.value = '';
      return;
    }
    if (file.size > 512 * 1024 * 1024) {
      toast('음원 파일은 512MB 이하로 올려 주세요.');
      input.value = '';
      return;
    }
    void checkAudioFile(file).then(check => {
      if (check.error) {
        // 규격에 맞지 않으면 올리지 않고 사유를 파일 칸 아래에 보여 준다
        setUpload(id, { pct: 0, state: 'error', message: check.error });
        input.value = '';
        return;
      }
      if (check.warnings.length) toast(check.warnings[0], 'info');
      acceptAudio(id, file, check.spec ? specLabel(check.spec) : '', check.spec?.duration ? formatDuration(check.spec.duration) : '');
    }).catch(() => acceptAudio(id, file, '', ''));
  }, [startUpload, setUpload, toast]); // eslint-disable-line react-hooks/exhaustive-deps -- acceptAudio는 매 렌더 새로 만들어지지만 상태 setter만 쓴다

  const acceptAudio = (id: string, file: File, spec: string, headerDuration: string) => {
    dirtyRef.current = true;
    setForm(f => ({
      ...f,
      tracks: f.tracks.map(t => (t.id === id ? { ...t, audioName: file.name, audioSize: file.size, assetId: '', audioSpec: spec, duration: headerDuration || t.duration } : t)),
    }));
    void startUpload(id, file, 'AUDIO').then(assetId => {
      if (!assetId) return;
      setForm(f => ({ ...f, tracks: f.tracks.map(t => (t.id === id && t.audioName === file.name ? { ...t, assetId } : t)) }));
      dirtyRef.current = true;
      toast('음원 파일을 올렸어요.');
    });
    // 파일 머리에서 길이를 읽었으면 끝, 아니면 브라우저로 길이 추출 (못 읽어도 파일 등록은 진행)
    if (headerDuration) return;
    const commit = (duration: string) => {
      if (!duration) return;
      setForm(f => ({ ...f, tracks: f.tracks.map(t => (t.id === id ? { ...t, duration } : t)) }));
    };
    const url = URL.createObjectURL(file);
    const audio = new Audio();
    audio.preload = 'metadata';
    const done = (duration: string) => { URL.revokeObjectURL(url); audio.removeAttribute('src'); commit(duration); };
    audio.onloadedmetadata = () => {
      const secs = Math.round(Number.isFinite(audio.duration) ? audio.duration : 0);
      done(secs ? `${String(Math.floor(secs / 60)).padStart(2, '0')}:${String(secs % 60).padStart(2, '0')}` : '');
    };
    audio.onerror = () => done('');
    audio.src = url;
  };

  // 보완 항목으로 이동: 단계를 바꾸고 해당 입력칸을 강조
  const jumpToFix = (c: ResolvedCorrection) => {
    const m = c.field && /^tr-(\d+)-isrc$/.exec(c.field);
    if (m) {
      const t = form.tracks[+m[1]];
      if (t) setExpandedTracks(prev => new Set(prev).add(t.id));
    }
    const tm = c.field && /^(?:tr|trackFile)-(\d+)/.exec(c.field);
    if (tm && form.tracks[+tm[1]]) setOpenTrack(form.tracks[+tm[1]].id);
    if (c.field && MORE_INFO_FIELDS.includes(c.field)) setMoreInfo(true);
    if (c.step !== step) goStep(c.step);
    setFocusField(c.field ? { id: c.field, n: Date.now() } : null);
  };

  useEffect(() => {
    if (!focusField || loadingEdit) return;
    // 단계 전환 애니메이션이 자리 잡은 뒤 스크롤
    const timer = window.setTimeout(() => {
      const el = document.getElementById(focusField.id);
      if (!el) return;
      const box = el.closest<HTMLElement>('.field, .check-line, details, .aq-fix-zone, .aq-dropzone') ?? el;
      box.scrollIntoView({ behavior: 'smooth', block: 'center' });
      box.classList.remove('aq-fix-flash');
      void box.offsetWidth;
      box.classList.add('aq-fix-flash');
      if (el.matches('input:not([type=file]), select, textarea')) el.focus({ preventScroll: true });
    }, 380);
    return () => window.clearTimeout(timer);
  }, [focusField, step, loadingEdit]);

  // 발매일 기본값 — 배급 설정에 처음 오면 가장 빠른 날짜를 넣어 둔다(눌러서 바꾼다). 자동으로 넣은 날짜는
  // 신속 발매를 켜고 끌 때 새 기준의 가장 빠른 날짜로 따라 바뀌고, 직접 고른 날짜는 건드리지 않는다
  const autoDate = useRef('');
  const earliest = minReleaseDate(form.options.express);
  useEffect(() => {
    if (loadingEdit || step < 3) return;
    if (form.releaseDate && form.releaseDate !== autoDate.current) return;
    if (form.releaseDate === earliest) return;
    autoDate.current = earliest;
    set('releaseDate', earliest);
  }, [step, earliest, form.releaseDate, loadingEdit, set]);

  // 서명자 기본값: 권리자 → 아티스트 정보의 이름
  const signerName = signer.touched ? signer.name : (form.ownership || profile.name || '');
  const resubmit = !!editId && origStatus !== 'draft' && hasApplication;
  const s = STEPS[step];
  const showMoreInfo = moreInfo || !!form.label.trim() || !!form.notes.trim() || fixes.some(f => !!f.field && MORE_INFO_FIELDS.includes(f.field));
  // 곡 입력이 다 됐는지 — 접힌 곡 요약의 '입력 완료/필요' 표시
  const trackComplete = (t: Track) => !!(t.title.trim() && t.composers.trim() && (t.instrumental || t.lyricists.trim()) && t.audioName && (MOCK || t.assetId || uploads[t.id]?.state === 'received'));
  // 처음엔 아직 덜 채운 첫 곡(없으면 첫 곡)을 펼친다
  const shownTrack = openTrack === undefined
    ? (form.tracks.find(t => !trackComplete(t)) ?? form.tracks[0])?.id ?? null
    : openTrack;
  const nextLabel = finishing ? '파일 마무리 중' : submitting ? '접수하는 중' : step === STEPS.length - 1 ? (resubmit ? '다시 접수하기' : editId && origStatus !== 'draft' ? '서명하고 수정 완료' : '서명하고 접수하기') : '다음으로';
  const genreIsCustom = form.genre === '__other__';
  // 발매 유형이 곡 구성과 맞지 않을 때 — 모든 곡의 길이를 알게 되면(또는 넘어가려 하면) 바꾸기 버튼을 띄운다
  const trackSecs = form.tracks.map(t => mmssToSeconds(t.duration));
  const typeIssue = releaseTypeIssue(form.type, trackSecs);
  const suggestedType = expectedReleaseType(trackSecs);
  const showTypeFix = !!typeIssue && (typeNudge || trackSecs.every(Boolean));
  const availablePlatforms = dsps?.filter(d => d.available).map(d => d.slug) ?? [];
  // '모두 배급'은 Content ID를 뺀 음악 플랫폼 전체 — Content ID는 켜져 있든 아니든 따로 본다
  const basePlatforms = availablePlatforms.filter(p => !isContentId(p));
  const cidPlatforms = availablePlatforms.filter(isContentId);
  const cidOn = form.platforms.some(isContentId);
  const allPlatforms = basePlatforms.length > 0 && basePlatforms.every(p => form.platforms.includes(p)) && form.platforms.every(p => availablePlatforms.includes(p));
  // 기본은 ‘모두 배급’ 스위치만. 끄면 아래에 플랫폼별 선택이 열린다 (일부만 고른 발매는 처음부터 열림)
  const [pickPlatforms, setPickPlatforms] = useState(false);
  const showPlatforms = pickPlatforms || !allPlatforms;
  const unavailableDsps = dsps?.filter(d => !d.available && !form.platforms.includes(d.slug)) ?? [];

  // 플랫폼 이름 50여 개를 다 늘어놓으면 확인 화면이 휴대폰에서 3,000px 넘게 길어졌다 — 몇 곳만 보이고 개수로 줄인다
  const platformSummary = (() => {
    const base = form.platforms.filter(p => !isContentId(p));
    const cid = cidOn ? ' · YouTube Content ID 포함' : '';
    if (!form.platforms.length) return '선택하지 않음';
    if (allPlatforms) return `주요 음악 플랫폼 ${base.length}곳 전체${cid}`;
    const names = base.map(dspLabel);
    return `${names.slice(0, 5).join(', ')}${names.length > 5 ? ` 외 ${names.length - 5}곳` : ''}${cid}`;
  })();
  // 네 번째 값: 빠진 항목 — 다른 줄과 같은 회색이면 '등록되지 않음'이 눈에 띄지 않았다. 주황 글자 + '채우기'
  const reviewSections: [string, React.ReactNode, number, boolean][] = [
    ['발매 정보', `${form.title || '제목 없음'} · ${form.artist || '아티스트 없음'} · ${kindLabel(form.type)}`, 0, !form.title.trim() || !form.artist.trim()],
    ['트랙', form.tracks.length > 1
      // 어느 곡이 덜 채워졌는지 곡마다 — 목록 전체가 주황이면 몇 번 곡을 고쳐야 할지 몰랐다
      ? <ol className="aq-review-tracks">{form.tracks.map(t => <li key={t.id}>{t.title || '곡명 없음'}{!trackComplete(t) && <small className="aq-review-need"> · 입력 필요</small>}</li>)}</ol>
      : <>{form.tracks[0]?.title || '곡명 없음'}{form.tracks[0] && !trackComplete(form.tracks[0]) && <small className="aq-review-need"> · 입력 필요</small>}</>, 1, !form.tracks.length || !form.tracks.every(trackComplete)],
    ['커버아트', form.coverData
      ? <span className="aq-review-cover"><img src={form.coverData} alt="" width={56} height={56} />{form.coverName}</span>
      : form.coverName || '등록되지 않음', 2, !form.coverData && !form.coverName],
    ['발매일', form.releaseDate ? `${formatKoreanDate(form.releaseDate)}${form.releaseTime ? ` · 한국 시간 ${form.releaseTime} 동시 발매` : ' · 나라별 자정'}` : '지정하지 않음', 3, !form.releaseDate],
    ['사전 공개', form.preorderStart ? `${formatKoreanDate(form.preorderStart)}부터 (예약 판매·미리듣기)` : '없음', 3, false],
    ['배급 지역', territorySummary(form.territories, '선택하지 않음'), 3, !form.territories.length],
    ['배급 플랫폼', platformSummary, 3, !form.platforms.length],
    ['권리자', form.ownership || '입력 전', 4, !form.ownership.trim()],
    ['권리 확인', rightsOk(form) ? '필수 확인 완료' : '필수 확인 항목 누락', 4, !rightsOk(form)],
  ];

  // 상단 바 오른쪽 칸에 들어가도록 짧게 (긴 문구는 title 속성으로)
  const saveLabel = !canAutoSave ? '수정 중'
    : save.kind === 'saving' ? '저장 중'
    : save.kind === 'saved' ? `저장됨 ${save.at}`
    : save.kind === 'error' ? '저장 안 됨' : '';
  const saveTitle = !canAutoSave ? '수정 내용은 마지막 단계에서 ‘수정 완료’를 눌러야 반영돼요.'
    : save.kind === 'saved' ? `${save.at}에 자동으로 저장했어요.`
    : save.kind === 'error' ? (save.message || '자동으로 저장하지 못했어요. 인터넷 연결을 확인해 주세요.') : '';

  return (
    <section id="view-new" className="view">
    <div className="wizard">
      {/* 뒤로 가기 · 제목 · 진행 막대는 한 덩어리로 화면 위에 고정 — 스크롤해도, 단계가 바뀌어도 움직이지 않는다
          (움직이면 누르려던 자리에 다른 칸이 와서 엉뚱하게 이전 단계로 돌아가는 일이 있었다) */}
      <div className="aq-wiz-top">
        <div className="wizard-topbar" aria-label="발매 신청 탐색">
          <button type="button" id="wizardTopBack" className="wizard-topback" aria-label="이전으로 돌아가기" onClick={back}>
            <BackIcon />
          </button>
          <span className="wizard-top-title">{editId ? (origStatus === 'draft' ? '발매 이어서 작성' : '발매 정보 수정') : '새로운 발매'}</span>
          <span className="aq-save-slot" aria-live="polite">
            {saveLabel && (
              // 상태가 바뀔 때마다 새로 그려 '저장 중 → 저장됨'이 살며시 바뀐다
              <span key={canAutoSave ? save.kind : 'edit'} className={`aq-save-state is-${canAutoSave ? save.kind : 'edit'}`} title={saveTitle}>{saveLabel}</span>
            )}
          </span>
        </div>
        <div className="wizard-progress" id="wizardProgress" ref={progressRef} aria-label={`발매 신청 진행 단계 ${step + 1} / ${STEPS.length}`}>
          {STEPS.map((st, i) => (
            <span
              key={i}
              className={`wizard-progress-seg${i <= step ? ' current' : ''}${i <= reached && i !== step ? ' aq-seg-link' : ''}`}
              onClick={i <= reached && i !== step ? () => goStep(i) : undefined}
              title={i <= reached && i !== step ? `${st.short}(으)로 이동` : st.short}
            >
              {i === step && <em>{st.short}</em>}
            </span>
          ))}
        </div>
      </div>

      {loadingEdit && (
        <div className="ui-callout aq-loading-notice" role="status" style={{ marginBottom: 12 }}>
          <span className="aq-spinner" aria-hidden="true" /> 기존 발매 정보를 불러오는 중이에요…
        </div>
      )}


      <div className="aq-wiz-layout">
      <aside className="aq-wiz-rail" aria-label="발매 신청 단계">
        <div className="aq-wiz-summary">
          {form.coverData
            ? <img src={form.coverData} alt="" />
            : <span className="aq-wiz-summary-cover" aria-hidden="true"><Glyph name="music" size={18} /></span>}
          <div className="min-0">
            <strong>{form.title.trim() || '제목 없는 발매'}</strong>
            <span>{form.artist.trim() || '아티스트 없음'} · {kindLabel(form.type)}</span>
            <span>{form.tracks.filter(t => t.title.trim()).length}곡{form.releaseDate ? ` · ${formatKoreanDate(form.releaseDate)}` : ''}</span>
          </div>
        </div>
        <div className="aq-wiz-rail-progress" aria-hidden="true">
          <span>발매 신청</span>
          <b>{step + 1} / {STEPS.length}</b>
          <i style={{ ['--p' as string]: `${((step + 1) / STEPS.length) * 100}%` }} />
        </div>
        <ol className="aq-wiz-steps" style={{ ['--cur' as string]: step }}>
          {STEPS.map((st, i) => {
            const state = i === step ? 'is-current' : i <= reached ? 'is-done' : '';
            const fixHere = fixes.some(f => f.step === i);
            return (
              <li key={i} className={`${state}${fixHere ? ' is-fix' : ''}`}>
                <button
                  type="button" disabled={i > reached}
                  aria-current={i === step ? 'step' : undefined}
                  onClick={i <= reached && i !== step ? () => goStep(i) : undefined}
                >
                  <span className="aq-wiz-step-no" aria-hidden="true">
                    {i !== step && i <= reached ? <svg viewBox="0 0 16 16" width="12" height="12"><path d="M3.5 8.5l3 3 6-7" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" /></svg> : i + 1}
                  </span>
                  <span className="aq-wiz-step-label">{st.short}</span>
                  {fixHere && <em className="aq-wiz-step-fix">보완</em>}
                </button>
              </li>
            );
          })}
        </ol>
        <p className="aq-wiz-rail-note">
          {canAutoSave ? '입력 내용은 자동으로 임시 저장돼요.' : '수정 내용은 마지막 단계에서 ‘수정 완료’를 눌러야 반영돼요.'}
        </p>
      </aside>
      <div className="aq-wiz-main">
      <div className="wizard-header" key={`h-${step}`}>
        <p className="aq-step-kicker">{s.kicker}</p>
        <h1 id="newTitle" className="aq-step-title">{s.title}</h1>
        <p id="wizardSubtitle">{s.sub}</p>
      </div>

      {(fixes.length > 0 || reviewNote) && (
        <section className={`aq-fix-panel${fixOpen ? '' : ' is-collapsed'}`} aria-labelledby="aqFixHead">
          <div className="aq-fix-top">
            <Badge tone="amber">{fixes.length ? `보완 요청 ${fixes.length}건` : '담당자 의견'}</Badge>
            <button type="button" className="aq-fix-toggle" aria-expanded={fixOpen} onClick={toggleFix}>
              {fixOpen ? '접기' : '펼치기'} <Glyph name="chevron-down" size={14} />
            </button>
          </div>
          <h2 id="aqFixHead" className="aq-fix-title">
            {fixes.length ? `${fixCountWord(fixes.length)} 고치면 바로 다시 검토해요` : '검토팀이 의견을 남겼어요'}
          </h2>
          {(fixOpen || fixes.some(f => f.step === step)) && (
            <div className="aq-fix-body">
              {fixOpen && reviewNote && (
                <figure className="aq-fix-note">
                  <span className="aq-fix-note-avatar" aria-hidden="true">A</span>
                  <div className="aq-fix-note-bubble">
                    <figcaption>AUDENIQ 검토팀</figcaption>
                    <blockquote><p>{reviewNote}</p></blockquote>
                  </div>
                </figure>
              )}
              {fixes.length > 0 && <ul className="aq-fix-list">
                {fixes.map((c, i) => {
                  const here = c.step === step;
                  if (!fixOpen && !here) return null;
                  return (
                    <li key={`${c.code}-${c.trackId ?? ''}-${i}`} className={here ? 'is-here' : ''}>
                      <span className="aq-fix-step">{c.step + 1}단계 · {STEPS[c.step]?.short}</span>
                      <p><strong>{c.label}</strong>{c.message}</p>
                      {isDocumentFix(c) ? (
                        <button type="button" className={`ui-btn is-sm ${here ? 'is-primary' : 'is-soft'}`} onClick={() => nav(fixPath(editId ?? '', c))}>
                          서류 첨부하기
                        </button>
                      ) : (
                        <button type="button" className={`ui-btn is-sm ${here ? 'is-primary' : 'is-soft'}`} onClick={() => jumpToFix(c)}>
                          {here ? '입력칸 보기' : '고치러 가기'}
                        </button>
                      )}
                    </li>
                  );
                })}
              </ul>}
            </div>
          )}
        </section>
      )}

      <div id="wizardBody" key={`b-${step}`}>
        {step === 0 && (
          <section className="step-section">
            <div className="form-grid">
              <div className="ui-field">
                <label htmlFor="f-artist">아티스트명 <span className="required">*</span></label>
                <input id="f-artist" value={form.artist} onChange={e => set('artist', e.target.value)} onBlur={e => void loadKnownPages(e.target.value)} maxLength={120} placeholder="예: AUDENIQ" autoComplete="off" />
              </div>
              <div className="ui-field">
                <label htmlFor="f-title">발매 제목 <span className="required">*</span></label>
                <input id="f-title" value={form.title} onChange={e => set('title', e.target.value)} maxLength={180} placeholder="싱글 또는 앨범 제목" autoComplete="off" />
              </div>
              <div className="ui-field">
                <label htmlFor="f-type">발매 유형</label>
                <select id="f-type" value={form.type} onChange={e => set('type', e.target.value)}>
                  {KINDS.map(([id, title]) => <option key={id} value={id}>{title}</option>)}
                </select>
              </div>
              <div className="ui-field">
                <label htmlFor="f-language">주요 언어</label>
                <select id="f-language" value={form.language} onChange={e => set('language', e.target.value)}>
                  {LANGUAGES.map(([id, title]) => <option key={id} value={id}>{title}</option>)}
                </select>
              </div>
              <div className="ui-field">
                <label htmlFor="f-genre">장르 <span className="required">*</span></label>
                <select
                  id="f-genre" value={genreIsCustom ? '__other__' : form.genre}
                  onChange={e => {
                    set('genre', e.target.value);
                    if (e.target.value === '__other__') {
                      requestAnimationFrame(() => document.getElementById('f-genre-custom')?.focus());
                    }
                  }}
                  required
                >
                  {GENRES.map(([id, title]) => (
                    <option key={id || 'empty'} value={id} disabled={id === ''}>{title}</option>
                  ))}
                </select>
                {genreIsCustom && (
                  <input
                    type="text" id="f-genre-custom" value={form.genreCustom}
                    onChange={e => set('genreCustom', e.target.value)}
                    maxLength={90} placeholder="예: 시티팝" aria-label="직접 입력할 장르"
                    className="aq-reveal" style={{ marginTop: 10 }}
                  />
                )}
              </div>
              {showMoreInfo && (
              <div className="ui-field aq-reveal">
                <label htmlFor="f-label">레이블 / 발매사 표기 <span className="aq-optional">선택</span></label>
                <input id="f-label" value={form.label} onChange={e => set('label', e.target.value)} maxLength={120} placeholder="권리 계약에 맞는 표기" />
              </div>
              )}
            </div>
            {showMoreInfo ? (
            <div className="ui-field aq-reveal">
              <label htmlFor="f-notes">앨범 소개 <span className="aq-optional">선택</span></label>
              <textarea
                id="f-notes" value={form.notes} onChange={e => set('notes', e.target.value)}
                rows={3} maxLength={1500} placeholder="발매 소개와 전달 메모"
              />
              <p className="ui-field-help aq-counter">{form.notes.length} / 1500</p>
            </div>
            ) : (
              <button type="button" className="ui-btn is-text aq-more-info" aria-expanded={false} onClick={() => setMoreInfo(true)}>
                레이블 표기·앨범 소개 더 입력하기<Glyph name="chevron-down" size={14} />
              </button>
            )}
            <h2 className="subhead">플랫폼 아티스트 프로필</h2>
            <div className="aq-choice-row" role="radiogroup" aria-label="플랫폼 아티스트 프로필">
              {([[true, '처음 발매해요', '플랫폼에 새 아티스트 페이지가 만들어져요.'], [false, '이미 발매한 적이 있어요', '기존 아티스트 페이지에 이어서 올라가요.']] as const).map(([v, t, sub]) => (
                <label key={t} className={`aq-choice${form.artistProfile.isNew === v ? ' is-on' : ''}`}>
                  <input
                    type="radio" name="artistIsNew" checked={form.artistProfile.isNew === v}
                    onChange={() => set('artistProfile', { ...form.artistProfile, isNew: v })}
                  />
                  <strong>{t}</strong>
                  <small>{sub}</small>
                </label>
              ))}
            </div>
            {knownPages && Object.values(knownPages.pages).some(p => p.status === 'LINKED') && (
              <p className="ui-callout is-info small">이전에 등록한 {form.artist} 아티스트 페이지를 불러왔어요. 이번 발매도 같은 페이지에 올라가요.</p>
            )}
            {knownPages && !Object.values(knownPages.pages).some(p => p.status === 'LINKED') && Object.values(knownPages.pages).some(p => p.status === 'PENDING') && (
              <p className="ui-callout is-info small">첫 발매로 {form.artist}의 새 아티스트 페이지가 만들어지고 있어요. 이번 발매도 같은 아티스트로 묶여요. 페이지가 생기면 ‘플랫폼 요청 → 아티스트 프로필’에서 링크를 등록해 주세요.</p>
            )}
            {!form.artistProfile.isNew && (
              <div className="aq-profile-links ui-on-soft aq-reveal">
                <p className="aq-profile-links-help">
                  기존 아티스트 페이지 주소를 <b>하나 이상</b> 넣어 주세요. 이름이 같은 다른 아티스트 페이지로 잘못 올라가는 것을 막아요.
                </p>
                {PROFILE_LINKS.map(l => (
                  <div className="ui-field" key={l.key}>
                    <label htmlFor={`f-link-${l.key}`}>
                      <BrandMark id={l.key} label={l.label} color={DSP_COLOR[l.key]} size={28} className="aq-dsp-mark" />
                      {l.label}
                    </label>
                    <input
                      id={`f-link-${l.key}`} type="url" inputMode="url" autoComplete="off"
                      value={form.artistProfile[l.key]} placeholder={l.placeholder} maxLength={300}
                      onChange={e => set('artistProfile', { ...form.artistProfile, [l.key]: e.target.value })}
                    />
                  </div>
                ))}
              </div>
            )}
          </section>
        )}

        {step === 1 && (
          <section className="step-section">
            <div id="trackEditors" className="aq-track-list">
              {form.tracks.map((t, i) => (
                <TrackEditor
                  key={t.id}
                  track={t}
                  index={i}
                  collapsed={form.tracks.length > 1 && shownTrack !== t.id}
                  complete={trackComplete(t)}
                  onOpen={setOpenTrack}
                  onCollapse={form.tracks.length > 1 ? () => setOpenTrack(null) : undefined}
                  expanded={expandedTracks.has(t.id)}
                  canDelete={form.tracks.length > 1}
                  onToggleExpand={toggleTrackExpand}
                  onDeleteTrack={deleteTrack}
                  setTrack={setTrack}
                  onTrackAudio={onTrackAudio}
                  credits={creditNames}
                  upload={uploads[t.id]}
                  forcedExplicit={forcedFor(i)}
                />
              ))}
            </div>
            {showTypeFix && (
              <div id="aqTypeFix" className="ui-callout is-warn is-row aq-reveal" role="status" tabIndex={-1}>
                <span>
                  <strong>{form.tracks.length}곡{trackSecs.every(Boolean) ? ` · 총 ${Math.round(trackSecs.reduce((a, b) => a + b, 0) / 60)}분` : ''}</strong>
                  {' '}구성은 플랫폼 기준으로 ‘{kindLabel(suggestedType)}’이에요. 지금은 ‘{kindLabel(form.type)}’으로 골라져 있어요.
                </span>
                <button
                  type="button" className="ui-btn is-primary is-sm"
                  onClick={() => { set('type', suggestedType); setTypeNudge(false); clearFieldError(); setError(''); toast(`발매 유형을 ${kindLabel(suggestedType)}(으)로 바꿨어요.`, 'success'); }}
                >{kindLabel(suggestedType)}(으)로 바꾸기</button>
              </div>
            )}
            <div className="spaced-actions">
              <button
                type="button" className="ui-btn is-soft"
                onClick={() => {
                  const t = newTrack();
                  set('tracks', [...form.tracks, t]);
                  setOpenTrack(t.id);
                  requestAnimationFrame(() => document.getElementById(`tr-${form.tracks.length}-title`)?.focus());
                }}
              >
                <Glyph name="plus" size={15} className="aq-glyph-lead" />트랙 추가
              </button>
              <span className="small muted">
                {form.tracks.length}곡 · 파일 {form.tracks.filter(t => t.audioName).length}개 등록
                {form.tracks.some(t => t.audioSize) && ` · ${fileSize(form.tracks.reduce((n, t) => n + (t.audioSize || 0), 0))}`}
              </span>
            </div>
          </section>
        )}

        {step === 2 && (
          <section className="step-section">
            <div className="ui-field">
              <label htmlFor="coverFile">커버아트 <span className="required">*</span></label>
              <label
                className={`aq-dropzone${dragOver ? ' is-over' : ''}${form.coverData ? ' has-file' : ''}`}
                onDragOver={e => { e.preventDefault(); setDragOver(true); }}
                onDragLeave={() => setDragOver(false)}
                onDrop={e => { e.preventDefault(); setDragOver(false); void applyCover(e.dataTransfer.files?.[0]); }}
              >
                <input
                  type="file" id="coverFile" ref={coverInputRef}
                  accept=".jpg,.jpeg,.png,image/jpeg,image/png"
                  onChange={e => void applyCover(e.target.files?.[0], e.target)}
                />
                {form.coverData ? (
                  <img src={form.coverData} alt="" className="aq-dropzone-thumb" />
                ) : (
                  <span className="aq-dropzone-icon" aria-hidden="true">
                    <svg viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"><rect x="3" y="3" width="18" height="18" rx="4" /><circle cx="9" cy="9" r="2" /><path d="m21 15-3.1-3.1a2 2 0 0 0-2.8 0L6 21" /></svg>
                  </span>
                )}
                <span className="aq-dropzone-text">
                  <strong>{form.coverName || '이미지를 끌어다 놓거나 눌러서 선택'}</strong>
                  <small>{form.coverName ? '다른 이미지로 바꾸려면 다시 선택해 주세요.' : `JPG · PNG, 정사각형 ${COVER_MIN}~${COVER_MAX}px (최대 20MB)`}</small>
                </span>
              </label>
              {uploads.cover && <UploadStatus upload={uploads.cover} idle="올렸어요. 바로 다음 단계로 넘어가도 돼요." />}
              {coverWarn && <p className="ui-field-help aq-help-warn">{coverWarn}</p>}
            </div>
            {form.coverData && (
              <div className="cover-dist-preview aq-reveal">
                <h2 className="subhead">배급 미리보기</h2>
                <div className="dist-mock">
                  <img src={form.coverData} alt="배급될 커버아트" />
                  <div className="dist-mock-meta">
                    <strong>{form.title.trim() || '발매 제목'}</strong>
                    <span>{form.artist.trim() || '아티스트'}</span>
                  </div>
                </div>
                <p className="ui-field-help">실제 배급되면 각 플랫폼에 이 커버아트로 표시돼요.</p>
                <button
                  type="button" className="ui-btn is-danger is-sm aq-cover-remove"
                  onClick={() => {
                    uploadAborts.current.get('cover')?.abort();
                    uploadAborts.current.delete('cover');
                    setUpload('cover', null);
                    set('coverName', '');
                    setForm(f => ({ ...f, coverData: '', coverAssetId: '', coverThumbAssetId: '', coverPreviewAssetId: '' }));
                    setCoverWarn('');
                    if (coverInputRef.current) coverInputRef.current.value = '';
                  }}
                >커버아트 지우기</button>
              </div>
            )}
          </section>
        )}

        {step === 3 && (
          <section className="step-section">
            <div className="form-grid">
              <KoreanDateField
                id="f-releaseDate" label="발매 예정일" required
                value={form.releaseDate} min={origDate && origDate < minReleaseDate(form.options.express) ? origDate : minReleaseDate(form.options.express)}
                onChange={v => set('releaseDate', v)}
                help={<>
                  {form.releaseDate && form.releaseDate === autoDate.current ? <>가장 빠른 날짜로 넣어 두었어요. 원하는 날이 있으면 눌러서 바꿔 주세요{form.options.express ? ' (신속 발매, 최소 3일)' : ' (플랫폼 검수에 2주가 걸려요)'}.</>
                    : <>가장 빠른 날짜는 <b>{formatKoreanDate(earliest)}</b>이에요{form.options.express ? ' (신속 발매, 최소 3일)' : ' (플랫폼 검수에 2주가 걸려요)'}.</>}
                  {' '}플레이리스트 추천을 받으려면 3~4주 뒤를 권해요.
                  {!form.options.express && <>
                    {' '}<button type="button" className="aq-inline-link" onClick={() => {
                      set('options', { ...form.options, express: true });
                      requestAnimationFrame(() => document.getElementById('aqExpressReason')?.closest('.aq-option-detail')?.scrollIntoView({ behavior: 'smooth', block: 'center' }));
                    }}>더 빨리 발매해야 하나요?</button>
                  </>}
                </>}
              />
              <div className="ui-field">
                <label htmlFor="f-releaseTime">발매 시각</label>
                <select id="f-releaseTime" className="ui-select" value={form.releaseTime} onChange={e => set('releaseTime', e.target.value)}>
                  <option value="">나라별 자정 (현지 0시)</option>
                  {RELEASE_TIMES.map(t => <option key={t} value={t}>한국 시간 {t} 전 세계 동시{t === '18:00' ? ' (국내 표준)' : ''}</option>)}
                </select>
                <small className="ui-field-help">국내 음원은 보통 오후 6시에 공개돼요. 동시 발매를 고르면 해외도 같은 순간에 열려요(플랫폼 사정으로 조금 늦을 수 있어요).</small>
              </div>
              <div className="ui-field">
                <label htmlFor="f-upc">음반 코드 UPC (있을 때만)</label>
                <input id="f-upc" inputMode="numeric" value={form.upc} onChange={e => set('upc', e.target.value.replace(/\D/g, '').slice(0, 13))} maxLength={13} placeholder="없으면 자동 발급돼요" />
              </div>
            </div>
            <h2 className="subhead">플랫폼 사전 공개·예약 판매<span className="aq-optional">선택</span></h2>
            <div className="form-grid">
              <div className="ui-field">
                <label htmlFor="f-preorder">사전 공개 시작일</label>
                <DateField id="f-preorder" value={form.preorderStart}
                  min={form.releaseDate ? shiftDate(form.releaseDate, -90) : undefined}
                  max={form.releaseDate ? shiftDate(form.releaseDate, -1) : undefined}
                  onChange={v => set('preorderStart', v)} />
                {form.releaseDate && preorderChoices(form.releaseDate).length > 0 && (
                  <div className="ui-chips is-wrap aq-preorder-picks" role="group" aria-label="사전 공개 빠른 선택">
                    {preorderChoices(form.releaseDate).map(c => (
                      <button key={c.weeks} type="button" className="ui-chip is-sm" aria-pressed={form.preorderStart === c.date}
                        onClick={() => set('preorderStart', form.preorderStart === c.date ? '' : c.date)}>발매 {c.weeks}주 전</button>
                    ))}
                  </div>
                )}
                <small className="ui-field-help">이 날부터 Spotify·Apple Music 등에 발매 예정 페이지·트랙 목록·커버가 열려 팬이 앱에서 미리 저장(Apple Music은 미리 추가)할 수 있고, iTunes 같은 다운로드 스토어는 예약 판매를 받아요. 발매일 1–90일 전으로 골라 주세요. 보통 2주 전을 많이 써요.</small>
              </div>
            </div>
            {form.tracks.some(t => t.title.trim()) && (
              <div className="ui-field aq-preview-starts" role="group" aria-labelledby="f-previewStarts">
                <span id="f-previewStarts" className="ui-field-label">미리듣기 시작 지점<span className="aq-optional">선택</span></span>
                {/* 곡마다 '내 발매'와 같은 줄 — 번호 · 곡 이름 · 길이, 오른쪽에 시작 지점 칸 */}
                <div className="ui-list">
                  {form.tracks.filter(t => t.title.trim()).map((t, i) => (
                    <label key={t.id} className="ui-row aq-preview-row">
                      <IconTile size="sm"><span className="aq-preview-no">{i + 1}</span></IconTile>
                      <span className="ui-row-body min-0">
                        <b className="ui-row-title">{t.title}</b>
                        <span className="ui-row-meta">{t.duration ? `길이 ${t.duration}` : '미리듣기 시작'}</span>
                      </span>
                      <input className="ui-input" inputMode="numeric" placeholder="0:00" maxLength={5} value={t.previewStart}
                        id={`f-preview-${i}`} autoComplete="off"
                        aria-label={`${t.title} 미리듣기 시작 지점`}
                        aria-invalid={!!t.previewStart.trim() && !previewOk(t)}
                        onChange={e => setTrack(t.id, 'previewStart', maskPreviewInput(e.target.value))}
                        onBlur={e => { const v = settlePreviewInput(e.target.value); if (v !== t.previewStart) setTrack(t.id, 'previewStart', v); }} />
                    </label>
                  ))}
                </div>
                <small className="ui-field-help">곡별 미리듣기·숏폼 클립이 시작하는 곳이에요. 숫자만 입력하면 돼요 (0105 → 1:05). 비우면 플랫폼이 정해요.</small>
              </div>
            )}
            <h2 className="subhead">배급 지역</h2>
            <Later h={88}><TerritoryPicker value={form.territories} onChange={v => set('territories', v)} /></Later>
            <h2 className="subhead">배급 플랫폼</h2>
            <div className="distribution-default">
              <div>
                {/* 줄 제목은 스위치를 켰을 때의 뜻 — '5개 플랫폼에 배급해요' 옆 스위치가 꺼져 있어 무엇을 끈 건지 헷갈렸다 */}
                <strong>주요 음악 플랫폼에 모두 배급</strong>
                <p className="ui-field-help">{dspError || (!dsps ? '서버에서 플랫폼 상태를 확인하는 중이에요.'
                  : showPlatforms ? `지금은 ${form.platforms.filter(p => !isContentId(p)).length}곳을 골랐어요. 켜면 배급할 수 있는 ${basePlatforms.length}곳 모두에 배급해요.`
                  : `배급할 수 있는 ${basePlatforms.length}곳 모두에 배급해요. 특정 플랫폼만 고르려면 스위치를 꺼 주세요.`)}</p>
              </div>
              <label className="aq-switch">
                <input
                  type="checkbox" aria-label="모든 플랫폼에 배급"
                  checked={allPlatforms && !pickPlatforms}
                  disabled={!dsps || availablePlatforms.length === 0}
                  onChange={e => {
                    // 켜면 모두 선택하고 목록을 닫고, 끄면 지금 선택 그대로 목록을 연다
                    if (e.target.checked) { set('platforms', [...basePlatforms, ...form.platforms.filter(isContentId)]); setPickPlatforms(false); } else setPickPlatforms(true);
                  }}
                />
                <span />
              </label>
            </div>
            {cidPlatforms.length > 0 && (
              <div className="distribution-default aq-cid-optin">
                <div>
                  <strong>YouTube Content ID<span className="aq-optional">선택</span></strong>
                  <p className="ui-field-help">다른 사람 영상에 내 음원이 쓰이면 권리를 주장하고 수익을 받아요. 직접 만든 고유한 녹음일 때만 켜 주세요. 커버·샘플·무료 음원이 들어 있으면 끄는 게 좋아요.</p>
                </div>
                <label className="aq-switch">
                  <input
                    type="checkbox" aria-label="YouTube Content ID 사용"
                    checked={cidOn}
                    onChange={e => set('platforms', e.target.checked
                      ? [...form.platforms.filter(p => !isContentId(p)), ...cidPlatforms]
                      : form.platforms.filter(p => !isContentId(p)))}
                  />
                  <span />
                </label>
              </div>
            )}
            {showPlatforms && (
            <div id="aqPlatforms" className="aq-dsp-groups aq-reveal">
              {/* 지금 고를 수 없는 플랫폼(40여 곳)은 고를 수 있는 곳과 같은 크기로 섞어 두지 않고 맨 아래 접힌 목록으로 — 휴대폰에서 이 단계가 4,000px 가까이 길어졌다 */}
              {DSP_GROUPS.map(([region, match]) => [region, dsps?.filter(d => match(d) && (d.available || form.platforms.includes(d.slug))) ?? []] as const).filter(([, list]) => list.length > 0).map(([region, list]) => (
                <div key={region}>
                  <p className="aq-dsp-region">{region}</p>
                  <div className="aq-dsp-grid" role="group" aria-label={`${region} 플랫폼`}>
                    {list.map(({ slug: key, name: label, available }) => {
                      const on = form.platforms.includes(key);
                      return (
                        <button
                          key={key} type="button" aria-pressed={on} disabled={!available && !on} className={`aq-dsp${on ? ' is-on' : ''}${!available ? ' is-unavailable' : ''}`}
                          onClick={() => set('platforms', on ? form.platforms.filter(p => p !== key) : [...form.platforms, key])}
                        >
                          <BrandMark id={key} label={label} color={DSP_COLOR[key] ?? '#3B63F3'} size={28} className="aq-dsp-mark" />
                          <span className="aq-dsp-name">{breakAtDots(label)}{!available && <small>지금은 배급할 수 없어요</small>}</span>
                          <span className="aq-dsp-check" aria-hidden="true"><CheckIcon size={16} /></span>
                        </button>
                      );
                    })}
                  </div>
                </div>
              ))}
              {unavailableDsps.length > 0 && (
                <details className="aq-dsp-unavailable">
                  <summary>지금은 배급할 수 없는 플랫폼 {unavailableDsps.length}곳<Glyph name="chevron-down" size={14} /></summary>
                  <p>{unavailableDsps.map(d => d.name).join(', ')}</p>
                  <small>배급이 열리면 여기서 고를 수 있게 바뀌어요.</small>
                </details>
              )}
            </div>
            )}
            {!dsps && <p className="ui-field-help" role="status">{dspError || '플랫폼 목록을 서버에서 불러오는 중이에요.'}</p>}
            {/* 공동작업자 수익 분배 — 정산은 배급 조건이라 권리 확인이 아니라 여기서 정한다. 임시 저장된 발매(서버의 곡 목록)를 기준으로 */}
            {draftIdRef.current
              ? <SplitsCard releaseId={draftIdRef.current} embedded registerSave={registerSplitsSave} />
              : !MOCK && <p className="small muted">임시 저장이 끝나면 공동작업자 수익 분배를 정할 수 있어요.</p>}
            <div className="aq-fix-zone"><Later h={240}><OptionsSection form={form} set={set} group="service" onDocument={attachOptionDocument} uploads={uploads} onElectronic={kind => void openElectronic(kind)} onOpenDoc={setOpenDocId} releaseId={draftIdRef.current ?? undefined} onGuardian={() => void openGuardian()} /></Later></div>
          </section>
        )}

        {step === 4 && (
          <section className="step-section">
            <div className="form-grid">
              <div className="ui-field aq-field-wide">
                <label htmlFor="f-ownership">음원(마스터) 권리자 <span className="required">*</span></label>
                <input
                  id="f-ownership" value={form.ownership} maxLength={160} placeholder="개인명 또는 법인명"
                  onChange={e => {
                    // 권리자를 입력하면 ℗·© 표기를 ‘연도 권리자’로 바로 채운다 (직접 고친 표기는 건드리지 않음)
                    const who = e.target.value;
                    const y = (form.releaseDate || todayStr()).slice(0, 4);
                    const auto = (v: string) => !v.trim() || v === `${y} ${form.ownership.trim()}`;
                    const line = who.trim() ? `${y} ${who.trim()}` : '';
                    setForm(f => ({
                      ...f, ownership: who,
                      phonogram: auto(f.phonogram) ? line : f.phonogram,
                      copyright: auto(f.copyright) ? line : f.copyright,
                    }));
                    dirtyRef.current = true;
                  }}
                />
                <small className="ui-field-help">입력하면 아래 ℗·© 표기가 ‘{(form.releaseDate || todayStr()).slice(0, 4)} 권리자명’으로 자동으로 채워져요. 다르면 직접 고쳐 주세요.</small>
              </div>
              <div className="ui-field">
                <label htmlFor="f-phonogram">℗ 음반제작자 권리 표기 <span className="required">*</span></label>
                <input id="f-phonogram" value={form.phonogram} onChange={e => set('phonogram', e.target.value)} maxLength={180} placeholder={`예: ${new Date().getFullYear()} 권리자명`} />
              </div>
              <div className="ui-field">
                <label htmlFor="f-copyright">© 아트워크 / 앨범 권리 표기 <span className="required">*</span></label>
                <input id="f-copyright" value={form.copyright} onChange={e => set('copyright', e.target.value)} maxLength={180} placeholder={`예: ${new Date().getFullYear()} 권리자명`} />
              </div>
            </div>
            {!form.ownership && !form.phonogram && !form.copyright && form.artist.trim() && (
              <button
                type="button" className="ui-btn is-soft is-block aq-autofill"
                onClick={() => {
                  const y = (form.releaseDate || todayStr()).slice(0, 4);
                  const who = form.label.trim() || form.artist.trim();
                  setForm(f => ({ ...f, ownership: who, phonogram: `${y} ${who}`, copyright: `${y} ${who}` }));
                  dirtyRef.current = true;
                }}
              >
                <Glyph name="pencil" size={15} className="aq-glyph-lead" />
                <span className="aq-autofill-text">‘{form.label.trim() || form.artist.trim()}’(으)로 권리자 정보 채우기</span>
              </button>
            )}
            <div id="aqSpecialOptions" className="aq-fix-zone"><Later h={160}><OptionsSection form={form} set={set} group="rights" onDocument={attachOptionDocument} uploads={uploads} onElectronic={kind => void openElectronic(kind)} onOpenDoc={setOpenDocId} releaseId={draftIdRef.current ?? undefined} onGuardian={() => void openGuardian()} /></Later></div>
            {/* 곡의 작사·작곡 저작권은 퍼블리싱 메뉴에서 — 신청서에는 한 줄 안내만 (적은 크레딧으로 곡 정보는 미리 정리해 둔다) */}
            <p className="ui-field-help">작사·작곡 저작권은 발매 뒤 <b>퍼블리싱</b> 메뉴에서 맡길 수 있어요. 여기 적은 크레딧으로 곡 정보를 미리 정리해 둘게요.</p>
            <h2 className="subhead">필수 확인 항목</h2>
            <div className="field-group">
              {RIGHTS_CHECKS.map(([k, label]) => (
                <label key={k} className="ui-check is-plain">
                  <input
                    type="checkbox"
                    checked={!!form.rightsChecks[k]}
                    onChange={e => set('rightsChecks', { ...form.rightsChecks, [k]: e.target.checked })}
                  />
                  <span>{label}</span>
                </label>
              ))}
            </div>
            {applicableConditionals(form.options).length > 0 && (
              <>
                <h2 className="subhead" style={{ marginTop: 20 }}>선택한 옵션 추가 확인</h2>
                <div className="field-group">
                  {applicableConditionals(form.options).map(([k, label]) => (
                    <label key={k} className="ui-check is-plain">
                      <input
                        type="checkbox"
                        checked={!!form.rightsChecks[k]}
                        onChange={e => set('rightsChecks', { ...form.rightsChecks, [k]: e.target.checked })}
                      />
                      <span>{label}</span>
                    </label>
                  ))}
                </div>
              </>
            )}
            <div className="ui-callout">
              허락서·동의서는 위의 AUDENIQ 전자 문서 도구에서 작성하고 권리자가 직접 서명할 수 있어요. 완성된 서류는 ‘권리·보완 서류’에도 보관돼요.
            </div>
            <Later><OptionDocsBanner options={form.options} /></Later>
          </section>
        )}

        {step === 5 && (
          <section className="step-section">
            <div className="aq-catalog-cards aq-stagger">
              {reviewSections.map(([h, t, target, missing]) => (
                <div key={h} className={`aq-review-card${missing ? ' is-missing' : ''}`}>
                  <div className="aq-review-body">
                    <strong>{h}</strong>
                    <div className="break aq-review-value">{t}</div>
                  </div>
                  <button type="button" className="ui-btn is-white is-xs aq-review-edit" onClick={() => goStep(target)} aria-label={`${h} ${missing ? '채우기' : '수정'}`}>{missing ? '채우기' : '수정'}</button>
                </div>
              ))}
            </div>
            <div className="ui-callout" style={{ marginTop: 24 }}>
              {resubmit
                ? '‘다시 접수하기’를 누르면 고친 내용으로 다시 검토를 받아요. 처음 낸 배급 신청서는 그대로 유지돼요.'
                : `아래에 서명하고 ‘${editId && origStatus !== 'draft' ? '서명하고 수정 완료' : '서명하고 접수하기'}’를 누르면 배급 신청서가 발급되고 권리 확인서가 준비돼요. 최종 승인과 배급일은 담당자 검토 후 확정돼요.`}
            </div>
            <Later h={160}><FinalReviewBanner form={form} /></Later>
            <AddonSummary options={form.options} onEdit={() => goStep(3)} />

            {!resubmit && (
            <section className="aq-apply-sign" aria-labelledby="aqApplySignHead">
              <h2 className="subhead" id="aqApplySignHead">신청인 서명</h2>
              <p className="ui-field-help">서명하면 입력한 내용으로 배급 신청서가 만들어지고, 접수 후 바로 확인·저장할 수 있어요.</p>
              <div className="form-grid">
                <div className="ui-field">
                  <label htmlFor="f-signer">신청인 성명 <span className="required">*</span></label>
                  <input
                    id="f-signer" autoComplete="name" maxLength={80} value={signerName}
                    onChange={e => setSigner(v => ({ ...v, name: e.target.value, touched: true }))} placeholder="실명 또는 법인명"
                  />
                </div>
                <div className="ui-field">
                  <label htmlFor="f-signer-role">신청인 구분</label>
                  <select id="f-signer-role" value={signer.role} onChange={e => setSigner(v => ({ ...v, role: e.target.value }))}>
                    {SIGNER_ROLES.map(r => <option key={r} value={r}>{r}</option>)}
                  </select>
                </div>
              </div>
              <div className="ui-field">
                <label htmlFor="aqApplyPad">서명 <span className="required">*</span></label>
                <SignaturePad ref={signRef} id="aqApplyPad" label="신청인 서명 입력" height={170} onChange={setSigned} />
              </div>
              <div className="aq-agree-box">
                <label className="ui-check is-plain aq-agree-all-line">
                  <input
                    type="checkbox" checked={AGREEMENTS.every(a => agreed[a.id])}
                    onChange={e => setAgreed(Object.fromEntries(AGREEMENTS.map(a => [a.id, e.target.checked])))}
                  />
                  <span><strong>아래 내용에 모두 동의해요</strong></span>
                </label>
                {AGREEMENTS.map(a => (
                  <label key={a.id} className="ui-check is-plain">
                    <input
                      type="checkbox" id={`agree-${a.id}`} checked={!!agreed[a.id]}
                      onChange={e => setAgreed(v => ({ ...v, [a.id]: e.target.checked }))}
                    />
                    <span>{a.text}{a.id === 'terms' && <> <a href="/terms" target="_blank" rel="noopener" className="aq-inline-link">약관 보기</a></>}</span>
                  </label>
                ))}
              </div>
              <PublishingAlongside on={pubAlong} onToggle={setPubAlong} agreed={pubAgreed} onAgree={setPubAgreed} />
            </section>
            )}
          </section>
        )}
      </div>

      {error && <div id="wizardError" className="ui-callout is-danger aq-shake" role="alert" key={error}>{error}</div>}
      {submitError && step === STEPS.length - 1 && (
        <div id="wizardSubmitError" className="aq-submit-error ui-tone-danger" role="alert">
          <span className="aq-submit-error-icon" aria-hidden="true"><Glyph name="alert" size={18} /></span>
          <div className="aq-submit-error-text">
            <strong>{resubmit ? '다시 접수하지 못했어요' : '접수하지 못했어요'}</strong>
            <p>{submitError}</p>
            <small>입력한 내용{resubmit ? '' : '과 서명'}은 그대로 남아 있어요. 잠시 후 다시 시도해 주세요.</small>
          </div>
          <button type="button" className="ui-btn is-primary is-sm" onClick={() => void submit()} disabled={submitting}>
            {submitting ? '접수하는 중' : '다시 시도'}
          </button>
        </div>
      )}
      </div>
      </div>

      <div className="step-actions">
        <button type="button" id="wizardBack" className="ui-btn is-soft" onClick={back} disabled={submitting}>
          {step === 0 ? (editId ? '나가기' : '홈으로') : '이전'}
        </button>
        {resubmit && step < STEPS.length - 1 && (
          // 이미 접수한 발매를 고칠 때는 끝까지 가지 않고 여기서 바로 저장(다시 접수)할 수 있다
          <button type="button" id="wizardSaveNow" className="ui-btn is-soft" onClick={() => void submit()} disabled={submitting || loadingEdit}>
            {submitting ? '저장하는 중' : '수정 완료'}
          </button>
        )}
        <button
          type="button" id="wizardNext"
          className={`ui-btn is-primary${submitting ? ' is-busy' : ''}`}
          onClick={next} disabled={submitting || loadingEdit} aria-busy={submitting}
        >
          {nextLabel}
        </button>
      </div>
      {guardianOpen && draftIdRef.current && orgId() && (
        <GuardianConsentPanel
          org={orgId() as string}
          releaseId={draftIdRef.current}
          onClose={() => setGuardianOpen(false)}
          onReady={() => {
            set('options', { ...formRef.current.options, guardianConsentDone: true });
            setGuardianOpen(false);
            toast('법정대리인 동의를 확인했어요.', 'success');
          }}
        />
      )}
      {electronic && <RightsDocumentModal kind={electronic.kind} context={electronic.context} onClose={() => setElectronic(null)} onRequested={() => toast('서명 요청을 만들었어요. 권리자가 서명하면 권리·보완 서류에 문서가 생겨요.', 'success')} />}
      {openDoc && <DocumentModal doc={openDoc} onClose={() => setOpenDocId(null)} onOpenSignature={() => {}} />}
    </div>
    </section>
  );
}
