// 발매 신청서 — 입력 모양·상수·서버 전송 형식·커버 처리. 화면 없는 부분 (pages/Upload가 쓴다)
import { MOCK, api, type ArtistProfileLinks, type ReleasePayload } from '../../api/client';
import { addDoc, docsForRelease, getDocsSnapshot, type DocRecord } from '../../store/docs';
import { type RereleaseData } from '../../lib/rerelease';
import { localStamp } from '../../lib/format';
import { DSP, dspLabel, genreLabel, kindLabel } from '../../lib/catalog';
import { stampNow } from '../../lib/date';
import { COVER_MIN, LEAD_DAYS, mmssToSeconds } from '../../lib/dsp';
import { uid } from '../../lib/store';
import { OTHER } from '../../components/ChipPicker';
import { EMPTY_ADDONS, type AddonFields } from '../../components/AddonOptions';
import { territorySummary } from '../../lib/territories';
import { previewSecs } from '../../lib/previewTime';


/** 동시 발매로 고를 수 있는 한국 시간 (서버는 정각·30분만 받는다) */
export const RELEASE_TIMES = ['00:00', '09:00', '12:00', '13:00', '18:00', '19:00', '20:00'];

/** 미리듣기 시작 지점이 맞는지 — 형식, 그리고 곡 길이를 알면 그보다 앞이어야 한다 (끝 30초 전까지 클립이 나오게 하지는 않는다) */
export function previewOk(t: { previewStart: string; duration: string }): boolean {
  const secs = previewSecs(t.previewStart);
  if (!secs) return false;
  const len = mmssToSeconds(t.duration);
  return !len || Number(secs) < len;
}

/** 'YYYY-MM-DD'에 일수를 더한다 */
export function shiftDate(d: string, days: number): string {
  const t = new Date(`${d}T00:00:00Z`);
  t.setUTCDate(t.getUTCDate() + days);
  return t.toISOString().slice(0, 10);
}

export const STEPS = [
  { short: '발매 정보', kicker: '01 / 06 · 발매 정보', title: '어떤 음악을\n발매할까요?', sub: '별표(*) 칸만 채우면 다음 단계로 넘어갈 수 있어요.' },
  { short: '트랙 등록', kicker: '02 / 06 · 트랙 등록', title: '발매할 곡을\n등록해 주세요.', sub: '곡마다 음원 파일과 크레딧을 넣어요.' },
  { short: '커버아트', kicker: '03 / 06 · 커버아트', title: '커버아트를\n등록해 주세요.', sub: `${COVER_MIN}×${COVER_MIN}px 이상 정사각형 이미지가 필요해요.` },
  { short: '배급 설정', kicker: '04 / 06 · 배급 설정', title: '언제, 어디에\n발매할까요?', sub: `보통 접수일로부터 ${LEAD_DAYS.normal}일 뒤부터 발매할 수 있어요.` },
  { short: '권리 확인', kicker: '05 / 06 · 권리 확인', title: '음악의 권리를\n확인해 주세요.', sub: '권리자 정보와 필요한 증빙을 준비해 주세요.' },
  { short: '최종 확인', kicker: '06 / 06 · 최종 확인', title: '발매 정보를\n마지막으로 확인해 주세요.', sub: '빠진 항목이 있으면 여기서 알려 드려요.' },
];

/** 1~5는 '한·두·세·네·다섯 가지만', 그 이상은 'N가지를' */

/** 1단계에서 접어 두는 선택 칸 */
export const MORE_INFO_FIELDS = ['f-label', 'f-notes'];

export const RIGHTS_CHECKS: [string, string][] = [
  ['rightsMaster', '음원 마스터를 배급할 권한이 있어요.'],
  ['rightsComposition', '작사·작곡·편곡 등 저작물 이용에 필요한 허락을 확보했어요.'],
  ['rightsArtwork', '커버아트와 사용한 이미지·폰트에 필요한 이용 권한이 있어요.'],
  ['rightsConsent', '입력한 정보가 정확하며 필요한 권리 증빙을 요청받으면 제출할 수 있어요.'],
];

export const CONDITIONAL_RIGHTS: [string, string, (o: ReleaseOptions) => boolean][] = [
  ['rightsSamples', '샘플링·피처링 관련 제3자 권리 허락을 확보했어요.', o => o.sample || o.featured],
  ['rightsAi', 'AI 생성·보조 제작물의 플랫폼 수용 기준을 확인했어요.', o => o.ai],
  ['rightsShared', '공동 권리자와의 배급 위임 범위를 확인했어요.', o => o.shared],
  ['rightsRerelease', '이전 발매와 겹쳐 보내지 않는지 확인했어요.', o => o.rerelease],
];

export interface Track {
  id: string;
  title: string; version: string; isrc: string;
  composers: string; lyricists: string; arrangers: string; performers: string;
  producer: string;
  lyrics: string;
  audioName: string; audioSize: number; explicit: boolean; duration: string;
  /** 업로드 완료된 음원 자산 ID (실서버) */
  assetId: string;
  /** 서버 트랙 ID (실서버, 임시 저장 간 매칭) */
  serverId: string;
  /** 미리듣기·숏폼 클립 시작점 (입력 그대로 'm:ss' 또는 초) */
  previewStart: string;
  featuring: string;
  instrumental: boolean;
  /** 음원 규격 표기 */
  audioSpec: string;
}

/** 파일 업로드 진행 상태 (저장하지 않는 화면 전용 상태)
 *  received: 파일은 저장소에 다 올라갔고 서버가 마무리 확인 중 — 기다리지 않고 다음 단계로 넘어갈 수 있다(접수할 때만 기다린다) */
export interface UploadState { pct: number; state: 'uploading' | 'received' | 'done' | 'error'; message?: string }

export interface CoverTrackInfo {
  trackId: string;
  originalTitle: string;
  originalArtist: string;
  originalWriters: string;
}

export interface ReleaseOptions extends RereleaseData, AddonFields {
  express: boolean; expressAck: boolean; expressReason: string;
  minor: boolean;
  guardian: string; guardianRelation: string; guardianContact: string;
  guardian2: string; guardian2Relation: string; guardian2Contact: string;
  guardianConsentDone: boolean; familyCertName: string; familyCertMethod: string;
  cover: boolean; coverTracks: CoverTrackInfo[]; coverRightsAck: boolean; coverLicenseFile: string; coverLicenseAssetId?: string;
  sample: boolean; sampleLicenseFile: string; sampleLicenseAssetId?: string;
  featured: boolean; featuredConsentFile: string; featuredConsentAssetId?: string;
  ai: boolean; aiTool: string;
  contentIdExclusiveRightsAck?: boolean; contentIdOriginalRecordingAck?: boolean;
  /** 버튼으로 고른 AI 활용 방식·도구 (aiTool은 이걸 합친 문장) */
  aiUses: string[]; aiTools: string[]; aiUseOther: string; aiToolOther: string;
  shared: boolean; sharedContractFile: string; sharedContractAssetId?: string;
  rerelease: boolean; previousTitle: string; previousId: string;
}

export interface WizardForm {
  artist: string; title: string; type: string; language: string; genre: string;
  genreCustom: string; label: string; notes: string;
  tracks: Track[];
  coverName: string; coverData: string; coverAssetId: string; coverThumbAssetId: string; coverPreviewAssetId: string;
  releaseDate: string; releaseTime: string; preorderStart: string; originalDate: string; upc: string;
  territories: string[]; platforms: string[];
  ownership: string; phonogram: string; copyright: string;
  rightsChecks: Record<string, boolean>;
  options: ReleaseOptions;
  artistProfile: ArtistProfileLinks;
}

export const newTrack = (): Track => ({
  id: uid('t'),
  title: '', version: '', isrc: '', composers: '', lyricists: '', arrangers: '', performers: '',
  producer: '',
  lyrics: '',
  audioName: '', audioSize: 0, explicit: false, duration: '',
  assetId: '', serverId: '', previewStart: '', featuring: '', instrumental: false, audioSpec: '',
});

export const EMPTY_OPTIONS: ReleaseOptions = {
  express: false, expressAck: false, expressReason: '',
  minor: false,
  guardian: '', guardianRelation: '', guardianContact: '',
  guardian2: '', guardian2Relation: '', guardian2Contact: '',
  guardianConsentDone: false, familyCertName: '', familyCertMethod: '',
  cover: false, coverTracks: [], coverRightsAck: false, coverLicenseFile: '',
  sample: false, sampleLicenseFile: '',
  featured: false, featuredConsentFile: '',
  ai: false, aiTool: '', aiUses: [], aiTools: [], aiUseOther: '', aiToolOther: '',
  shared: false, sharedContractFile: '',
  rerelease: false, previousTitle: '', previousId: '',
  ...EMPTY_ADDONS,
};

/** YouTube Content ID — 다른 사람 영상에 권리를 주장하는 별도 서비스라 '모두 배급'에 넣지 않고 따로 켠다 */
export const isContentId = (slug: string) => slug === 'youtube-cid' || slug === 'D-27';

export const EMPTY: WizardForm = {
  artist: '', title: '', type: 'single', language: 'ko', genre: '', genreCustom: '',
  label: '', notes: '',
  tracks: [newTrack()],
  coverName: '', coverData: '', coverAssetId: '', coverThumbAssetId: '', coverPreviewAssetId: '',
  releaseDate: '', releaseTime: '', preorderStart: '', originalDate: '', upc: '',
  territories: ['WORLD'], platforms: MOCK ? DSP.map(d => d[0]).filter(s => !isContentId(s)) : [],
  ownership: '', phonogram: '', copyright: '',
  rightsChecks: {},
  options: EMPTY_OPTIONS,
  artistProfile: { isNew: true, spotify: '', apple: '', melon: '' },
};

export const SERVICE_OPTIONS: [keyof ReleaseOptions, string, string][] = [
  ['express', '신속 발매 · 10,000원', '심사와 전송을 먼저 처리해 3영업일 안에 발매해요.'],
  ['mv', '뮤직비디오 배급 · 19,000원부터', '멜론·지니·벅스·FLO·바이브·유튜브에 보내고, 영상물등급위원회 심의도 대신해요.'],
  ['lyricSync', 'AI 가사 싱크 · 곡당 5,000원', '가사에 노래 시간을 맞춘 싱크 가사를 만들어 Musixmatch·국내 플랫폼에 등록해요.'],
];

export const RIGHTS_OPTIONS: [keyof ReleaseOptions, string, string][] = [
  ['minor', '미성년 아티스트·권리자', '법정대리인의 동의와 권한 확인이 필요해요.'],
  ['cover', '커버곡', '원곡의 작사·작곡 저작물 이용 권한을 확인해요.'],
  ['sample', '샘플링·타인 음원 사용', '원본 음원과 저작물의 이용 허락을 확인해요.'],
  ['featured', '피처링·공동 실연', '참여자 크레딧과 필요한 이용 허락을 확인해요.'],
  ['ai', 'AI 생성·보조 제작', '제작 방식과 각 플랫폼의 수용 기준을 검토해요.'],
  ['shared', '공동 권리자·레이블 계약', '각 권리자와 배급 위임 범위를 확인해요.'],
  ['rerelease', '기존 발매 이전·재발매', '이전 발매의 음반·음원 코드와 겹치지 않는지 확인해요.'],
];

export const OPTIONS_CATALOG: [keyof ReleaseOptions, string, string][] = [
  ...SERVICE_OPTIONS,
  ...RIGHTS_OPTIONS,
];

export function rightsOk(f: WizardForm): boolean {
  if (!RIGHTS_CHECKS.every(([k]) => f.rightsChecks[k])) return false;
  return CONDITIONAL_RIGHTS
    .filter(([, , cond]) => cond(f.options))
    .every(([k]) => f.rightsChecks[k]);
}

export function applicableConditionals(o: ReleaseOptions): [string, string][] {
  return CONDITIONAL_RIGHTS
    .filter(([, , cond]) => cond(o))
    .map(([k, label]) => [k, label]);
}

/** 칸 바로 아래 안내 — 칸을 감싼 .ui-field(없으면 칸 자신) 뒤에 한 줄을 붙이고 칸과 aria-describedby로 잇는다.
 *  한 번에 하나만 둔다 */
export const FIELD_ERROR_ID = 'wizardFieldError';

export function clearFieldError() {
  const old = document.getElementById(FIELD_ERROR_ID);
  if (!old) return;
  document.querySelector(`[aria-describedby~="${FIELD_ERROR_ID}"]`)?.removeAttribute('aria-describedby');
  old.remove();
}

export function showFieldError(el: HTMLElement, msg: string) {
  clearFieldError();
  const host = el.closest<HTMLElement>('.ui-field, .aq-option-detail, .aq-pick, .ui-check') ?? el;
  // 같은 칸에 더 구체적인 안내가 이미 빨갛게 떠 있으면(예: '원본 파일이 아니에요') 그것만 둔다 —
  // '파일을 선택해 주세요'가 아래에 또 붙어 빨간 줄이 두 개가 됐다. 칸은 그대로 흔들고 커서를 둔다
  if (host.querySelector('.aq-help-error:not(#' + FIELD_ERROR_ID + ')')) return;
  const note = document.createElement('p');
  note.id = FIELD_ERROR_ID;
  note.className = 'ui-field-help aq-help-error aq-field-error';
  note.setAttribute('role', 'alert');
  note.textContent = msg;
  if (host.matches('.ui-field, .aq-option-detail')) host.appendChild(note);
  else host.insertAdjacentElement('afterend', note);
  if (!el.hasAttribute('aria-describedby')) el.setAttribute('aria-describedby', FIELD_ERROR_ID);
}

export function selectedOptions(o: ReleaseOptions): [keyof ReleaseOptions, string, string][] {
  return OPTIONS_CATALOG.filter(([id]) => !!o[id]);
}

/** 플랫폼 구분용 색 (로고 대신 첫 글자 배지) */
export const DSP_COLOR: Record<string, string> = {
  melon: '#00c73c', genie: '#1d6bf3', flo: '#3f3fff', bugs: '#ff3a3a', spotify: '#1db954', apple: '#fa2d48',
  youtube: '#ff0033', amazon: '#1ec8d6', tidal: '#111827', deezer: '#a238ff', qobuz: '#1f2a44',
  pandora: '#224099', soundcloud: '#ff5500', audiomack: '#ffa200', anghami: '#9b2cf5', boomplay: '#1e90ff',
  jiosaavn: '#2bc5b4', kkbox: '#09cef6', 'line-music': '#06c755', awa: '#f05a28', netease: '#e60026',
  tencent: '#31c27c', napster: '#2259ff', iheart: '#c6002b', meta: '#0866ff', tiktok: '#111111',
  'youtube-cid': '#cc0000', snapchat: '#e6cf00', beatport: '#01ff95',
  itunes: '#ea4cc0', 'claro-musica': '#da291c', pretzel: '#1a8cff', triller: '#ff0f63', touchtunes: '#003da5', yandex: '#fc3f1d',
  kuaishou: '#ff4906', joox: '#00d05a', trebel: '#6c2bd9', mixcloud: '#5000ff', twitch: '#9146ff', peloton: '#df1c2f',
  canva: '#00c4cc', lickd: '#ff3d6e', adaptr: '#2d2d86', styngr: '#ff7a00',
};

/** 플랫폼 선택 화면의 묶음 — 서버가 주는 region/category 기준 */
export const DSP_GROUPS: [string, (d: { region: string; category?: string }) => boolean][] = [
  ['국내', d => d.region === 'KR'],
  ['해외 스트리밍', d => d.region !== 'KR' && (d.category ?? 'STREAMING') === 'STREAMING'],
  ['소셜·숏폼 영상', d => d.category === 'SOCIAL'],
  ['스토어', d => d.category === 'STORE'],
  ['피트니스·크리에이터·게임 라이선스', d => d.category === 'LICENSING'],
];

export const AI_USES = [
  '전체를 AI로 제작했어요', '작곡(멜로디)에 AI 도움을 받았어요', '작사에 AI 도움을 받았어요', 'AI로 보컬(목소리)을 만들었어요',
  '편곡·반주에 AI를 사용했어요', '믹싱·마스터링에 AI 도구를 썼어요', '커버아트를 AI로 만들었어요',
];

export const AI_TOOLS = ['Suno', 'Udio', 'ChatGPT', 'Stable Audio', 'AIVA', 'Midjourney'];

export const EXPRESS_REASONS = ['공연 일정에 맞춰야 해요', '방송·광고 일정이 있어요', '이벤트·프로모션 일정이 있어요', '영상·드라마 공개일에 맞춰야 해요'];

/** 고른 버튼을 심사용 문장으로 합친다 */
export function aiSummary(o: Pick<ReleaseOptions, 'aiUses' | 'aiTools' | 'aiUseOther' | 'aiToolOther'>): string {
  const uses = o.aiUses.map(u => (u === OTHER ? o.aiUseOther.trim() : u)).filter(Boolean);
  const tools = o.aiTools.map(t => (t === OTHER ? o.aiToolOther.trim() : t)).filter(Boolean);
  return [uses.join(' · '), tools.length ? `도구: ${tools.join(', ')}` : ''].filter(Boolean).join(' / ');
}

/** 폼 → 서버 전송 형식 */
export function toPayload(form: WizardForm, step: number): ReleasePayload {
  return {
    title: form.title.trim(),
    artist: form.artist.trim(),
    type: form.type,
    language: form.language,
    genre: form.genre === '__other__' ? form.genreCustom.trim() : form.genre,
    genreCustom: form.genreCustom.trim(),
    label: form.label.trim(),
    upc: form.upc.trim(),
    notes: form.notes.trim(),
    coverName: form.coverName,
    coverData: form.coverData,
    // 비운 커버('')도 그대로 보낸다 — undefined면 서버의 이전 커버로 되돌아간다
    coverAssetId: form.coverAssetId,
    coverThumbAssetId: form.coverThumbAssetId || undefined,
    coverPreviewAssetId: form.coverPreviewAssetId || undefined,
    originalDate: form.options.rerelease && form.options.rereleaseKind === 'new_version' ? form.releaseDate : form.originalDate,
    release_date: form.releaseDate || '',
    release_time: form.releaseTime || '',
    preorder_start: form.preorderStart || '',
    tracks: form.tracks.map(t => ({
      id: t.id, title: t.title.trim(), isrc: t.isrc.trim(), duration: t.duration,
      version: t.version.trim(), composers: t.composers.trim(),
      lyricists: t.lyricists.trim(), arrangers: t.arrangers.trim(),
      performers: t.performers.trim(), audioName: t.audioName,
      audioSize: t.audioSize, explicit: t.explicit,
      producer: t.producer.trim(), lyrics: t.lyrics.trim(),
      assetId: t.assetId || undefined, serverId: t.serverId || undefined,
      previewStart: previewSecs(t.previewStart),
      featuring: t.featuring.trim(), instrumental: t.instrumental, audioSpec: t.audioSpec || undefined,
    })),
    territories: form.territories,
    platforms: form.platforms,
    ownership: form.ownership.trim(),
    phonogram: form.phonogram.trim(),
    copyright: form.copyright.trim(),
    rightsChecks: form.rightsChecks,
    options: { ...form.options, ...(form.options.rerelease ? { previousReleaseDate: form.originalDate } : {}) },
    lastStep: step,
    artistProfile: form.artistProfile.isNew
      ? { isNew: true, spotify: '', apple: '', melon: '' }
      : { ...form.artistProfile, spotify: form.artistProfile.spotify.trim(), apple: form.artistProfile.apple.trim(), melon: form.artistProfile.melon.trim() },
  };
}

// 미리보기용 썸네일 생성 (최대 480px JPEG, 보통 20~40KB) — 브라우저 저장소 용량을 넘지 않도록 원본 대신 저장
export function makeCoverThumbnail(file: File): Promise<{ data: string; width: number; height: number }> {
  // 실서버는 발매 정보(최대 64KB)에 함께 저장하므로 더 작게
  return MOCK ? scaleCover(file, 480, 0.8) : scaleCover(file, 240, 0.72);
}

/** 커버를 긴 변 max px JPEG로 줄인다 (width·height는 원본 크기).
 *  서버의 이미지 자산은 JPEG·PNG만 받고(WebP 미지원) 브라우저마다 WebP 저장 지원도 달라 JPEG로 한다.
 *  화면에 쓸 크기로만 줄이고 화질은 높게(0.86) 둬서 눈으로는 원본과 구분되지 않으면서 용량은 수십 분의 일이 된다. */
export function scaleCover(file: File, max: number, quality: number): Promise<{ data: string; width: number; height: number }> {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(file);
    const img = new Image();
    img.onload = () => {
      URL.revokeObjectURL(url);
      const scale = Math.min(1, max / Math.max(img.width, img.height));
      const w = Math.max(1, Math.round(img.width * scale));
      const h = Math.max(1, Math.round(img.height * scale));
      const canvas = document.createElement('canvas');
      canvas.width = w; canvas.height = h;
      const ctx = canvas.getContext('2d');
      if (ctx) { ctx.imageSmoothingEnabled = true; ctx.imageSmoothingQuality = 'high'; ctx.drawImage(img, 0, 0, w, h); }
      resolve({ data: canvas.toDataURL('image/jpeg', quality), width: img.width, height: img.height });
    };
    img.onerror = () => { URL.revokeObjectURL(url); reject(new Error('이미지를 읽을 수 없어요.')); };
    img.src = url;
  });
}

/** 커버 미리보기(JPEG)를 이미지 파일로 올리고 자산 ID를 돌려준다 */
export async function uploadCoverPreview(dataUrl: string): Promise<string> {
  // fetch(data:)는 CSP connect-src에 막힐 수 있어 직접 바꾼다
  const bin = atob(dataUrl.slice(dataUrl.indexOf(',') + 1));
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  const file = new File([bytes], 'cover-preview.jpg', { type: 'image/jpeg' });
  const r = await api.uploadFile(file, 'IMAGE', () => {});
  return r.assetId;
}

export const AUDIO_RE = /\.(wav|flac|m4a|aif|aiff|aifc|wv|tta)$/i;

/** 발매 신청 시 계약서·권리 서류를 준비 (같은 발매에 이미 있으면 다시 만들지 않음) */
export function ensureReleaseDocuments(f: WizardForm, releaseId: string) {
  const title = f.title.trim() || '제목 없는 발매';
  const existing = docsForRelease(getDocsSnapshot(), releaseId);
  if (existing.some(d => d.kind === 'agreements')) return;
  const genreVal = f.genre === '__other__' ? f.genreCustom.trim() : genreLabel(f.genre);
  const at = stampNow();
  const lines = [
    'AUDENIQ 디지털 음원 배급 신청·계약서',
    '',
    '1. 신청인 및 발매 정보',
    `아티스트: ${f.artist.trim() || '입력 전'}`,
    `발매명: ${title}`,
    `발매 유형: ${kindLabel(f.type)}`,
    `장르: ${genreVal || '입력 전'}`,
    `발매 희망일: ${f.releaseDate || '미정'}${f.releaseTime ? ` (한국 시간 ${f.releaseTime} 동시 발매)` : ''}`,
    `레이블 표기: ${f.label.trim() || '입력 전'}`,
    `수록곡: ${f.tracks.map((t, i) => `${i + 1}. ${t.title.trim() || '제목 없음'} (${t.duration || '길이 확인 전'})`).join(' / ')}`,
    '',
    '2. 권리자 및 배급 범위',
    `마스터 권리자: ${f.ownership.trim() || '입력 전'}`,
    `℗ 표기: ${f.phonogram.trim() || '입력 전'}`,
    `© 표기: ${f.copyright.trim() || '입력 전'}`,
    `배급 지역: ${territorySummary(f.territories)}`,
    `배급 플랫폼: ${f.platforms.map(dspLabel).join(', ')}`,
    `추가 확인 항목: ${selectedOptions(f.options).map(([, t]) => t).join(', ') || '일반 발매'}`,
    '',
    '3. 신청인의 확인',
    '신청인은 제출한 음원, 가사, 커버아트, 크레딧 및 메타데이터를 배급할 적법한 권한을 보유하고 있으며, 제3자의 권리가 포함된 경우 필요한 허락을 확보했음을 확인합니다.',
    '',
    `신청일: ${localStamp(at)}`,
  ];
  const base = {
    releaseId, releaseTitle: title, version: '1.0', created: at,
    fileName: '', checked: false, checkedAt: '', consentHistory: [],
    reviewNote: '', signerName: '', localSignatureData: '', localSignatureAt: '',
  };
  const rights: DocRecord = {
    ...base, id: uid('doc'), kind: 'rights',
    title: title + ' · 권리 증빙 제출',
    content: '발매 권리를 확인할 수 있는 자료를 제출해 주세요. 해당하는 자료: 마스터 음원 제작 또는 이용 허락서, 커버아트 사용 허락서, 공동 창작·피처링 또는 커버곡의 권리 허락서. 필요한 자료만 제출하면 돼요.',
    reviewHistory: [{ status: '서류 접수 대기', time: at, detail: '권리 관련 증빙이 필요한 경우 제출해 주세요.' }],
    reviewStatus: 'awaiting_documents',
  };
  const agreement: DocRecord = {
    ...base, id: uid('doc'), kind: 'agreements',
    title: title + ' · AUDENIQ 디지털 음원 배급 신청·계약서',
    content: lines.join('\n'),
    reviewHistory: [{ status: '접수 요청', time: at, detail: '신청서를 만들었어요. 담당자 검토가 끝나면 서명할 수 있어요.' }],
    reviewStatus: 'prepared',
  };
  if (!existing.some(d => d.kind === 'rights')) addDoc(rights);
  addDoc(agreement);
}

export type SaveState = { kind: 'idle' } | { kind: 'saving' } | { kind: 'saved'; at: string } | { kind: 'error'; message?: string };
