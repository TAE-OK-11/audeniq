import { Fragment, memo, useCallback, useEffect, useRef, useState } from 'react';
import { useNavigate, useSearchParams } from '../lib/router';
import { useToast } from '../components/Toast';
import { Modal } from '../components/Modal';
import { SignaturePad, type SignaturePadHandle } from '../components/SignaturePad';
import { useConfirm } from '../components/Confirm';
import { useProgressFill } from '../hooks/useAnimations';
import { ApiError, MOCK, api, type ArtistProfileLinks, type DspAvailability, type ReleasePayload } from '../api/client';
import { errorMessage } from '../api/errors';
import { addDoc, docsForRelease, getDocsSnapshot, useDocs, type DocRecord } from '../store/docs';
import { RightsDocumentModal } from '../components/RightsDocumentModal';
import { DocumentModal } from '../components/DocumentModal';
import { RIGHTS_DOCUMENTS, type RightsDocumentContext, type RightsDocumentKind } from '../lib/rightsDocument';
import { RERELEASE_KINDS, RERELEASE_AVAILABILITY, rereleaseIssue, rereleaseSummary, type RereleaseData } from '../lib/rerelease';
import { pushNotice } from '../store/support';
import { useProfile } from '../store/profile';
import { fileSize, localStamp } from '../lib/format';
import { DSP, GENRES, KINDS, LANGUAGES, dspLabel, genreLabel, kindLabel } from '../lib/catalog';
import { formatKoreanDate, stampNow, todayStr } from '../lib/date';
import { correctionWhere, resolveCorrection, type ResolvedCorrection } from '../lib/corrections';
import { checkAudioFile, formatDuration, specLabel } from '../lib/audioSpec';
import { AGREEMENTS, SIGNER_ROLES, compactSignature, createApplication } from '../lib/application';
import {
  artistIssue, artistWarning, coverIssue, isrcValid, minReleaseDate, mmssToSeconds, normalizeIsrc,
  profileLinkIssue, PROFILE_LINKS, releaseTypeIssue, rightsLineIssue, titleIssue, titleWarning, upcValid,
} from '../lib/dsp';
import { uid } from '../lib/store';
import { CheckIcon } from '../components/Check';
import { Glyph } from '../components/Glyph';

const STEPS = [
  { short: '발매 정보', kicker: '01 / 06 · 발매 정보', title: '어떤 음악을\n발매할까요?', sub: '발매 정보와 아티스트명을 입력해 주세요.' },
  { short: '트랙 등록', kicker: '02 / 06 · 트랙 등록', title: '발매할 곡을\n등록해 주세요.', sub: '곡별 음원 파일과 크레딧을 입력해 주세요.' },
  { short: '커버아트', kicker: '03 / 06 · 커버아트', title: '커버아트를\n등록해 주세요.', sub: '정사각형 커버아트를 등록해 주세요.' },
  { short: '배급 설정', kicker: '04 / 06 · 배급 설정', title: '언제, 어디에\n발매할까요?', sub: '발매일과 배급할 플랫폼을 선택해 주세요.' },
  { short: '권리 확인', kicker: '05 / 06 · 권리 확인', title: '음악의 권리를\n확인해 주세요.', sub: '권리자 정보와 필요한 증빙을 준비해 주세요.' },
  { short: '최종 확인', kicker: '06 / 06 · 최종 확인', title: '발매 정보를\n마지막으로 확인해 주세요.', sub: '입력한 정보와 빠진 항목을 확인해 주세요.' },
];

const RIGHTS_CHECKS: [string, string][] = [
  ['rightsMaster', '음원 마스터를 배급할 권한이 있어요.'],
  ['rightsComposition', '작사·작곡·편곡 등 저작물 이용에 필요한 허락을 확보했어요.'],
  ['rightsArtwork', '커버아트와 사용한 이미지·폰트에 필요한 이용 권한이 있어요.'],
  ['rightsConsent', '입력한 정보가 정확하며 필요한 권리 증빙을 요청받으면 제출할 수 있어요.'],
];

const CONDITIONAL_RIGHTS: [string, string, (o: ReleaseOptions) => boolean][] = [
  ['rightsSamples', '샘플링·피처링 관련 제3자 권리 허락을 확보했어요.', o => o.sample || o.featured],
  ['rightsAi', 'AI 생성·보조 제작물의 플랫폼 수용 기준을 확인했어요.', o => o.ai],
  ['rightsShared', '공동 권리자와의 배급 위임 범위를 확인했어요.', o => o.shared],
  ['rightsRerelease', '기존 발매와의 중복 송출 여부를 확인했어요.', o => o.rerelease],
];

interface Track {
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
  featuring: string;
  instrumental: boolean;
  /** 음원 규격 표기 */
  audioSpec: string;
}

/** 파일 업로드 진행 상태 (저장하지 않는 화면 전용 상태) */
interface UploadState { pct: number; state: 'uploading' | 'done' | 'error'; message?: string }

interface CoverTrackInfo {
  trackId: string;
  originalTitle: string;
  originalArtist: string;
  originalWriters: string;
}

interface ReleaseOptions extends RereleaseData {
  express: boolean; expressAck: boolean; expressReason: string;
  minor: boolean;
  guardian: string; guardianRelation: string; guardianContact: string;
  guardian2: string; guardian2Relation: string; guardian2Contact: string;
  guardianConsentDone: boolean; familyCertName: string; familyCertMethod: string;
  cover: boolean; coverTracks: CoverTrackInfo[]; coverRightsAck: boolean; coverLicenseFile: string; coverLicenseAssetId?: string;
  sample: boolean; sampleLicenseFile: string; sampleLicenseAssetId?: string;
  featured: boolean; featuredConsentFile: string; featuredConsentAssetId?: string;
  ai: boolean; aiTool: string;
  /** 버튼으로 고른 AI 활용 방식·도구 (aiTool은 이걸 합친 문장) */
  aiUses: string[]; aiTools: string[]; aiUseOther: string; aiToolOther: string;
  shared: boolean; sharedContractFile: string; sharedContractAssetId?: string;
  rerelease: boolean; previousTitle: string; previousId: string;
}

interface WizardForm {
  artist: string; title: string; type: string; language: string; genre: string;
  genreCustom: string; label: string; notes: string;
  tracks: Track[];
  coverName: string; coverData: string; coverAssetId: string;
  releaseDate: string; originalDate: string; upc: string;
  territories: string[]; platforms: string[];
  ownership: string; phonogram: string; copyright: string;
  rightsChecks: Record<string, boolean>;
  options: ReleaseOptions;
  artistProfile: ArtistProfileLinks;
}

const newTrack = (): Track => ({
  id: uid('t'),
  title: '', version: '', isrc: '', composers: '', lyricists: '', arrangers: '', performers: '',
  producer: '',
  lyrics: '',
  audioName: '', audioSize: 0, explicit: false, duration: '',
  assetId: '', serverId: '', featuring: '', instrumental: false, audioSpec: '',
});

const EMPTY_OPTIONS: ReleaseOptions = {
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
};

const EMPTY: WizardForm = {
  artist: '', title: '', type: 'single', language: 'ko', genre: '', genreCustom: '',
  label: '', notes: '',
  tracks: [newTrack()],
  coverName: '', coverData: '', coverAssetId: '',
  releaseDate: '', originalDate: '', upc: '',
  territories: ['WORLD'], platforms: MOCK ? DSP.map(d => d[0]) : [],
  ownership: '', phonogram: '', copyright: '',
  rightsChecks: {},
  options: EMPTY_OPTIONS,
  artistProfile: { isNew: true, spotify: '', apple: '', melon: '' },
};

const SERVICE_OPTIONS: [keyof ReleaseOptions, string, string][] = [
  ['express', '신속 발매 요청', '희망 일정과 우선 검토 가능 여부를 확인해요.'],
];

const RIGHTS_OPTIONS: [keyof ReleaseOptions, string, string][] = [
  ['minor', '미성년 아티스트·권리자', '법정대리인의 동의와 권한 확인이 필요해요.'],
  ['cover', '커버곡', '원곡의 작사·작곡 저작물 이용 권한을 확인해요.'],
  ['sample', '샘플링·타인 음원 사용', '원본 음원과 저작물의 이용 허락을 확인해요.'],
  ['featured', '피처링·공동 실연', '참여자 크레딧 및 필요한 이용 허락을 확인해요.'],
  ['ai', 'AI 생성·보조 제작', '제작 방식과 각 플랫폼의 수용 기준을 검토해요.'],
  ['shared', '공동 권리자·레이블 계약', '각 권리자와 배급 위임 범위를 확인해요.'],
  ['rerelease', '기존 발매 이전·재발매', '이전 발매의 음반·음원 코드와 중복 발매 여부를 확인해요.'],
];

const OPTIONS_CATALOG: [keyof ReleaseOptions, string, string][] = [
  ...SERVICE_OPTIONS,
  ...RIGHTS_OPTIONS,
];

function rightsOk(f: WizardForm): boolean {
  if (!RIGHTS_CHECKS.every(([k]) => f.rightsChecks[k])) return false;
  return CONDITIONAL_RIGHTS
    .filter(([, , cond]) => cond(f.options))
    .every(([k]) => f.rightsChecks[k]);
}

function applicableConditionals(o: ReleaseOptions): [string, string][] {
  return CONDITIONAL_RIGHTS
    .filter(([, , cond]) => cond(o))
    .map(([k, label]) => [k, label]);
}

function KoreanDateField({ id, label, required, value, min, onChange }: {
  id: string; label: React.ReactNode; required?: boolean;
  value: string; min?: string; onChange: (v: string) => void;
}) {
  return (
    <div className="field">
      <label htmlFor={id}>{label}{required && <> <span className="required">*</span></>}</label>
      <div className="kdate-wrap">
        <input
          id={id} type="date" className="kdate-input"
          value={value} min={min} onChange={e => onChange(e.target.value)}
        />
        <div className={`kdate-display${value ? '' : ' empty'}`} aria-hidden="true">
          {value ? formatKoreanDate(value) : '날짜를 선택해 주세요.'}
        </div>
      </div>
    </div>
  );
}

function BackIcon() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" width="24" height="24" fill="none"
      stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="m15 18-6-6 6-6" />
    </svg>
  );
}

function selectedOptions(o: ReleaseOptions): [keyof ReleaseOptions, string, string][] {
  return OPTIONS_CATALOG.filter(([id]) => !!o[id]);
}

function DocAttach({ id, label, fileName, assetId, busy, onSelect, required, help, kind, releaseId, onElectronic, onOpenDoc }: {
  id: string; label: React.ReactNode; fileName: string;
  assetId?: string; busy?: boolean;
  onSelect: (file: File) => void; required?: boolean; help?: string;
  kind: RightsDocumentKind; releaseId?: string; onElectronic: (kind: RightsDocumentKind) => void; onOpenDoc: (id: string) => void;
}) {
  const [method, setMethod] = useState<'electronic' | 'upload'>(fileName ? 'upload' : 'electronic');
  const docs = useDocs().filter(d => d.releaseId === releaseId && d.electronic?.document_kind === kind);
  return (
    <div className="field doc-attach aq-rights-evidence">
      <strong>{label}{required && <> <span className="required">*</span></>}</strong>
      <div className="aq-chips" role="group" aria-label="서류 준비 방법">
        <button type="button" className={`aq-chip${method === 'electronic' ? ' is-on' : ''}`} aria-pressed={method === 'electronic'} onClick={() => setMethod('electronic')}>AUDENIQ에서 작성</button>
        <button type="button" className={`aq-chip${method === 'upload' ? ' is-on' : ''}`} aria-pressed={method === 'upload'} onClick={() => setMethod('upload')}>보유한 서류 첨부</button>
      </div>
      {method === 'electronic' ? <>
        <p className="help">발매 정보로 {RIGHTS_DOCUMENTS[kind].title}를 만들고 권리자가 직접 서명하면 문서가 완성돼요.</p>
        <button type="button" className="button secondary" onClick={() => onElectronic(kind)}>전자 문서 작성{docs.length ? ' · 권리자 추가' : '하기'}</button>
      </> : <><label className="sr-only" htmlFor={id}>{label}</label><input
        type="file" id={id}
        accept=".pdf,.png,.jpg,.jpeg,application/pdf,image/png,image/jpeg"
        onChange={e => {
          const f = e.target.files?.[0];
          if (f) onSelect(f);
          e.target.value = '';
        }}
      />
      <p className="help">
        {busy ? '서류를 서버에 올리는 중이에요…' : fileName ? (assetId ? `서버에 등록됨 · ${fileName}` : `재첨부 필요 · ${fileName}`) : (help || '서류를 첨부해 주세요.')}
      </p></>}
      {docs.map(d => <button key={d.id} type="button" className="aq-rights-receipt" onClick={() => onOpenDoc(d.id)}><CheckIcon size={16} /><span>{d.electronic?.rights_holder} · {d.reviewStatus === 'needs' ? '보완 요청' : '서명 완료'}<small>{d.reviewStatus === 'approved' ? 'AUDENIQ 검토 승인' : d.reviewStatus === 'needs' ? d.reviewNote : 'AUDENIQ 검토 중'} · 문서 보기</small></span></button>)}
    </div>
  );
}

const certSvg = (d: string) => (
  <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={d} /></svg>
);
const CERT_METHODS: { key: string; label: string; sub: string; icon: React.ReactNode }[] = [
  { key: 'camera', label: '직접 촬영', sub: '카메라로 찍기', icon: certSvg('M4 8h3l2-3h6l2 3h3a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V9a1 1 0 0 1 1-1Zm8 9a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Z') },
  { key: 'pdf', label: 'PDF 선택', sub: '파일에서 고르기', icon: certSvg('M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8Zm0 0v5h5M9 13h6M9 17h4') },
  { key: 'wallet', label: '전자문서지갑', sub: '연동 준비 중', icon: certSvg('M3 7a2 2 0 0 1 2-2h13v4M3 7v10a2 2 0 0 0 2 2h15V9H5a2 2 0 0 1-2-2Zm13 7h.01') },
];

function GuardianConsentModal({ guardianName, onClose, onComplete }: {
  guardianName: string;
  onClose: () => void;
  onComplete: (certName: string, certMethod: string) => void;
}) {
  const padRef = useRef<SignaturePadHandle>(null);
  const [signed, setSigned] = useState(false);
  const [certName, setCertName] = useState('');
  const [certMethod, setCertMethod] = useState('');
  const [ack, setAck] = useState(false);
  const cameraRef = useRef<HTMLInputElement>(null);
  const pdfRef = useRef<HTMLInputElement>(null);

  const pick = (method: string) => {
    if (method === 'camera') cameraRef.current?.click();
    else if (method === 'pdf') pdfRef.current?.click();
    else { setCertMethod('wallet'); setCertName('전자문서지갑에서 가져옴'); }
  };
  const onFile = (method: string) => (e: React.ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0];
    if (f) { setCertMethod(method); setCertName(f.name); }
    e.target.value = '';
  };

  const missing = !ack ? '동의 항목을 체크해 주세요.' : !signed ? '법정대리인 서명을 입력해 주세요.' : !certName ? '가족관계증명서를 제출해 주세요.' : '';

  return (
    <Modal title="법정대리인 동의" onClose={onClose} dismissible={false} modalClass="aq-guardian-mode">
      <div className="guardian-modal-body">
        <p className="guardian-consent-text">
          미성년 아티스트의 음원 발매에 대해 법정대리인{guardianName ? <strong> {guardianName}</strong> : ''}님의 동의를 확인해요.
          아래 순서대로 진행해 주세요.
        </p>

        <ol className="aq-guardian-steps">
          <li className={ack ? 'is-done' : ''}>
            <span className="aq-guardian-no" aria-hidden="true">{ack ? <CheckIcon size={13} /> : 1}</span>
            <label className="aq-guardian-agree">
              <input type="checkbox" checked={ack} onChange={e => setAck(e.target.checked)} />
              <span>미성년자의 음원 발매와 배급에 법정대리인으로서 동의해요.<small>동의 내용은 발매 심사 때 확인돼요.</small></span>
            </label>
          </li>
          <li className={signed ? 'is-done' : ''}>
            <span className="aq-guardian-no" aria-hidden="true">{signed ? <CheckIcon size={13} /> : 2}</span>
            <div className="min-0">
              <strong className="aq-guardian-label">법정대리인 전자서명 <span className="required">*</span></strong>
              <SignaturePad ref={padRef} id="aqGuardianPad" label="법정대리인 서명 입력" height={170} onChange={setSigned} />
            </div>
          </li>
          <li className={certName ? 'is-done' : ''}>
            <span className="aq-guardian-no" aria-hidden="true">{certName ? <CheckIcon size={13} /> : 3}</span>
            <div className="min-0">
              <strong className="aq-guardian-label">가족관계증명서 <span className="required">*</span></strong>
              <div className="aq-cert-methods" role="radiogroup" aria-label="가족관계증명서 제출 방법">
                {CERT_METHODS.map(m => (
                  <button
                    key={m.key} type="button" role="radio" aria-checked={certMethod === m.key}
                    className={`aq-cert-method${certMethod === m.key ? ' is-active' : ''}`}
                    onClick={() => pick(m.key)}
                  >
                    <span className="aq-cert-icon">{m.icon}</span>
                    <strong>{m.label}</strong>
                    <small>{m.sub}</small>
                  </button>
                ))}
              </div>
              <input ref={cameraRef} type="file" accept="image/*" capture="environment" hidden onChange={onFile('camera')} />
              <input ref={pdfRef} type="file" accept=".pdf,application/pdf" hidden onChange={onFile('pdf')} />
              {certName && (
                <p className="aq-cert-picked">
                  <span className="aq-check-badge is-sm"><CheckIcon size={10} /></span> {certName}
                  {certMethod === 'wallet' && <small>전자문서지갑 연동 후 원본을 가져와요. 지금은 선택 상태로 저장돼요.</small>}
                </p>
              )}
            </div>
          </li>
        </ol>
      </div>
      <div className="aq-modal-foot aq-guardian-foot">
        <p className="aq-guardian-missing" aria-live="polite">{missing}</p>
        <button type="button" className="button secondary" onClick={onClose}>취소</button>
        <button type="button" className="button" disabled={!!missing} onClick={() => onComplete(certName, certMethod)}>동의 완료</button>
      </div>
    </Modal>
  );
}

/** 플랫폼 구분용 색 (로고 대신 첫 글자 배지) */
const DSP_COLOR: Record<string, string> = {
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
const DSP_GROUPS: [string, (d: { region: string; category?: string }) => boolean][] = [
  ['국내', d => d.region === 'KR'],
  ['해외 스트리밍', d => d.region !== 'KR' && (d.category ?? 'STREAMING') === 'STREAMING'],
  ['소셜·숏폼 영상', d => d.category === 'SOCIAL'],
  ['스토어', d => d.category === 'STORE'],
  ['피트니스·크리에이터·게임 라이선스', d => d.category === 'LICENSING'],
];

const OTHER = '기타';
const AI_USES = [
  '전체를 AI로 제작했어요', '작곡(멜로디)에 AI 도움을 받았어요', '작사에 AI 도움을 받았어요', 'AI로 보컬(목소리)을 만들었어요',
  '편곡·반주에 AI를 사용했어요', '믹싱·마스터링에 AI 도구를 썼어요', '커버아트를 AI로 만들었어요',
];
const AI_TOOLS = ['Suno', 'Udio', 'ChatGPT', 'Stable Audio', 'AIVA', 'Midjourney'];
const EXPRESS_REASONS = ['공연 일정에 맞춰야 해요', '방송·광고 일정이 있어요', '이벤트·프로모션 일정이 있어요', '영상·드라마 공개일에 맞춰야 해요'];

/** 버튼으로 고르는 선택지 — 직접 타이핑은 ‘기타’를 눌렀을 때만 */
function ChipPicker({ id, options, value, onChange, other, onOther, otherPlaceholder, single }: {
  id: string; options: string[]; value: string[]; onChange: (v: string[]) => void;
  other?: string; onOther?: (v: string) => void; otherPlaceholder?: string; single?: boolean;
}) {
  const toggle = (opt: string) => {
    const on = value.includes(opt);
    onChange(single ? (on ? [] : [opt]) : on ? value.filter(v => v !== opt) : [...value, opt]);
  };
  const all = onOther ? [...options, OTHER] : options;
  return (
    <div className="aq-chip-picker" id={id}>
      <div className="aq-chips" role="group">
        {all.map(opt => (
          <button key={opt} type="button" className={`aq-chip${value.includes(opt) ? ' is-on' : ''}`} aria-pressed={value.includes(opt)} onClick={() => toggle(opt)}>
            {value.includes(opt) && <CheckIcon size={12} className="aq-chip-check" />}{opt === OTHER ? '기타 (직접 입력)' : opt}
          </button>
        ))}
      </div>
      {onOther && value.includes(OTHER) && (
        <input className="aq-chip-other" maxLength={300} value={other ?? ''} placeholder={otherPlaceholder} onChange={e => onOther(e.target.value)} autoFocus />
      )}
    </div>
  );
}

/** 고른 버튼을 심사용 문장으로 합친다 */
function aiSummary(o: Pick<ReleaseOptions, 'aiUses' | 'aiTools' | 'aiUseOther' | 'aiToolOther'>): string {
  const uses = o.aiUses.map(u => (u === OTHER ? o.aiUseOther.trim() : u)).filter(Boolean);
  const tools = o.aiTools.map(t => (t === OTHER ? o.aiToolOther.trim() : t)).filter(Boolean);
  return [uses.join(' · '), tools.length ? `도구: ${tools.join(', ')}` : ''].filter(Boolean).join(' / ');
}

function OptionsSection({ form, set, group, onDocument, uploads, onElectronic, onOpenDoc, releaseId }: {
  form: WizardForm;
  group: 'service' | 'rights';
  set: <K extends keyof WizardForm>(key: K, value: WizardForm[K]) => void;
  onDocument: (name: keyof ReleaseOptions, asset: keyof ReleaseOptions, file: File) => void;
  uploads: Record<string, UploadState>;
  onElectronic: (kind: RightsDocumentKind) => void; onOpenDoc: (id: string) => void; releaseId?: string;
}) {
  const o = form.options;
  const setOpt = <K extends keyof ReleaseOptions>(key: K, value: ReleaseOptions[K]) =>
    set('options', { ...o, [key]: value });
  const [showGuardianModal, setShowGuardianModal] = useState(false);
  const [expressOther, setExpressOther] = useState(false);
  // 예전 신청서(직접 입력한 aiTool만 있음)는 ‘기타’로 보여 준다
  const aiUses = o.aiUses?.length ? o.aiUses : o.aiTool ? [OTHER] : [];
  const aiUseOther = o.aiUses?.length ? o.aiUseOther ?? '' : o.aiTool;
  const setAi = (patch: Partial<ReleaseOptions>) => {
    const next = { aiUses, aiTools: o.aiTools ?? [], aiUseOther, aiToolOther: o.aiToolOther ?? '', ...patch };
    set('options', { ...o, ...next, aiTool: aiSummary(next) });
  };
  const expressPick = !o.expressReason ? (expressOther ? [OTHER] : []) : EXPRESS_REASONS.includes(o.expressReason) ? [o.expressReason] : [OTHER];

  // 선택한 카드 바로 밑에 여는 입력 (목록 맨 아래가 아니라)
  const detail: Partial<Record<keyof ReleaseOptions, React.ReactNode>> = {
    express: (
      <div className="aq-option-detail">
        <h3>신속 발매 요청</h3>
        <p className="aq-option-intro">신속 발매는 가능 여부와 조건을 확인한 뒤 진행해요. 특정 발매일이나 플랫폼 송출을 보장하지 않아요.</p>
        <div className="field">
          <label htmlFor="aqExpressReason">신속 발매가 필요한 이유 (선택)</label>
          <ChipPicker
            id="aqExpressReason" single options={EXPRESS_REASONS} value={expressPick}
            onChange={v => { setExpressOther(v[0] === OTHER); setOpt('expressReason', v[0] && v[0] !== OTHER ? v[0] : ''); }}
            other={EXPRESS_REASONS.includes(o.expressReason) ? '' : o.expressReason} onOther={v => setOpt('expressReason', v)}
            otherPlaceholder="신속 발매가 필요한 이유"
          />
        </div>
        <label className="check-line">
          <input
            type="checkbox" id="aqExpressAck"
            checked={o.expressAck}
            onChange={e => setOpt('expressAck', e.target.checked)}
          />
          <span>가능한 일정과 비용 등 별도 안내를 확인한 후 진행할게요.<small>신청 단계에서 추가 비용이 자동 결제되지는 않아요.</small></span>
        </label>
      </div>
    ),
    minor: (
      <div className="aq-option-detail">
        <h3>법정대리인 확인</h3>
        <p className="aq-option-intro">본인 및 권리자의 동의 범위를 확인할 수 있도록 보호자 정보를 입력해 주세요. 법정대리인은 2명을 입력하는 것이 원칙이지만, 1명인 경우도 접수할 수 있어요.</p>
        <h4 className="aq-guardian-head">법정대리인 1</h4>
        <div className="field">
          <label htmlFor="aqGuardian">법정대리인 성명 <span className="required">*</span></label>
          <input
            id="aqGuardian" autoComplete="name" maxLength={90} value={o.guardian}
            onChange={e => setOpt('guardian', e.target.value)}
            placeholder="법정대리인 성명"
          />
        </div>
        <div className="field">
          <label htmlFor="aqGuardianRelation">아티스트와의 관계 <span className="required">*</span></label>
          <select
            id="aqGuardianRelation" value={o.guardianRelation}
            onChange={e => setOpt('guardianRelation', e.target.value)}
          >
            <option value="">관계를 선택해 주세요.</option>
            {['부', '모', '기타 법정대리인'].map(x => (
              <option key={x} value={x}>{x}</option>
            ))}
          </select>
        </div>
        <div className="field">
          <label htmlFor="aqGuardianContact">법정대리인 연락처 또는 이메일 <span className="required">*</span></label>
          <input
            id="aqGuardianContact" maxLength={160} value={o.guardianContact}
            onChange={e => setOpt('guardianContact', e.target.value)}
            placeholder="확인이 가능한 연락처"
          />
        </div>
        <h4 className="aq-guardian-head">법정대리인 2 <small>(해당하는 경우)</small></h4>
        <div className="field">
          <label htmlFor="aqGuardian2">법정대리인 성명</label>
          <input
            id="aqGuardian2" autoComplete="name" maxLength={90} value={o.guardian2}
            onChange={e => setOpt('guardian2', e.target.value)}
            placeholder="법정대리인 성명"
          />
        </div>
        <div className="field">
          <label htmlFor="aqGuardian2Relation">아티스트와의 관계</label>
          <select
            id="aqGuardian2Relation" value={o.guardian2Relation}
            onChange={e => setOpt('guardian2Relation', e.target.value)}
          >
            <option value="">관계를 선택해 주세요.</option>
            {['부', '모', '기타 법정대리인'].map(x => (
              <option key={x} value={x}>{x}</option>
            ))}
          </select>
        </div>
        <div className="field">
          <label htmlFor="aqGuardian2Contact">법정대리인 연락처 또는 이메일</label>
          <input
            id="aqGuardian2Contact" maxLength={160} value={o.guardian2Contact}
            onChange={e => setOpt('guardian2Contact', e.target.value)}
            placeholder="확인이 가능한 연락처"
          />
        </div>
        <div className="field">
          <label>법정대리인 동의</label>
          <button
            type="button" className="button secondary" style={{ width: '100%' }}
            onClick={() => setShowGuardianModal(true)}
          >
            {o.guardianConsentDone ? '법정대리인 동의 완료 · 다시 진행' : '법정대리인 동의 진행하기'}
          </button>
          <p className="help">
            {o.guardianConsentDone
              ? `동의 완료${o.familyCertName ? ` · 가족관계증명서: ${o.familyCertName}` : ''}`
              : '동의창에서 법정대리인 서명과 가족관계증명서를 제출해요.'}
          </p>
        </div>
        <p className="aq-option-note">법정대리인 정보 입력만으로 본인 확인이나 동의 검증이 완료되지는 않아요. 담당자 확인 후 서명 단계를 안내해요.</p>
      </div>
    ),
    cover: (
      <div className="aq-option-detail">
        <h3>커버곡 정보</h3>
        <p className="aq-option-intro">커버한 트랙을 선택하고, 원곡 정보를 입력해 주세요.</p>
        {form.tracks.map((t, i) => {
          const info = o.coverTracks.find(c => c.trackId === t.id);
          const toggleCover = (checked: boolean) => {
            setOpt('coverTracks', checked
              ? [...o.coverTracks, { trackId: t.id, originalTitle: '', originalArtist: '', originalWriters: '' }]
              : o.coverTracks.filter(c => c.trackId !== t.id));
          };
          const setCoverInfo = (key: keyof Omit<CoverTrackInfo, 'trackId'>, value: string) => {
            setOpt('coverTracks', o.coverTracks.map(c =>
              c.trackId === t.id ? { ...c, [key]: value } : c));
          };
          return (
            <div key={t.id} className="cover-track-item">
              <label className="check-line">
                <input
                  type="checkbox" checked={!!info}
                  onChange={e => toggleCover(e.target.checked)}
                />
                <span><strong>트랙 {i + 1} · {t.title.trim() || '(제목 없음)'}</strong></span>
              </label>
              {info && (
                <div className="cover-track-fields">
                  <div className="field">
                    <label htmlFor={`cover-orig-title-${i}`}>원곡 제목 <span className="required">*</span></label>
                    <input
                      id={`cover-orig-title-${i}`} maxLength={200} value={info.originalTitle}
                      onChange={e => setCoverInfo('originalTitle', e.target.value)}
                      placeholder="원곡의 제목"
                    />
                  </div>
                  <div className="field">
                    <label htmlFor={`cover-orig-artist-${i}`}>원곡 아티스트 <span className="required">*</span></label>
                    <input
                      id={`cover-orig-artist-${i}`} maxLength={200} value={info.originalArtist}
                      onChange={e => setCoverInfo('originalArtist', e.target.value)}
                      placeholder="원곡자 또는 원 아티스트"
                    />
                  </div>
                  <div className="field">
                    <label htmlFor={`cover-orig-writers-${i}`}>원곡 작사 / 작곡</label>
                    <input
                      id={`cover-orig-writers-${i}`} maxLength={200} value={info.originalWriters}
                      onChange={e => setCoverInfo('originalWriters', e.target.value)}
                      placeholder="알고 있다면 입력"
                    />
                  </div>
                </div>
              )}
            </div>
          );
        })}
        <label className="check-line" style={{ marginTop: 12 }}>
          <input
            type="checkbox" id="aqCoverRightsAck"
            checked={o.coverRightsAck}
            onChange={e => setOpt('coverRightsAck', e.target.checked)}
          />
          <span>원곡 저작권자의 이용 허락을 받았거나, 정당한 라이선스 절차를 진행할 것을 확인해요.<small>허락 없는 커버곡 배급은 저작권 침해가 될 수 있어요.</small></span>
        </label>
        <DocAttach
          kind="composition" releaseId={releaseId} onElectronic={onElectronic} onOpenDoc={onOpenDoc}
          id="aqCoverLicense" label="원곡 이용 허락서"
          fileName={o.coverLicenseFile} assetId={o.coverLicenseAssetId} busy={uploads.coverLicenseFile?.state === 'uploading'}
          onSelect={file => onDocument('coverLicenseFile', 'coverLicenseAssetId', file)}
          help="이용 허락서나 라이선스 계약서를 첨부해 주세요."
        />
      </div>
    ),
    sample: (
      <div className="aq-option-detail">
        <h3>샘플링·타인 음원 사용</h3>
        <p className="aq-option-intro">사용한 원본 음원과 저작물의 출처를 확인해요.</p>
        <DocAttach
          kind="sample" releaseId={releaseId} onElectronic={onElectronic} onOpenDoc={onOpenDoc}
          id="aqSampleLicense" label="원본 이용 허락서"
          fileName={o.sampleLicenseFile} assetId={o.sampleLicenseAssetId} busy={uploads.sampleLicenseFile?.state === 'uploading'}
          onSelect={file => onDocument('sampleLicenseFile', 'sampleLicenseAssetId', file)}
          help="샘플링 원본의 이용 허락서나 라이선스 계약서를 첨부해 주세요."
        />
        <p className="aq-option-note">허락 없는 샘플링은 저작권 침해가 될 수 있어요.</p>
      </div>
    ),
    featured: (
      <div className="aq-option-detail">
        <h3>피처링·공동 실연</h3>
        <p className="aq-option-intro">참여자의 크레딧과 이용 허락을 확인해요.</p>
        <DocAttach
          kind="performer" releaseId={releaseId} onElectronic={onElectronic} onOpenDoc={onOpenDoc}
          id="aqFeaturedConsent" label="참여자 동의서 (보유 시)"
          fileName={o.featuredConsentFile} assetId={o.featuredConsentAssetId} busy={uploads.featuredConsentFile?.state === 'uploading'}
          onSelect={file => onDocument('featuredConsentFile', 'featuredConsentAssetId', file)}
          help="피처링 참여자의 동의서나 계약서를 첨부해 주세요."
        />
      </div>
    ),
    shared: (
      <div className="aq-option-detail">
        <h3>공동 권리자·레이블 계약</h3>
        <p className="aq-option-intro">각 권리자와의 배급 위임 범위를 확인해요.</p>
        <DocAttach
          kind="shared" releaseId={releaseId} onElectronic={onElectronic} onOpenDoc={onOpenDoc}
          id="aqSharedContract" label="공동 권리 계약서"
          fileName={o.sharedContractFile} assetId={o.sharedContractAssetId} busy={uploads.sharedContractFile?.state === 'uploading'}
          onSelect={file => onDocument('sharedContractFile', 'sharedContractAssetId', file)}
          help="배급 위임 범위가 명시된 계약서를 첨부해 주세요."
        />
      </div>
    ),
    rerelease: (
      <div className="aq-option-detail">
        <h3>기존 발매 이전·재발매</h3>
        <p className="aq-option-intro">현재 상황을 고르면 기존 코드와 서비스 이전에 필요한 내용을 안내해 드려요.</p>
        <div className="field">
          <label>어떤 상황인가요? <span className="required">*</span></label>
          <ChipPicker id="aqRereleaseKind" single options={Object.values(RERELEASE_KINDS)} value={o.rereleaseKind ? [RERELEASE_KINDS[o.rereleaseKind]] : []}
            onChange={v => set('options', { ...o, rereleaseKind: (Object.entries(RERELEASE_KINDS).find(([, label]) => label === v[0])?.[0] ?? '') as RereleaseData['rereleaseKind'], rereleaseAck: false })} />
        </div>
        <div className="field">
          <label htmlFor="aqPreviousTitle">기존 발매명 <span className="required">*</span></label>
          <input
            id="aqPreviousTitle" maxLength={180} value={o.previousTitle}
            onChange={e => setOpt('previousTitle', e.target.value)}
            placeholder="기존 앨범·싱글 제목"
          />
        </div>
        <div className="field">
          <label htmlFor="aqPreviousDistributor">이전 유통사·레이블 (알고 있다면)</label>
          <input id="aqPreviousDistributor" maxLength={160} value={o.previousDistributor ?? ''} onChange={e => setOpt('previousDistributor', e.target.value)} placeholder="기존 배급을 맡은 곳" />
        </div>
        <div className="field">
          <label htmlFor="aqPreviousUrl">기존 발매 링크 (보유 시)</label>
          <input id="aqPreviousUrl" type="url" maxLength={500} value={o.previousUrl ?? ''} onChange={e => setOpt('previousUrl', e.target.value)} placeholder="https://…" />
        </div>
        <KoreanDateField
          id="aqOriginalDate" label={o.rereleaseKind === 'new_version' ? '이전 버전의 최초 발매일' : '최초 발매일'} required
          value={form.originalDate}
          onChange={v => set('originalDate', v)}
        />
        <div className="field"><label>기존 음원은 지금 서비스 중인가요? <span className="required">*</span></label>
          <ChipPicker id="aqPreviousAvailability" single options={Object.values(RERELEASE_AVAILABILITY)} value={o.previousAvailability ? [RERELEASE_AVAILABILITY[o.previousAvailability]] : []}
            onChange={v => set('options', { ...o, previousAvailability: (Object.entries(RERELEASE_AVAILABILITY).find(([, label]) => label === v[0])?.[0] ?? '') as RereleaseData['previousAvailability'], rereleaseAck: false })} />
        </div>
        <div className="field"><label htmlFor="aqRereleaseAudio">이번 음원은 기존 녹음과 같은가요? <span className="required">*</span></label>
          <select id="aqRereleaseAudio" value={o.rereleaseAudio ?? ''} onChange={e => set('options', { ...o, rereleaseAudio: e.target.value as RereleaseData['rereleaseAudio'], rereleaseAck: false })}>
            <option value="">선택해 주세요</option><option value="same">같은 녹음 · 음원 내용 변경 없음</option><option value="changed">새 녹음·리믹스 등 음악 내용 변경</option><option value="unknown">리마스터 등 변경 여부 확인 필요</option>
          </select>
        </div>
        {o.rereleaseAudio && <div className="aq-option-note" role="status">
          {o.rereleaseAudio === 'same' ? '같은 녹음은 기존 ISRC를 유지해요. 제목·아티스트·길이 등 기존 정보를 맞춰 주세요. 스트리밍 수와 플레이리스트 연결 결과는 플랫폼에서 결정해요.'
            : o.rereleaseAudio === 'changed' ? '새 녹음·리믹스처럼 음악 내용이 달라지면 새로운 ISRC가 필요해요. 기존 코드는 비교용으로 보관하고 새 음원에 적용하지 않아요.'
              : '단순 음량 조정과 음악 내용 변경은 코드 처리 방식이 달라요. 변경 내용을 아래에 적고 문의 메뉴에서 확인해 주세요. 확인 전에는 임시 저장해 둘 수 있어요.'}
        </div>}
        <div className="field"><label htmlFor="aqPreviousUpc">기존 음반 코드 UPC (보유 시)</label>
          <input id="aqPreviousUpc" inputMode="numeric" maxLength={14} value={o.previousUpc ?? ''} onChange={e => setOpt('previousUpc', e.target.value)} placeholder="기존 앨범·싱글의 UPC" />
          {o.rereleaseKind === 'transfer' && o.rereleaseAudio === 'same' && o.previousUpc && <button type="button" className="link-btn" onClick={() => set('upc', o.previousUpc ?? '')}>동일한 음반의 UPC를 배급 설정에 적용</button>}
          <p className="help">트랙 구성이나 버전이 다른 음반은 새 UPC를 사용해요. 기존 UPC는 이전 유통사에서 확인할 수 있어요.</p>
        </div>
        <h4>트랙별 기존 ISRC</h4>
        {form.tracks.map((t, i) => <div key={t.id} className="field">
          <label htmlFor={`aqPreviousIsrc-${i}`}>{i + 1}. {t.title || '제목 없는 트랙'} · 기존 ISRC{o.rereleaseAudio === 'same' && <> <span className="required">*</span></>}</label>
          <input id={`aqPreviousIsrc-${i}`} maxLength={15} value={o.rereleaseTracks?.find(x => x.trackId === t.id)?.previousIsrc ?? (i === 0 ? o.previousId : '')}
            onChange={e => setOpt('rereleaseTracks', form.tracks.map((track, k) => ({ trackId: track.id, previousIsrc: track.id === t.id ? e.target.value : o.rereleaseTracks?.find(x => x.trackId === track.id)?.previousIsrc ?? (k === 0 ? o.previousId : '') })))} placeholder="예: KR-ABC-26-00001" />
        </div>)}
        {o.rereleaseAudio === 'same' && <button type="button" className="button secondary" onClick={() => {
          const previous = form.tracks.map((t, i) => ({ trackId: t.id, previousIsrc: o.rereleaseTracks?.find(x => x.trackId === t.id)?.previousIsrc ?? (i === 0 ? o.previousId : '') }));
          set('tracks', form.tracks.map(t => ({ ...t, isrc: previous.find(x => x.trackId === t.id)?.previousIsrc || t.isrc })));
          setOpt('rereleaseTracks', previous);
        }}>기존 ISRC를 트랙 등록에 적용</button>}
        <p className="help">ISRC를 모르면 이전 유통사의 발매 내역에서 확인해 주세요. 같은 녹음에 새 코드를 발급하지 않도록 확인 후 접수해요.</p>
        <div className="field"><label htmlFor="aqRereleaseRights">기존 계약과 이번 배급 권한 <span className="required">*</span></label>
          <select id="aqRereleaseRights" value={o.rereleaseRights ?? ''} onChange={e => setOpt('rereleaseRights', e.target.value as RereleaseData['rereleaseRights'])}>
            <option value="">선택해 주세요</option><option value="owned">내가 권리자이며 이전 계약의 제한을 확인했어요</option><option value="permission">권리자·레이블로부터 이번 배급 허락을 받았어요</option><option value="pending">계약 또는 이전 허락을 확인 중이에요</option>
          </select>
        </div>
        {o.rereleaseRights === 'permission' && <DocAttach kind="master" releaseId={releaseId} onElectronic={onElectronic} onOpenDoc={onOpenDoc}
          id="aqRereleasePermission" label="이번 배급 이용 허락서" fileName={o.rereleasePermissionFile ?? ''} assetId={o.rereleasePermissionAssetId} busy={uploads.rereleasePermissionFile?.state === 'uploading'} onSelect={file => onDocument('rereleasePermissionFile', 'rereleasePermissionAssetId', file)} />}
        <div className="field"><label htmlFor="aqRereleaseNotes">변경 사항·이전 일정·확인이 필요한 내용</label><textarea id="aqRereleaseNotes" maxLength={1200} rows={3} value={o.rereleaseNotes ?? ''} onChange={e => setOpt('rereleaseNotes', e.target.value)} placeholder="예: 기존 서비스 종료 요청일, 리마스터 변경 내용, 이전 계약 확인 상황" /></div>
        {(o.previousAvailability === 'live' || o.previousAvailability === 'takedown_requested') && <p className="aq-option-note">기존 서비스가 남아 있어요. 이전 유통사와 종료·이전 일정을 조율하고, 새 서비스 연결을 확인한 뒤 중복 송출을 정리해 주세요. AUDENIQ 신청만으로 기존 서비스가 내려가지는 않아요.</p>}
        <label className="check-line"><input id="aqRereleaseAck" type="checkbox" checked={!!o.rereleaseAck} onChange={e => setOpt('rereleaseAck', e.target.checked)} /><span>기존 계약·음원 코드·서비스 상태를 확인했고, 서비스 이전 일정과 플랫폼 연결 결과는 별도로 확인할게요.</span></label>
        <p className="help">코드 안내 기준: <a href="https://isrc.ifpi.org/why-use-isrc/when-to-assign" target="_blank" rel="noreferrer">IFPI ISRC</a> · <a href="https://support.spotify.com/sc-en/artists/article/re-uploading-music/" target="_blank" rel="noreferrer">Spotify 재업로드 안내</a></p>
      </div>
    ),
    ai: (
      <div className="aq-option-detail">
        <h3>AI 활용 내역</h3>
        <div className="field">
          <label htmlFor="aqAiTool">어떻게 활용했나요? <span className="required">*</span> <small className="muted">여러 개 고를 수 있어요</small></label>
          <ChipPicker
            id="aqAiTool" options={AI_USES} value={aiUses} onChange={v => setAi({ aiUses: v })}
            other={aiUseOther} onOther={v => setAi({ aiUseOther: v })} otherPlaceholder="AI를 활용한 부분을 적어 주세요"
          />
        </div>
        <div className="field">
          <label htmlFor="aqAiTools">사용한 도구 <small className="muted">(선택)</small></label>
          <ChipPicker
            id="aqAiTools" options={AI_TOOLS} value={o.aiTools ?? []} onChange={v => setAi({ aiTools: v })}
            other={o.aiToolOther ?? ''} onOther={v => setAi({ aiToolOther: v })} otherPlaceholder="도구 이름"
          />
        </div>
        <p className="help">이용 약관과 상업적 이용 허가 자료를 요청할 수 있어요. 모든 배급 플랫폼이 AI 음악을 받는 것은 아니에요.</p>
      </div>
    ),
  };
  const list = group === 'service' ? SERVICE_OPTIONS : RIGHTS_OPTIONS;

  return (
    <>
      {group === 'service' ? (
        <>
          <h2 className="subhead" style={{ marginTop: 25 }}>부가서비스</h2>
          <p className="aq-option-intro">필요한 서비스를 선택해 주세요.</p>
        </>
      ) : (
        <>
          <h2 className="subhead" style={{ marginTop: 25 }}>해당하는 항목</h2>
          <p className="aq-option-intro">해당하는 항목을 모두 선택해 주세요. 누르면 바로 아래에 필요한 확인 사항이 열려요.</p>
        </>
      )}
      <div className="aq-options">
        {list.map(([id, title, sub]) => (
          <Fragment key={id}>
            <label className={`aq-option${o[id] ? ' is-on' : ''}${!MOCK && id === 'minor' ? ' is-unavailable' : ''}`}>
              <span className="aq-option-text"><strong>{title}</strong><small>{!MOCK && id === 'minor' ? '현재 온라인 접수가 준비되지 않았어요. 기존 선택은 해제할 수 있어요.' : sub}</small></span>
              <input
                type="checkbox" aria-label={title}
                checked={!!o[id]}
                disabled={!MOCK && id === 'minor' && !o.minor}
                onChange={e => setOpt(id, e.target.checked as ReleaseOptions[typeof id])}
              />
            </label>
            {!!o[id] && detail[id]}
          </Fragment>
        ))}
      </div>
      {showGuardianModal && (
        <GuardianConsentModal
          guardianName={o.guardian}
          onClose={() => setShowGuardianModal(false)}
          onComplete={(certName, certMethod) => {
            // 단일 업데이트로 통합 — 연속 setOpt는 stale closure로 마지막만 남음
            set('options', { ...o, guardianConsentDone: true, familyCertName: certName, familyCertMethod: certMethod });
            setShowGuardianModal(false);
          }}
        />
      )}
      {group === 'rights' && <p className="aq-option-note">선택한 내용에 따라 권리·계약 서류가 별도로 준비돼요.</p>}
    </>
  );
}

function OptionDocsBanner({ options }: { options: ReleaseOptions }) {
  const chosen = selectedOptions(options);
  if (!chosen.length) return null;
  return (
    <div className="aq-req-banner">
      <h3>추가 확인이 필요한 항목</h3>
      <p>허락서·동의서는 신청 화면에서 자동 작성하고 권리자가 직접 서명할 수 있어요. 완성된 문서는 권리·보완 서류에 보관돼요.</p>
      <div>{chosen.map(([id, title]) => <span key={id} className="aq-req-tag">{title}</span>)}</div>
      {options.minor && (
        <p style={{ marginTop: 13 }}>법정대리인 동의서는 아티스트 본인의 확인과 별도로 관리돼요.</p>
      )}
    </div>
  );
}

function FinalReviewBanner({ form }: { form: WizardForm }) {
  const o = form.options;
  const chosen = selectedOptions(o);
  const warnings: string[] = [];
  if (o.express) warnings.push('신속 발매: 희망일 및 이용 조건 확인 후 진행');
  if (o.minor) warnings.push('미성년: 법정대리인 동의서 및 자격 확인 필요');
  if (o.ai) warnings.push('AI 음원: 이용 권한과 플랫폼별 허용 기준 확인 필요');
  if (chosen.length) warnings.push('필요한 허락서·동의서는 AUDENIQ 전자 문서 작성 또는 보유 서류 첨부로 준비');
  if (o.rerelease) warnings.push(rereleaseSummary(o));
  // 배급은 되지만 플랫폼 검수에서 걸릴 수 있는 표기
  const named = form.tracks.filter(t => t.title.trim());
  for (const w of [titleWarning(form.title), artistWarning(form.artist)]) if (w) warnings.push(w);
  named.forEach((t, k) => {
    const w = titleWarning(t.title);
    if (w) warnings.push(`트랙 ${k + 1}: ${w}`);
    if (t.audioSpec.includes('모노')) warnings.push(`트랙 ${k + 1}: 모노 음원이에요. 스테레오 마스터가 있다면 교체해 주세요.`);
    const secs = mmssToSeconds(t.duration);
    if (secs && secs < 30) warnings.push(`트랙 ${k + 1}: 30초 미만 곡은 일부 플랫폼에서 재생 수익이 집계되지 않아요.`);
  });
  const noLyrics = named.filter(t => !t.instrumental && !t.lyrics.trim()).length;
  if (noLyrics) warnings.push(`가사가 없는 보컬 곡이 ${noLyrics}곡 있어요. 국내 플랫폼 가사 노출을 원하면 곡 상세 정보에 가사를 입력해 주세요.`);
  const incomplete = form.tracks.filter(t => !t.audioName || !t.title || !t.composers).length;
  const withAudio = form.tracks.filter(t => t.audioName).length;
  return (
    <div className="aq-req-banner">
      <h3>접수 전 확인해 주세요.</h3>
      <div className="aq-review-list">
        <div className="aq-review-item">
          <span>발매 옵션</span>
          <strong>{chosen.length ? chosen.map(([, title]) => title).join(' · ') : '일반 발매'}</strong>
        </div>
        <div className="aq-review-item">
          <span>트랙 필수 정보</span>
          <strong>{incomplete ? `${incomplete}곡 보완 필요` : '입력 완료'}</strong>
        </div>
        <div className="aq-review-item">
          <span>원본 음원</span>
          <strong>{withAudio} / {form.tracks.length}곡 선택</strong>
        </div>
        <div className="aq-review-item">
          <span>커버아트</span>
          <strong>{form.coverName || '미등록'}</strong>
        </div>
      </div>
      {warnings.map(w => <p key={w} className="aq-option-note">{w}</p>)}
      <p className="help" style={{ marginTop: 13 }}>
        접수하기를 누르면 입력 내용을 바탕으로 신청서와 필요한 서류 목록이 생성돼요. 최종 승인 및 배급일은 검토 후 결정돼요.
      </p>
    </div>
  );
}

/** 업로드 진행 표시 */
function UploadStatus({ upload, idle }: { upload?: UploadState; idle: string }) {
  if (!upload || upload.state === 'done') return <p className="help">{idle}</p>;
  if (upload.state === 'error') return <p className="help aq-help-error" role="alert">{upload.message || '업로드에 실패했어요. 파일을 다시 선택해 주세요.'}</p>;
  const pct = Math.round(upload.pct * 100);
  return (
    <div className="aq-upload-progress" role="progressbar" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100} aria-label="업로드 진행률">
      <span><i style={{ width: `${Math.max(4, pct)}%` }} /></span>
      <small>{pct < 100 ? `업로드 중 ${pct}%` : '서버에서 확인하는 중…'}</small>
    </div>
  );
}

// 트랙 입력 행 — memo로 감싸 한 곡 입력 시 다른 곡이 리렌더되지 않도록 분리
const TrackEditor = memo(function TrackEditor({
  track: t, index: i, expanded, canDelete, upload,
  onToggleExpand, onDeleteTrack, setTrack, onTrackAudio,
}: {
  track: Track; index: number; expanded: boolean; canDelete: boolean;
  onToggleExpand: (id: string) => void; onDeleteTrack: (id: string) => void;
  setTrack: (id: string, key: keyof Track, value: string | boolean | number) => void;
  onTrackAudio: (id: string, e: React.ChangeEvent<HTMLInputElement>) => void;
  upload?: UploadState;
}) {
  return (
    <article className="track-editor">
      <div className="minor-actions">
        <h3>트랙 {i + 1}</h3>
        {canDelete && (
          <button type="button" className="link-btn" onClick={() => onDeleteTrack(t.id)}>삭제</button>
        )}
      </div>
      <div className="field track-title-field">
        <label htmlFor={`tr-${i}-title`}>곡 제목 <span className="required">*</span></label>
        <input id={`tr-${i}-title`} value={t.title} onChange={e => setTrack(t.id, 'title', e.target.value)} placeholder="곡명만 입력 (피처링·버전 표기 제외)" maxLength={200} />
      </div>
      <div className="field">
        <label htmlFor={`tr-${i}-featuring`}>피처링 아티스트</label>
        <input id={`tr-${i}-featuring`} value={t.featuring} onChange={e => setTrack(t.id, 'featuring', e.target.value)} placeholder="없으면 비워 두세요 · 여러 명은 쉼표로 구분" maxLength={200} />
      </div>
      <div className="form-grid">
        <div className="field">
          <label htmlFor={`tr-${i}-composers`}>작곡 <span className="required">*</span></label>
          <input id={`tr-${i}-composers`} value={t.composers} onChange={e => setTrack(t.id, 'composers', e.target.value)} placeholder="실명 또는 활동명, 쉼표로 구분" maxLength={200} />
        </div>
        <div className="field">
          <label htmlFor={`tr-${i}-lyricists`}>작사 {!t.instrumental && <span className="required">*</span>}</label>
          <input
            id={`tr-${i}-lyricists`} value={t.instrumental ? '' : t.lyricists} disabled={t.instrumental}
            onChange={e => setTrack(t.id, 'lyricists', e.target.value)}
            placeholder={t.instrumental ? '연주곡은 입력하지 않아요' : '실명 또는 활동명, 쉼표로 구분'} maxLength={200}
          />
        </div>
      </div>
      <label className="check-line aq-inst-line">
        <input type="checkbox" checked={t.instrumental} onChange={e => setTrack(t.id, 'instrumental', e.target.checked)} />
        <span><strong>가사 없는 연주곡이에요</strong><small>보컬·가사가 없으면 작사와 가사를 입력하지 않아도 돼요.</small></span>
      </label>
      <div className="field">
        <label htmlFor={`trackFile-${i}`}>음원 파일 <span className="required">*</span></label>
        <input
          type="file" id={`trackFile-${i}`}
          accept=".wav,.flac,.m4a,.aif,.aiff,.aifc,.wv,.tta,audio/wav,audio/flac,audio/mp4,audio/aiff,audio/wavpack,audio/tta"
          onChange={e => onTrackAudio(t.id, e)}
        />
        <UploadStatus upload={upload} idle={t.audioName
          ? `${t.audioName}${t.audioSpec ? ` · ${t.audioSpec}` : t.audioSize ? ` · ${fileSize(t.audioSize)}` : ''}${t.assetId ? ' · 업로드 완료' : MOCK ? '' : ' · 업로드되지 않았어요. 파일을 다시 선택해 주세요.'}`
          : '무손실 원본 WAV·FLAC·ALAC(.m4a)·AIFF·WavPack(.wv)·TTA · 44.1kHz 이상, 16bit 이상, 스테레오'} />
      </div>
      <div className="track-duration-label" aria-live="polite">
        {t.duration ? `곡 길이 · ${t.duration}` : '음원을 선택하면 곡 길이를 자동으로 확인해요.'}
      </div>
      <button
        type="button" className="track-detail-toggle"
        onClick={() => onToggleExpand(t.id)}
        aria-expanded={expanded}
      >
        <span>상세 정보 {t.isrc ? `· ${t.isrc}` : ''}</span>
        <span className={`toggle-arrow${expanded ? ' open' : ''}`}>›</span>
      </button>
      {expanded && (
        <div className="track-detail-body">
          <div className="form-grid">
            <div className="field">
              <label htmlFor={`tr-${i}-version`}>버전 / 부제</label>
              <input id={`tr-${i}-version`} value={t.version} onChange={e => setTrack(t.id, 'version', e.target.value)} placeholder="예: Acoustic Version" maxLength={200} />
            </div>
            <div className="field">
              <label htmlFor={`tr-${i}-isrc`}>ISRC (보유 시)</label>
              <input id={`tr-${i}-isrc`} value={t.isrc} onChange={e => setTrack(t.id, 'isrc', e.target.value)} placeholder="예: KR-ABC-26-00001" maxLength={200} />
            </div>
            <div className="field">
              <label htmlFor={`tr-${i}-arrangers`}>편곡</label>
              <input id={`tr-${i}-arrangers`} value={t.arrangers} onChange={e => setTrack(t.id, 'arrangers', e.target.value)} placeholder="참여자 이름" maxLength={200} />
            </div>
            <div className="field">
              <label htmlFor={`tr-${i}-performers`}>실연자 (보컬·연주)</label>
              <input id={`tr-${i}-performers`} value={t.performers} onChange={e => setTrack(t.id, 'performers', e.target.value)} placeholder="참여자 이름" maxLength={200} />
            </div>
            <div className="field">
              <label htmlFor={`tr-${i}-producer`}>프로듀서</label>
              <input id={`tr-${i}-producer`} value={t.producer} onChange={e => setTrack(t.id, 'producer', e.target.value)} placeholder="프로듀서 이름" maxLength={200} />
            </div>
          </div>
          {!t.instrumental && <div className="field">
            <label htmlFor={`tr-${i}-lyrics`}>가사 전문</label>
            <textarea
              id={`tr-${i}-lyrics`} value={t.lyrics}
              onChange={e => setTrack(t.id, 'lyrics', e.target.value)}
              rows={4} maxLength={10000} placeholder="가사 전체를 입력해 주세요 (국내 플랫폼 가사 노출·심의에 쓰여요)"
            />
          </div>}
          <label className="check-line">
            <input
              type="checkbox" checked={t.explicit}
              onChange={e => setTrack(t.id, 'explicit', e.target.checked)}
            />
            <span>청소년 이용불가 / Explicit 가사가 포함돼요.</span>
          </label>
        </div>
      )}
    </article>
  );
});

/** 폼 → 서버 전송 형식 */
function toPayload(form: WizardForm, step: number): ReleasePayload {
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
    coverAssetId: form.coverAssetId || undefined,
    originalDate: form.options.rerelease && form.options.rereleaseKind === 'new_version' ? form.releaseDate : form.originalDate,
    release_date: form.releaseDate || '',
    tracks: form.tracks.map(t => ({
      id: t.id, title: t.title.trim(), isrc: t.isrc.trim(), duration: t.duration,
      version: t.version.trim(), composers: t.composers.trim(),
      lyricists: t.lyricists.trim(), arrangers: t.arrangers.trim(),
      performers: t.performers.trim(), audioName: t.audioName,
      audioSize: t.audioSize, explicit: t.explicit,
      producer: t.producer.trim(), lyrics: t.lyrics.trim(),
      assetId: t.assetId || undefined, serverId: t.serverId || undefined,
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
function makeCoverThumbnail(file: File): Promise<{ data: string; width: number; height: number }> {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(file);
    const img = new Image();
    img.onload = () => {
      URL.revokeObjectURL(url);
      // 실서버는 발매 정보(최대 64KB)에 함께 저장하므로 더 작게
      const max = MOCK ? 480 : 240;
      const scale = Math.min(1, max / Math.max(img.width, img.height));
      const w = Math.max(1, Math.round(img.width * scale));
      const h = Math.max(1, Math.round(img.height * scale));
      const canvas = document.createElement('canvas');
      canvas.width = w; canvas.height = h;
      canvas.getContext('2d')?.drawImage(img, 0, 0, w, h);
      resolve({ data: canvas.toDataURL('image/jpeg', MOCK ? 0.8 : 0.72), width: img.width, height: img.height });
    };
    img.onerror = () => { URL.revokeObjectURL(url); reject(new Error('이미지를 읽을 수 없어요.')); };
    img.src = url;
  });
}

const AUDIO_RE = /\.(wav|flac|m4a|aif|aiff|aifc|wv|tta)$/i;

/** 발매 신청 시 계약서·권리 서류를 준비 (같은 발매에 이미 있으면 다시 만들지 않음) */
function ensureReleaseDocuments(f: WizardForm, releaseId: string) {
  const title = f.title.trim() || '제목 없는 발매';
  const existing = docsForRelease(getDocsSnapshot(), releaseId);
  if (existing.some(d => d.kind === 'agreements')) return;
  const genreVal = f.genre === '__other__' ? f.genreCustom.trim() : genreLabel(f.genre);
  const at = stampNow();
  const lines = [
    'AUDENIQ 디지털 음원 배급 신청·계약서',
    '',
    '1. 신청인 및 발매 정보',
    `아티스트: ${f.artist.trim() || '미입력'}`,
    `발매명: ${title}`,
    `발매 유형: ${kindLabel(f.type)}`,
    `장르: ${genreVal || '미입력'}`,
    `발매 희망일: ${f.releaseDate || '미정'}`,
    `레이블 표기: ${f.label.trim() || '미입력'}`,
    `수록곡: ${f.tracks.map((t, i) => `${i + 1}. ${t.title.trim() || '제목 없음'} (${t.duration || '길이 확인 전'})`).join(' / ')}`,
    '',
    '2. 권리자 및 배급 범위',
    `마스터 권리자: ${f.ownership.trim() || '미입력'}`,
    `℗ 표기: ${f.phonogram.trim() || '미입력'}`,
    `© 표기: ${f.copyright.trim() || '미입력'}`,
    `배급 지역: ${f.territories.includes('WORLD') ? '전 세계' : '지정 안 함'}`,
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
    reviewHistory: [{ status: '접수 요청', time: at, detail: '신청서가 작성됐어요. 담당자 검토 후 서명을 진행할 수 있어요.' }],
    reviewStatus: 'prepared',
  };
  if (!existing.some(d => d.kind === 'rights')) addDoc(rights);
  addDoc(agreement);
}

type SaveState = { kind: 'idle' } | { kind: 'saving' } | { kind: 'saved'; at: string } | { kind: 'error' };

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
  // 지금까지 열어 본 가장 먼 단계 (단계 목록에서 바로 이동 가능)
  const [reached, setReached] = useState(0);
  // 보완 요청 — 수정 모드에서 불러온 발매의 요청 항목과 신청서 위치
  const [fixes, setFixes] = useState<ResolvedCorrection[]>([]);
  const [focusField, setFocusField] = useState<{ id: string; n: number } | null>(null);
  // 신청인 서명 (마지막 단계)
  const signRef = useRef<SignaturePadHandle>(null);
  const [signed, setSigned] = useState(false);
  const [signer, setSigner] = useState({ name: '', role: SIGNER_ROLES[0], touched: false });
  const [agreed, setAgreed] = useState<Record<string, boolean>>({});
  const [dir, setDir] = useState<'fwd' | 'back'>('fwd');
  const [form, setForm] = useState<WizardForm>(() => ({ ...EMPTY, artist: profile.name || '', tracks: [newTrack()] }));
  const [dsps, setDsps] = useState<DspAvailability[] | null>(null);
  const [dspError, setDspError] = useState('');
  useEffect(() => {
    let alive = true;
    api.listDsps().then(items => {
      if (!alive) return;
      setDsps(items);
      setDspError('');
      if (!editId) setForm(f => f.platforms.length ? f : ({ ...f, platforms: items.filter(d => d.available).map(d => d.slug) }));
    }).catch(e => { if (alive) { setDsps(null); setDspError(errorMessage(e, '플랫폼 상태를 확인하지 못했어요.')); } });
    return () => { alive = false; };
  }, [editId]);
  const [error, setError] = useState('');
  const [submitting, setSubmitting] = useState(false);
  // 중복 제출 방지용 ref (비동기 경계에서도 동작)
  const submittingRef = useRef(false);
  const [loadingEdit, setLoadingEdit] = useState(!!editId);
  const [origStatus, setOrigStatus] = useState<string>('draft');
  // 이미 신청서를 낸 발매를 보완·수정해 다시 접수할 때는 신청서(서명)를 새로 받지 않는다 — 신규 발매만 신청서를 쓴다
  const [hasApplication, setHasApplication] = useState(false);
  const [origDate, setOrigDate] = useState('');
  const [expandedTracks, setExpandedTracks] = useState<Set<string>>(new Set());
  const [save, setSave] = useState<SaveState>({ kind: 'idle' });
  const [dragOver, setDragOver] = useState(false);
  // 업로드 진행 상태 — 키: 트랙 ID 또는 'cover'
  const [uploads, setUploads] = useState<Record<string, UploadState>>({});
  const uploadAborts = useRef(new Map<string, AbortController>());
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
    try {
      const r = await api.uploadFile(file, kind, pct => setUpload(key, { pct, state: 'uploading' }), ctrl.signal);
      if (ctrl.signal.aborted) return null;
      setUpload(key, { pct: 1, state: 'done' });
      // '올리는 중' 안내가 떠 있었다면 끝났으니 지운다
      setError(prev => (prev.includes('올리는 중이에요') ? '' : prev));
      return r.assetId;
    } catch (e) {
      if (ctrl.signal.aborted) return null;
      setUpload(key, { pct: 0, state: 'error', message: errorMessage(e, '업로드에 실패했어요. 파일을 다시 선택해 주세요.') });
      return null;
    } finally {
      if (uploadAborts.current.get(key) === ctrl) uploadAborts.current.delete(key);
    }
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
        releaseDate: rel.release_date || '',
        originalDate: d?.options?.rerelease && d.options.previousReleaseDate ? d.options.previousReleaseDate : d?.originalDate || '',
        upc: d?.upc || '',
        territories: d?.territories ?? ['WORLD'],
        platforms: d?.platforms ?? f.platforms,
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
      const resolved = (rel.corrections ?? []).map(c => resolveCorrection(c, tracks.map(t => t.serverId || t.id)));
      setFixes(resolved);
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
      toast(errorMessage(e, '발매 정보를 불러오지 못했어요.'));
      nav('/releases', { replace: true });
    });
    return () => { cancelled = true; };
  }, [editId, nav, toast]); // eslint-disable-line react-hooks/exhaustive-deps -- fix 파라미터는 처음 불러올 때만 쓴다

  // 위자드에서는 헤더 숨김 + 레이아웃 패딩 제거
  useEffect(() => {
    document.body.classList.add('wizard-mode');
    return () => document.body.classList.remove('wizard-mode');
  }, []);

  // 저장되지 않은 입력이 있으면 창 닫기 전에 경고
  useEffect(() => {
    const onBeforeUnload = (e: BeforeUnloadEvent) => {
      if (!dirtyRef.current || submittingRef.current) return;
      e.preventDefault();
    };
    window.addEventListener('beforeunload', onBeforeUnload);
    return () => window.removeEventListener('beforeunload', onBeforeUnload);
  }, []);

  const set = useCallback(<K extends keyof WizardForm>(key: K, value: WizardForm[K]) => {
    dirtyRef.current = true;
    setForm(f => ({ ...f, [key]: value }));
  }, []);

  const setTrack = useCallback((id: string, key: keyof Track, value: string | boolean | number) => {
    dirtyRef.current = true;
    setForm(f => ({ ...f, tracks: f.tracks.map(t => (t.id === id ? { ...t, [key]: value } : t)) }));
  }, []);

  // 자동 임시 저장 — 저장 요청을 직렬화해 draft가 두 개 생기지 않도록 한다
  const autoSave = useCallback((): Promise<void> => {
    if (!canAutoSave) return Promise.resolve();
    const f = formRef.current;
    if (!f.title.trim() && !f.artist.trim()) return Promise.resolve(); // 빈 폼은 저장하지 않음
    const run = async () => {
      setSave({ kind: 'saving' });
      try {
        const r = await api.saveDraft(draftIdRef.current, toPayload(formRef.current, stepRef.current));
        draftIdRef.current = r.id;
        // 서버가 새로 만든 트랙 ID를 반영해야 다음 저장 때 트랙이 중복 생성되지 않는다
        const ids = r.trackServerIds;
        if (ids && Object.keys(ids).length) {
          setForm(f => {
            let changed = false;
            const tracks = f.tracks.map(t => {
              const sid = ids[t.id];
              if (sid && sid !== t.serverId) { changed = true; return { ...t, serverId: sid }; }
              return t;
            });
            return changed ? { ...f, tracks } : f;
          });
        }
        dirtyRef.current = false;
        setSave({ kind: 'saved', at: stampNow().slice(11) });
      } catch {
        setSave({ kind: 'error' });
      }
    };
    const p = saveChain.current.then(run, run);
    saveChain.current = p;
    return p;
  }, [canAutoSave]);

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
    setError(msg);
    requestAnimationFrame(() => {
      if (sel) {
        const el = document.querySelector<HTMLElement>(sel);
        if (el) {
          el.focus();
          el.scrollIntoView({ behavior: 'smooth', block: 'center' });
          el.classList.remove('aq-invalid');
          void el.offsetWidth;
          el.classList.add('aq-invalid');
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
        if (!PROFILE_LINKS.some(l => ap[l.key].trim())) {
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
        if (up?.state === 'error') return fail(`트랙 ${k + 1}의 음원 업로드에 실패했어요. 파일을 다시 선택해 주세요.`, `#trackFile-${k}`);
        if (!MOCK && !t.assetId) return fail(`트랙 ${k + 1}의 음원 파일을 다시 선택해 주세요. (업로드 기록이 없어요)`, `#trackFile-${k}`);
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
    if (i === 1) {
      const typeErr = releaseTypeIssue(form.type, form.tracks.map(t => mmssToSeconds(t.duration)));
      if (typeErr) return fail(typeErr, null);
    }
    if (i === 2) {
      if (!form.coverName) return fail('커버아트를 등록해 주세요.', '#coverFile');
      if (uploads.cover?.state === 'uploading') return fail('커버아트를 올리는 중이에요. 업로드가 끝나면 다음으로 넘어갈 수 있어요.', null);
      if (uploads.cover?.state === 'error') return fail('커버아트 업로드에 실패했어요. 이미지를 다시 선택해 주세요.', '#coverFile');
      if (!MOCK && !form.coverAssetId) return fail('커버아트를 다시 선택해 주세요. (업로드 기록이 없어요)', '#coverFile');
    }
    if (i === 3) {
      if (!form.releaseDate) return fail('발매일을 선택해 주세요.', '#f-releaseDate');
      // 수정 모드에서 기존 발매일을 그대로 두는 경우는 과거여도 허용
      if (form.releaseDate < minReleaseDate(form.options.express) && form.releaseDate !== origDate) {
        return fail(form.options.express
          ? '신속 발매도 플랫폼 납품에 최소 3일이 필요해요. 3일 뒤 이후 날짜를 선택해 주세요.'
          : '플랫폼 납품·검수에 2주가 필요해요. 오늘부터 14일 뒤 이후로 선택하거나, 급하면 아래 ‘신속 발매 요청’을 선택해 주세요.', '#f-releaseDate');
      }
      if (!dsps) return fail(dspError || '플랫폼 배급 가능 상태를 확인하는 중이에요.', '#aqPlatforms');
      if (!form.platforms.length) return fail('배급할 플랫폼을 하나 이상 선택해 주세요.', '#aqPlatforms');
      if (['coverLicenseFile', 'sampleLicenseFile', 'featuredConsentFile', 'sharedContractFile'].some(k => uploads[k]?.state === 'uploading')) return fail('첨부서류 업로드가 끝나면 계속할 수 있어요.', '#aqSpecialOptions');
      if (form.platforms.some(p => !dsps.some(d => d.slug === p && d.available))) return fail('현재 배급할 수 없는 플랫폼이 선택되어 있어요. 해당 선택을 해제해 주세요.', '#aqPlatforms');
      if (form.upc.trim() && !upcValid(form.upc.trim())) return fail('UPC/EAN 번호가 올바르지 않아요. 숫자 12~13자리와 마지막 확인 숫자를 확인해 주세요.', '#f-upc');
      if (form.upc.trim() && !/^0?\d{12}$/.test(form.upc.trim())) return fail('UPC는 12자리(UPC-A)만 받을 수 있어요. 13자리 EAN은 0으로 시작하는 번호만 쓸 수 있어요.', '#f-upc');
      const o = form.options;
      if (o.express && !o.expressAck) return fail('신속 발매 안내를 확인해 주세요.', '#aqExpressAck');
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
        if (!o.guardianConsentDone) return fail('법정대리인 동의를 진행해 주세요.', null);
      }
      if (o.ai && !o.aiTool.trim()) return fail('AI를 어떻게 활용했는지 골라 주세요.', '#aqAiTool');
      if (o.cover) {
        const valid = o.coverTracks.filter(c => form.tracks.some(t => t.id === c.trackId));
        if (!valid.length) return fail('커버곡에 해당하는 트랙을 하나 이상 선택해 주세요.', null);
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
    }
    setError('');
    return true;
  };

  const goStep = (to: number) => {
    setDir(to > step ? 'fwd' : 'back');
    setStep(to);
    setReached(r => Math.max(r, to));
    setError('');
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  const submit = async () => {
    // 앞 단계까지 모두 다시 검증 (단계를 건너뛰어 돌아온 경우 대비)
    for (let i = 0; i < STEPS.length; i++) {
      if (!validateStep(i)) { if (i !== step) goStep(i); return; }
    }
    if (submittingRef.current) return;
    submittingRef.current = true;
    setSubmitting(true);
    try {
      await saveChain.current.catch(() => {});
      const payload = toPayload(form, step);
      const application = resubmit ? undefined : await createApplication({
        payload, signature: await compactSignature(signRef.current?.toDataURL() ?? ''), signerName, signerRole: signer.role,
        agreements: AGREEMENTS.map(a => a.id),
      });
      const r = await api.submitRelease(draftIdRef.current, { ...payload, application });
      draftIdRef.current = r.id;
      dirtyRef.current = false;
      // 실서버는 계약서를 서버가 만든다 (알림·서류는 pushNotice가 새로 받게 한다)
      if (MOCK) ensureReleaseDocuments(form, r.id);
      pushNotice({
        id: uid('n'), kind: '발매', time: stampNow(), link: `/releases/${r.id}`,
        title: editId && origStatus !== 'draft' ? `${r.title} 수정 내용이 접수됐어요.` : `${r.title} 발매 신청이 접수됐어요.`,
        detail: '담당자 검토가 시작됐어요. 계약서와 권리 서류 메뉴에서 준비된 문서를 확인해 주세요.',
      });
      toast(editId && origStatus !== 'draft' ? '발매 정보가 수정됐어요.' : '발매 신청이 접수됐어요.', 'success');
      // 서명한 신청서를 정식 서류로 바로 보여 준다 (보완 재접수는 발매 상세로)
      nav(resubmit ? `/releases/${r.id}` : `/releases/${r.id}/application?done=1`, { replace: true });
    } catch (e) {
      if (e instanceof ApiError && e.code === 'DSP_UNAVAILABLE') {
        goStep(3);
        api.listDsps().then(setDsps).catch(err => { setDsps(null); setDspError(errorMessage(err)); });
      }
      setError(errorMessage(e, '제출에 실패했어요. 잠시 후 다시 시도해 주세요.'));
      toast('제출에 실패했어요. 다시 시도해 주세요.');
    } finally {
      submittingRef.current = false;
      setSubmitting(false);
    }
  };

  const next = async () => {
    if (step === STEPS.length - 1) { await submit(); return; }
    if (!validateStep(step)) return;
    void autoSave();
    goStep(step + 1);
  };

  const leave = async () => {
    if (dirtyRef.current && !canAutoSave) {
      const ok = await confirm({ title: '수정을 그만둘까요?', message: '저장하지 않은 변경 내용은 사라져요.', confirmLabel: '나가기', danger: true });
      if (!ok) return;
    } else if (dirtyRef.current) {
      await autoSave();
      if (draftIdRef.current) toast('작성 중인 내용을 임시 저장했어요.', 'info');
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
      setForm(f => ({ ...f, coverData: thumb.data, coverAssetId: '' }));
      const assetId = await startUpload('cover', file, 'IMAGE');
      if (assetId) {
        setForm(f => (f.coverName === file.name ? { ...f, coverAssetId: assetId } : f));
        dirtyRef.current = true;
        toast('커버 이미지가 등록됐어요.');
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
      toast('음원 파일이 등록됐어요.');
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

  // 서명자 기본값: 권리자 → 아티스트 정보의 이름
  const signerName = signer.touched ? signer.name : (form.ownership || profile.name || '');
  const resubmit = !!editId && origStatus !== 'draft' && hasApplication;
  const s = STEPS[step];
  const genreIsCustom = form.genre === '__other__';
  const availablePlatforms = dsps?.filter(d => d.available).map(d => d.slug) ?? [];
  const allPlatforms = availablePlatforms.length > 0 && availablePlatforms.every(p => form.platforms.includes(p)) && form.platforms.every(p => availablePlatforms.includes(p));
  // 기본은 ‘모두 배급’ 스위치만. 끄면 아래에 플랫폼별 선택이 열린다 (일부만 고른 발매는 처음부터 열림)
  const [pickPlatforms, setPickPlatforms] = useState(false);
  const showPlatforms = pickPlatforms || !allPlatforms;

  const reviewSections: [string, string, number][] = [
    ['발매 정보', `${form.title || '제목 없음'} · ${form.artist || '아티스트 없음'} · ${kindLabel(form.type)}`, 0],
    ['트랙', form.tracks.map(t => t.title || '곡명 없음').join(' / '), 1],
    ['커버아트', form.coverName || '등록되지 않음', 2],
    ['발매일', form.releaseDate ? formatKoreanDate(form.releaseDate) : '지정하지 않음', 3],
    ['배급 대상', form.platforms.map(dspLabel).join(', ') || '선택하지 않음', 3],
    ['권리자', form.ownership || '미입력', 4],
    ['권리 확인', rightsOk(form) ? '필수 확인 완료' : '필수 확인 항목 누락', 4],
  ];

  // 상단 바 오른쪽 칸에 들어가도록 짧게 (긴 문구는 title 속성으로)
  const saveLabel = !canAutoSave ? '수정 중'
    : save.kind === 'saving' ? '저장 중'
    : save.kind === 'saved' ? `저장됨 ${save.at}`
    : save.kind === 'error' ? '저장 실패' : '';
  const saveTitle = !canAutoSave ? '수정 내용은 마지막 단계에서 ‘수정 완료’를 눌러야 반영돼요.'
    : save.kind === 'saved' ? `${save.at}에 자동 저장됐어요.`
    : save.kind === 'error' ? '자동 저장에 실패했어요. 네트워크를 확인해 주세요.' : '';

  return (
    <section id="view-new" className="view">
    <div className="wizard">
      <div className="wizard-topbar" aria-label="발매 신청 탐색">
        <button type="button" id="wizardTopBack" className="wizard-topback" aria-label="이전으로 돌아가기" onClick={back}>
          <BackIcon />
        </button>
        <span className="wizard-top-title">{editId ? (origStatus === 'draft' ? '발매 이어서 작성' : '발매 정보 수정') : '새로운 발매'}</span>
        <span className="aq-save-slot" aria-live="polite">
          {saveLabel && (
            <span className={`aq-save-state is-${canAutoSave ? save.kind : 'edit'}`} title={saveTitle}>{saveLabel}</span>
          )}
        </span>
      </div>

      {loadingEdit && (
        <div className="notice aq-loading-notice" role="status" style={{ marginBottom: 12 }}>
          <span className="aq-spinner" aria-hidden="true" /> 기존 발매 정보를 불러오는 중이에요…
        </div>
      )}

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

      <div className="aq-wiz-layout">
      <aside className="aq-wiz-rail" aria-label="발매 신청 단계">
        <p className="aq-wiz-rail-kicker">발매 신청</p>
        <ol className="aq-wiz-steps">
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
        <div className="aq-wiz-summary">
          {form.coverData
            ? <img src={form.coverData} alt="" />
            : <span className="aq-wiz-summary-cover" aria-hidden="true"><Glyph name="music" size={18} /></span>}
          <div className="min-0">
            <strong>{form.title.trim() || '제목 없는 발매'}</strong>
            <span>{form.artist.trim() || '아티스트 미입력'} · {kindLabel(form.type)}</span>
            <span>{form.tracks.filter(t => t.title.trim()).length}곡{form.releaseDate ? ` · ${formatKoreanDate(form.releaseDate)}` : ''}</span>
          </div>
        </div>
        <p className="aq-wiz-rail-note">
          {canAutoSave ? '입력 내용은 자동으로 임시 저장돼요.' : '수정 내용은 마지막 단계에서 ‘수정 완료’를 눌러야 반영돼요.'}
        </p>
      </aside>
      <div className="aq-wiz-main">
      <div className={`wizard-header aq-step-anim is-${dir}`} key={`h-${step}`}>
        <p className="aq-step-kicker">{s.kicker}</p>
        <h1 id="newTitle" className="aq-step-title">{s.title}</h1>
        <p id="wizardSubtitle">{s.sub}</p>
      </div>

      {fixes.length > 0 && (
        <section className="aq-fix-panel" aria-labelledby="aqFixHead">
          <div className="aq-fix-head">
            <strong id="aqFixHead">보완 요청 {fixes.length}건</strong>
            <span>항목을 누르면 고쳐야 할 입력칸으로 이동해요.</span>
          </div>
          <ul>
            {fixes.map((c, i) => (
              <li key={`${c.code}-${c.trackId ?? ''}-${i}`} className={c.step === step ? 'is-here' : ''}>
                <button type="button" onClick={() => jumpToFix(c)}>
                  <em>{correctionWhere(c)}</em>
                  <span>{c.message}</span>
                  <b aria-hidden="true">{c.step === step ? '입력칸 보기' : '이동'} ›</b>
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}

      <div id="wizardBody" className={`aq-step-anim is-${dir}`} key={`b-${step}`}>
        {step === 0 && (
          <section className="step-section">
            <div className="form-grid">
              <div className="field">
                <label htmlFor="f-artist">아티스트명 <span className="required">*</span></label>
                <input id="f-artist" value={form.artist} onChange={e => set('artist', e.target.value)} maxLength={120} placeholder="예: AUDENIQ" autoComplete="off" />
              </div>
              <div className="field">
                <label htmlFor="f-title">발매 제목 <span className="required">*</span></label>
                <input id="f-title" value={form.title} onChange={e => set('title', e.target.value)} maxLength={180} placeholder="싱글 또는 앨범 제목" autoComplete="off" />
              </div>
              <div className="field">
                <label htmlFor="f-type">발매 유형</label>
                <select id="f-type" value={form.type} onChange={e => set('type', e.target.value)}>
                  {KINDS.map(([id, title]) => <option key={id} value={id}>{title}</option>)}
                </select>
              </div>
              <div className="field">
                <label htmlFor="f-language">주요 언어</label>
                <select id="f-language" value={form.language} onChange={e => set('language', e.target.value)}>
                  {LANGUAGES.map(([id, title]) => <option key={id} value={id}>{title}</option>)}
                </select>
              </div>
              <div className="field">
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
                    maxLength={90} placeholder="장르를 입력해 주세요" aria-label="직접 입력할 장르"
                    className="aq-reveal" style={{ marginTop: 10 }}
                  />
                )}
              </div>
              <div className="field">
                <label htmlFor="f-label">레이블 / 발매사 표기</label>
                <input id="f-label" value={form.label} onChange={e => set('label', e.target.value)} maxLength={120} placeholder="권리 계약에 맞는 표기" />
              </div>
            </div>
            <div className="field">
              <label htmlFor="f-notes">앨범 소개</label>
              <textarea
                id="f-notes" value={form.notes} onChange={e => set('notes', e.target.value)}
                rows={3} maxLength={1500} placeholder="발매 소개와 전달 메모"
              />
              <p className="help aq-counter">{form.notes.length} / 1500</p>
            </div>
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
            {!form.artistProfile.isNew && (
              <div className="aq-reveal">
                <p className="help" style={{ margin: '4px 0 14px' }}>
                  기존 아티스트 페이지 주소를 하나 이상 입력해 주세요. 이름이 같은 다른 아티스트 페이지로 잘못 올라가는 것을 막아요.
                </p>
                {PROFILE_LINKS.map(l => (
                  <div className="field" key={l.key}>
                    <label htmlFor={`f-link-${l.key}`}>{l.label}</label>
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
                  expanded={expandedTracks.has(t.id)}
                  canDelete={form.tracks.length > 1}
                  onToggleExpand={toggleTrackExpand}
                  onDeleteTrack={deleteTrack}
                  setTrack={setTrack}
                  onTrackAudio={onTrackAudio}
                  upload={uploads[t.id]}
                />
              ))}
            </div>
            <div className="spaced-actions">
              <button
                type="button" className="button secondary"
                onClick={() => {
                  const t = newTrack();
                  set('tracks', [...form.tracks, t]);
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
            <div className="field">
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
                  <small>{form.coverName ? '다른 이미지로 바꾸려면 다시 선택해 주세요.' : 'JPG · PNG, 정사각형 3000×3000 이상 권장 (최대 20MB)'}</small>
                </span>
              </label>
              {uploads.cover && uploads.cover.state !== 'done' && <UploadStatus upload={uploads.cover} idle="" />}
              {coverWarn && <p className="help aq-help-warn">{coverWarn}</p>}
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
                <p className="help">실제 배급되면 각 플랫폼에 이 커버아트로 표시돼요.</p>
                <button
                  type="button" className="link-btn"
                  onClick={() => {
                    uploadAborts.current.get('cover')?.abort();
                    uploadAborts.current.delete('cover');
                    setUpload('cover', null);
                    set('coverName', '');
                    setForm(f => ({ ...f, coverData: '', coverAssetId: '' }));
                    setCoverWarn('');
                    if (coverInputRef.current) coverInputRef.current.value = '';
                  }}
                >커버 삭제</button>
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
              />
              <div className="field">
                <label htmlFor="f-upc">음반 코드 UPC (있을 때만)</label>
                <input id="f-upc" inputMode="numeric" value={form.upc} onChange={e => set('upc', e.target.value.replace(/\D/g, '').slice(0, 13))} maxLength={13} placeholder="없으면 자동 발급돼요" />
              </div>
            </div>
            <h2 className="subhead">배급 대상</h2>
            <label className="check-line">
              <input
                type="checkbox"
                checked={form.territories.includes('WORLD')}
                onChange={e => set('territories', e.target.checked ? ['WORLD'] : [])}
              />
              <span><strong>전 세계 배급</strong><small>권리를 보유한 지역에만 배급할 수 있어요.</small></span>
            </label>
            <h2 className="subhead">배급 플랫폼</h2>
            <div className="distribution-default">
              <div>
                <strong>{allPlatforms ? '주요 음악 플랫폼에 모두 배급해요.' : `${form.platforms.length}개 플랫폼에 배급해요.`}</strong>
                <p className="help">{dspError || (!dsps ? '서버에서 플랫폼 상태를 확인하는 중이에요.' : showPlatforms ? '현재 배급 가능한 플랫폼만 선택할 수 있어요.' : '특정 플랫폼만 고르려면 스위치를 꺼 주세요.')}</p>
              </div>
              <label className="aq-switch">
                <input
                  type="checkbox" aria-label="모든 플랫폼에 배급"
                  checked={allPlatforms && !pickPlatforms}
                  disabled={!dsps || availablePlatforms.length === 0}
                  onChange={e => {
                    // 켜면 모두 선택하고 목록을 닫고, 끄면 지금 선택 그대로 목록을 연다
                    if (e.target.checked) { set('platforms', availablePlatforms); setPickPlatforms(false); } else setPickPlatforms(true);
                  }}
                />
                <span />
              </label>
            </div>
            {showPlatforms && (
            <div id="aqPlatforms" className="aq-dsp-groups aq-reveal">
              {DSP_GROUPS.map(([region, match]) => [region, dsps?.filter(match) ?? []] as const).filter(([, list]) => list.length > 0).map(([region, list]) => (
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
                          <span className="aq-dsp-mark" style={{ background: DSP_COLOR[key] ?? '#3B63F3' }} aria-hidden="true">{label.slice(0, 1)}</span>
                          <span className="aq-dsp-name">{label}{!available && <small>현재 배급 불가</small>}</span>
                          <span className="aq-dsp-check" aria-hidden="true"><CheckIcon size={11} /></span>
                        </button>
                      );
                    })}
                  </div>
                </div>
              ))}
            </div>
            )}
            {!dsps && <p className="help" role="status">{dspError || '플랫폼 목록을 서버에서 불러오는 중이에요.'}</p>}
            <div className="aq-fix-zone"><OptionsSection form={form} set={set} group="service" onDocument={attachOptionDocument} uploads={uploads} onElectronic={kind => void openElectronic(kind)} onOpenDoc={setOpenDocId} releaseId={draftIdRef.current ?? undefined} /></div>
          </section>
        )}

        {step === 4 && (
          <section className="step-section">
            <div className="form-grid">
              <div className="field">
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
                <small className="help">입력하면 아래 ℗·© 표기가 ‘{(form.releaseDate || todayStr()).slice(0, 4)} 권리자명’으로 자동으로 채워져요. 다르면 직접 고쳐 주세요.</small>
              </div>
              <div className="field">
                <label htmlFor="f-phonogram">℗ 음반제작자 권리 표기 <span className="required">*</span></label>
                <input id="f-phonogram" value={form.phonogram} onChange={e => set('phonogram', e.target.value)} maxLength={180} placeholder={`예: ${new Date().getFullYear()} 권리자명`} />
              </div>
              <div className="field">
                <label htmlFor="f-copyright">© 아트워크 / 앨범 권리 표기 <span className="required">*</span></label>
                <input id="f-copyright" value={form.copyright} onChange={e => set('copyright', e.target.value)} maxLength={180} placeholder={`예: ${new Date().getFullYear()} 권리자명`} />
              </div>
            </div>
            {!form.ownership && !form.phonogram && !form.copyright && form.artist.trim() && (
              <button
                type="button" className="link-btn aq-autofill"
                onClick={() => {
                  const y = (form.releaseDate || todayStr()).slice(0, 4);
                  const who = form.label.trim() || form.artist.trim();
                  setForm(f => ({ ...f, ownership: who, phonogram: `${y} ${who}`, copyright: `${y} ${who}` }));
                  dirtyRef.current = true;
                }}
              >
                ‘{form.label.trim() || form.artist.trim()}’(으)로 권리자 정보 채우기
              </button>
            )}
            <div id="aqSpecialOptions" className="aq-fix-zone"><OptionsSection form={form} set={set} group="rights" onDocument={attachOptionDocument} uploads={uploads} onElectronic={kind => void openElectronic(kind)} onOpenDoc={setOpenDocId} releaseId={draftIdRef.current ?? undefined} /></div>
            <h2 className="subhead">필수 확인 항목</h2>
            <div className="field-group">
              {RIGHTS_CHECKS.map(([k, label]) => (
                <label key={k} className="check-line">
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
                    <label key={k} className="check-line">
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
            <div className="notice">
              허락서·동의서는 위의 AUDENIQ 전자 문서 도구에서 작성하고 권리자가 직접 서명할 수 있어요. 완성된 서류는 ‘권리·보완 서류’에도 보관돼요.
            </div>
            <OptionDocsBanner options={form.options} />
          </section>
        )}

        {step === 5 && (
          <section className="step-section">
            <div className="aq-catalog-cards aq-stagger">
              {reviewSections.map(([h, t, target]) => (
                <div key={h} className="aq-review-card">
                  <strong>{h}</strong>
                  <p className="break">{t}</p>
                  <button type="button" className="link-btn aq-review-edit" onClick={() => goStep(target)} aria-label={`${h} 수정`}>수정</button>
                </div>
              ))}
            </div>
            <div className="notice" style={{ marginTop: 24 }}>
              {resubmit
                ? '‘다시 접수하기’를 누르면 고친 내용으로 다시 검토를 받아요. 처음 낸 배급 신청서는 그대로 유지돼요.'
                : `아래에 서명하고 ‘${editId && origStatus !== 'draft' ? '서명하고 수정 완료' : '서명하고 접수하기'}’를 누르면 배급 신청서가 발급되고 권리 확인서가 준비돼요. 최종 승인과 배급일은 담당자 검토 후 확정돼요.`}
            </div>
            <FinalReviewBanner form={form} />

            {!resubmit && (
            <section className="aq-apply-sign" aria-labelledby="aqApplySignHead">
              <h2 className="subhead" id="aqApplySignHead">신청인 서명</h2>
              <p className="help">서명하면 입력한 내용으로 배급 신청서가 만들어지고, 접수 후 바로 확인·저장할 수 있어요.</p>
              <div className="form-grid">
                <div className="field">
                  <label htmlFor="f-signer">신청인 성명 <span className="required">*</span></label>
                  <input
                    id="f-signer" autoComplete="name" maxLength={80} value={signerName}
                    onChange={e => setSigner(v => ({ ...v, name: e.target.value, touched: true }))} placeholder="실명 또는 법인명"
                  />
                </div>
                <div className="field">
                  <label htmlFor="f-signer-role">신청인 구분</label>
                  <select id="f-signer-role" value={signer.role} onChange={e => setSigner(v => ({ ...v, role: e.target.value }))}>
                    {SIGNER_ROLES.map(r => <option key={r} value={r}>{r}</option>)}
                  </select>
                </div>
              </div>
              <div className="field">
                <label htmlFor="aqApplyPad">서명 <span className="required">*</span></label>
                <SignaturePad ref={signRef} id="aqApplyPad" label="신청인 서명 입력" height={170} onChange={setSigned} />
              </div>
              <div className="aq-agree-box">
                <label className="check-line aq-agree-all-line">
                  <input
                    type="checkbox" checked={AGREEMENTS.every(a => agreed[a.id])}
                    onChange={e => setAgreed(Object.fromEntries(AGREEMENTS.map(a => [a.id, e.target.checked])))}
                  />
                  <span><strong>아래 내용에 모두 동의해요</strong></span>
                </label>
                {AGREEMENTS.map(a => (
                  <label key={a.id} className="check-line">
                    <input
                      type="checkbox" id={`agree-${a.id}`} checked={!!agreed[a.id]}
                      onChange={e => setAgreed(v => ({ ...v, [a.id]: e.target.checked }))}
                    />
                    <span>{a.text}</span>
                  </label>
                ))}
              </div>
            </section>
            )}
          </section>
        )}
      </div>

      {error && <div id="wizardError" className="notice error aq-shake" role="alert" key={error}>{error}</div>}
      </div>
      </div>

      <div className="step-actions">
        <button type="button" id="wizardBack" className="button secondary" onClick={back} disabled={submitting}>
          {step === 0 ? (editId ? '나가기' : '홈으로') : '이전'}
        </button>
        {resubmit && step < STEPS.length - 1 && (
          // 이미 접수한 발매를 고칠 때는 끝까지 가지 않고 여기서 바로 저장(다시 접수)할 수 있다
          <button type="button" id="wizardSaveNow" className="button secondary" onClick={() => void submit()} disabled={submitting || loadingEdit}>
            {submitting ? '저장하는 중' : '수정 완료'}
          </button>
        )}
        <button
          type="button" id="wizardNext"
          className={`button${submitting ? ' is-busy' : ''}`}
          onClick={next} disabled={submitting || loadingEdit} aria-busy={submitting}
        >
          {submitting ? '접수하는 중' : step === STEPS.length - 1 ? (resubmit ? '다시 접수하기' : editId && origStatus !== 'draft' ? '서명하고 수정 완료' : '서명하고 접수하기') : '다음으로'}
        </button>
      </div>
      {electronic && <RightsDocumentModal kind={electronic.kind} context={electronic.context} onClose={() => setElectronic(null)} onComplete={doc => { setElectronic(null); setOpenDocId(doc.id); toast('권리자 서명 문서가 완성됐어요. AUDENIQ 검토가 이어져요.', 'success'); }} />}
      {openDoc && <DocumentModal doc={openDoc} onClose={() => setOpenDocId(null)} onOpenSignature={() => {}} />}
    </div>
    </section>
  );
}
