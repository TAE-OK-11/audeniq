import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useToast } from '../components/Toast';
import { useProgressFill } from '../hooks/useAnimations';
import { mockApi } from '../api/mock';
import { addDoc, type DocRecord } from '../store/docs';
import { localStamp } from '../lib/format';

const STEPS = [
  { short: '발매 정보', kicker: '01 / 06 · 발매 정보', title: '어떤 음악을\n발매할까요?', sub: '발매 정보와 아티스트명을 입력해 주세요.' },
  { short: '트랙 등록', kicker: '02 / 06 · 트랙 등록', title: '발매할 곡을\n등록해 주세요.', sub: '곡별 음원 파일과 크레딧을 입력해 주세요.' },
  { short: '커버아트', kicker: '03 / 06 · 커버아트', title: '커버아트를\n등록해 주세요.', sub: '정사각형 커버아트를 등록해 주세요.' },
  { short: '배급 설정', kicker: '04 / 06 · 배급 설정', title: '언제, 어디에\n발매할까요?', sub: '발매일과 배급할 플랫폼을 선택해 주세요.' },
  { short: '권리 확인', kicker: '05 / 06 · 권리 확인', title: '음악의 권리를\n확인해 주세요.', sub: '권리자 정보와 필요한 증빙을 준비해 주세요.' },
  { short: '최종 확인', kicker: '06 / 06 · 최종 확인', title: '발매 정보를\n마지막으로 확인해 주세요.', sub: '입력한 정보와 빠진 항목을 확인해 주세요.' },
];

const KINDS = [['single', '싱글'], ['ep', 'EP'], ['album', '정규 앨범'], ['compilation', '컴필레이션']];
const LANGUAGES = [['ko', '한국어'], ['en', '영어'], ['ja', '일본어'], ['other', '기타']];
const DSP: [string, string][] = [
  ['melon', '멜론'], ['genie', '지니'], ['flo', 'FLO'], ['bugs', '벅스'],
  ['spotify', 'Spotify'], ['apple', 'Apple Music / iTunes'], ['youtube', 'YouTube Music'],
  ['amazon', 'Amazon Music'], ['tidal', 'TIDAL'], ['deezer', 'Deezer'], ['qobuz', 'Qobuz'],
];
const GENRES: [string, string][] = [
  ['', '장르를 선택해 주세요'], ['Pop', '팝'], ['K-Pop', 'K-Pop'], ['Indie Pop', '인디 팝'],
  ['Rock', '록'], ['Indie Rock', '인디 록'], ['Alternative', '얼터너티브'], ['Hip-Hop', '힙합'],
  ['R&B / Soul', 'R&B / 소울'], ['Electronic', '일렉트로닉'], ['Dance', '댄스'], ['Jazz', '재즈'],
  ['Classical', '클래식'], ['Folk', '포크'], ['Acoustic', '어쿠스틱'], ['Ballad', '발라드'],
  ['Metal', '메탈'], ['Punk', '펑크'], ['Blues', '블루스'], ['Reggae', '레게'],
  ['Latin', '라틴'], ['World', '월드'], ['New Age', '뉴에이지'], ['OST / Soundtrack', 'OST / 사운드트랙'],
  ['Children', '어린이 음악'], ['Religious', '종교음악'], ['Ambient', '앰비언트'],
  ['Instrumental', '연주곡'], ['Spoken Word', '낭독 / 스포큰 워드'], ['__other__', '기타 · 직접 입력'],
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
}

interface CoverTrackInfo {
  trackId: string;
  originalTitle: string;
  originalArtist: string;
  originalWriters: string;
}

interface ReleaseOptions {
  express: boolean; expressAck: boolean; expressReason: string;
  minor: boolean;
  guardian: string; guardianRelation: string; guardianContact: string;
  guardian2: string; guardian2Relation: string; guardian2Contact: string;
  guardianConsentDone: boolean; familyCertName: string; familyCertMethod: string;
  cover: boolean; coverTracks: CoverTrackInfo[]; coverRightsAck: boolean; coverLicenseFile: string;
  sample: boolean; sampleLicenseFile: string;
  featured: boolean; featuredConsentFile: string;
  ai: boolean; aiTool: string;
  shared: boolean; sharedContractFile: string;
  rerelease: boolean; previousTitle: string; previousId: string;
}

interface WizardForm {
  artist: string; title: string; type: string; language: string; genre: string;
  genreCustom: string; label: string; notes: string;
  tracks: Track[];
  coverName: string; coverData: string;
  releaseDate: string; originalDate: string; upc: string;
  territories: string[]; platforms: string[];
  ownership: string; phonogram: string; copyright: string;
  rightsChecks: Record<string, boolean>;
  options: ReleaseOptions;
}

const newTrack = (): Track => ({
  id: 't' + Math.random().toString(36).slice(2, 9),
  title: '', version: '', isrc: '', composers: '', lyricists: '', arrangers: '', performers: '',
  producer: '',
  lyrics: '',
  audioName: '', audioSize: 0, explicit: false, duration: '',
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
  ai: false, aiTool: '',
  shared: false, sharedContractFile: '',
  rerelease: false, previousTitle: '', previousId: '',
};

const EMPTY: WizardForm = {
  artist: '', title: '', type: 'single', language: 'ko', genre: '', genreCustom: '',
  label: '', notes: '',
  tracks: [newTrack()],
  coverName: '', coverData: '',
  releaseDate: '', originalDate: '', upc: '',
  territories: ['WORLD'], platforms: DSP.map(d => d[0]),
  ownership: '', phonogram: '', copyright: '',
  rightsChecks: {},
  options: EMPTY_OPTIONS,
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
  ['rerelease', '기존 발매 이전·재발매', '이전 발매의 식별자와 중복 송출 여부를 확인해요.'],
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

function stampNow(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

function todayStr(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

function formatKoreanDate(iso: string): string {
  if (!iso) return '';
  const [y, m, d] = iso.split('-').map(Number);
  if (!y || !m || !d) return iso;
  return `${y}년 ${m}월 ${d}일`;
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

function DocAttach({ id, label, fileName, onSelect, required, help }: {
  id: string; label: React.ReactNode; fileName: string;
  onSelect: (name: string) => void; required?: boolean; help?: string;
}) {
  return (
    <div className="field doc-attach">
      <label htmlFor={id}>{label}{required && <> <span className="required">*</span></>}</label>
      <input
        type="file" id={id}
        accept=".pdf,.png,.jpg,.jpeg,.webp,application/pdf,image/*"
        onChange={e => {
          const f = e.target.files?.[0];
          if (f) onSelect(f.name);
        }}
      />
      <p className="help">
        {fileName ? `선택한 서류 · ${fileName}` : (help || '서류를 첨부해 주세요.')}
      </p>
    </div>
  );
}

function GuardianConsentModal({ guardianName, onClose, onComplete }: {
  guardianName: string;
  onClose: () => void;
  onComplete: (certName: string, certMethod: string) => void;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [signed, setSigned] = useState(false);
  const [certName, setCertName] = useState('');
  const [certMethod, setCertMethod] = useState('');
  const [ack, setAck] = useState(false);
  const cameraRef = useRef<HTMLInputElement>(null);
  const pdfRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ratio = Math.min(2, window.devicePixelRatio || 1);
    const rect = canvas.getBoundingClientRect();
    canvas.width = Math.max(1, Math.round(rect.width * ratio));
    canvas.height = Math.max(1, Math.round(rect.height * ratio));
    const ctx = canvas.getContext('2d')!;
    ctx.scale(ratio, ratio);
    ctx.lineWidth = 2.5;
    ctx.lineCap = 'round';
    ctx.strokeStyle = '#1c2740';
  }, []);

  const pos = (e: React.PointerEvent) => {
    const r = canvasRef.current!.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };
  const drawing = useRef(false);
  const startDraw = (e: React.PointerEvent) => {
    drawing.current = true;
    canvasRef.current!.setPointerCapture(e.pointerId);
    const p = pos(e);
    const ctx = canvasRef.current!.getContext('2d')!;
    ctx.beginPath();
    ctx.moveTo(p.x, p.y);
  };
  const moveDraw = (e: React.PointerEvent) => {
    if (!drawing.current) return;
    const p = pos(e);
    const ctx = canvasRef.current!.getContext('2d')!;
    ctx.lineTo(p.x, p.y);
    ctx.stroke();
    setSigned(true);
  };
  const endDraw = () => { drawing.current = false; };
  const clearSign = () => {
    const canvas = canvasRef.current!;
    const ctx = canvas.getContext('2d')!;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    setSigned(false);
  };

  const pickCert = (method: string, name: string) => {
    setCertMethod(method);
    setCertName(name);
  };

  const canComplete = signed && certName && ack;

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-card guardian-modal" onClick={e => e.stopPropagation()}>
        <div className="modal-head">
          <h2>법정대리인 동의</h2>
          <button type="button" className="icon-button" onClick={onClose} aria-label="닫기">✕</button>
        </div>
        <div className="modal-body">
          <p className="guardian-consent-text">
            미성년 아티스트의 음원 발매에 대해 법정대리인
            {guardianName ? <strong> {guardianName}</strong> : ''}님의 동의를 확인합니다.
            아래 내용을 확인하고 서명해 주세요.
          </p>
          <label className="check-line">
            <input type="checkbox" checked={ack} onChange={e => setAck(e.target.checked)} />
            <span>미성년자의 음원 발매 및 배급에 법정대리인으로서 동의합니다.<small>동의 내용은 발매 심사 시 확인돼요.</small></span>
          </label>
          <div className="field" style={{ marginTop: 16 }}>
            <label>법정대리인 전자서명 <span className="required">*</span></label>
            <div className="sign-pad-wrap">
              <canvas
                ref={canvasRef} className="sign-pad"
                onPointerDown={startDraw} onPointerMove={moveDraw}
                onPointerUp={endDraw} onPointerCancel={endDraw}
              />
              {!signed && <span className="sign-pad-hint">여기에 서명해 주세요.</span>}
            </div>
            <button type="button" className="link-btn" onClick={clearSign} style={{ marginTop: 8 }}>서명 지우기</button>
          </div>
          <div className="field" style={{ marginTop: 16 }}>
            <label>가족관계증명서 <span className="required">*</span></label>
            <div className="cert-methods">
              <button type="button" className={`cert-method${certMethod === 'camera' ? ' active' : ''}`} onClick={() => cameraRef.current?.click()}>
                <span className="cert-icon">📷</span>
                <span>직접 촬영</span>
              </button>
              <button type="button" className={`cert-method${certMethod === 'pdf' ? ' active' : ''}`} onClick={() => pdfRef.current?.click()}>
                <span className="cert-icon">📄</span>
                <span>PDF 선택</span>
              </button>
              <button type="button" className={`cert-method${certMethod === 'wallet' ? ' active' : ''}`} onClick={() => pickCert('wallet', '전자문서지갑에서 가져옴')}>
                <span className="cert-icon">👛</span>
                <span>전자문서지갑</span>
              </button>
            </div>
            <input
              ref={cameraRef} type="file" accept="image/*" capture="environment"
              style={{ display: 'none' }}
              onChange={e => {
                const f = e.target.files?.[0];
                if (f) pickCert('camera', f.name);
              }}
            />
            <input
              ref={pdfRef} type="file" accept=".pdf,application/pdf"
              style={{ display: 'none' }}
              onChange={e => {
                const f = e.target.files?.[0];
                if (f) pickCert('pdf', f.name);
              }}
            />
            {certName && <p className="help" style={{ marginTop: 8 }}>선택됨 · {certName}</p>}
            {certMethod === 'wallet' && (
              <p className="help" style={{ marginTop: 8 }}>전자문서지갑 연동 후 가져올 수 있어요. 현재는 선택 상태로 저장돼요.</p>
            )}
          </div>
        </div>
        <div className="modal-foot">
          <button type="button" className="button secondary" onClick={onClose}>취소</button>
          <button
            type="button" className="button"
            disabled={!canComplete}
            onClick={() => onComplete(certName, certMethod)}
          >동의 완료</button>
        </div>
      </div>
    </div>
  );
}

function OptionsSection({ form, set }: {
  form: WizardForm;
  set: <K extends keyof WizardForm>(key: K, value: WizardForm[K]) => void;
}) {
  const o = form.options;
  const setOpt = <K extends keyof ReleaseOptions>(key: K, value: ReleaseOptions[K]) =>
    set('options', { ...o, [key]: value });
  const [showGuardianModal, setShowGuardianModal] = useState(false);

  return (
    <>
      <h2 className="subhead" style={{ marginTop: 25 }}>부가서비스</h2>
      <p className="aq-option-intro">필요한 서비스를 선택해 주세요.</p>
      <div className="aq-options">
        {SERVICE_OPTIONS.map(([id, title, sub]) => (
          <label key={id} className="aq-option">
            <span className="aq-option-text"><strong>{title}</strong><small>{sub}</small></span>
            <input
              type="checkbox" aria-label={title}
              checked={!!o[id]}
              onChange={e => setOpt(id, e.target.checked as ReleaseOptions[typeof id])}
            />
          </label>
        ))}
      </div>
      <h2 className="subhead" style={{ marginTop: 25 }}>권리 확인</h2>
      <p className="aq-option-intro">해당하는 항목을 모두 선택해 주세요. 필요한 확인 사항과 서류가 자동으로 안내돼요.</p>
      <div className="aq-options">
        {RIGHTS_OPTIONS.map(([id, title, sub]) => (
          <label key={id} className="aq-option">
            <span className="aq-option-text"><strong>{title}</strong><small>{sub}</small></span>
            <input
              type="checkbox" aria-label={title}
              checked={!!o[id]}
              onChange={e => setOpt(id, e.target.checked as ReleaseOptions[typeof id])}
            />
          </label>
        ))}
      </div>
      {o.express && (
        <div className="aq-option-detail">
          <h3>신속 발매 요청</h3>
          <p className="aq-option-intro">신속 발매는 가능 여부와 조건을 확인한 뒤 진행해요. 특정 발매일이나 플랫폼 송출을 보장하지 않아요.</p>
          <div className="field">
            <label htmlFor="aqExpressReason">신속 발매가 필요한 이유 (선택)</label>
            <input
              id="aqExpressReason" maxLength={300} value={o.expressReason}
              onChange={e => setOpt('expressReason', e.target.value)}
              placeholder="예: 공연 일정에 맞춰 발매하고 싶어요."
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
      )}
      {o.minor && (
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
              {o.guardianConsentDone ? '법정대리인 동의 완료 ✓ (다시 진행)' : '법정대리인 동의 진행하기'}
            </button>
            <p className="help">
              {o.guardianConsentDone
                ? `동의 완료${o.familyCertName ? ` · 가족관계증명서: ${o.familyCertName}` : ''}`
                : '동의창에서 법정대리인 서명과 가족관계증명서를 제출해요.'}
            </p>
          </div>
          <p className="aq-option-note">법정대리인 정보 입력만으로 본인 확인이나 동의 검증이 완료되지는 않아요. 담당자 확인 후 서명 단계를 안내해요.</p>
        </div>
      )}
      {showGuardianModal && (
        <GuardianConsentModal
          guardianName={o.guardian}
          onClose={() => setShowGuardianModal(false)}
          onComplete={(certName, certMethod) => {
            setOpt('guardianConsentDone', true);
            setOpt('familyCertName', certName);
            setOpt('familyCertMethod', certMethod);
            setShowGuardianModal(false);
          }}
        />
      )}
      {o.cover && (
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
            id="aqCoverLicense" label="원곡 이용 허락서 (보유 시)"
            fileName={o.coverLicenseFile}
            onSelect={v => setOpt('coverLicenseFile', v)}
            help="이용 허락서나 라이선스 계약서를 첨부해 주세요."
          />
        </div>
      )}
      {o.sample && (
        <div className="aq-option-detail">
          <h3>샘플링·타인 음원 사용</h3>
          <p className="aq-option-intro">사용한 원본 음원과 저작물의 출처를 확인해요.</p>
          <DocAttach
            id="aqSampleLicense" label="원본 이용 허락서"
            fileName={o.sampleLicenseFile}
            onSelect={v => setOpt('sampleLicenseFile', v)}
            help="샘플링 원본의 이용 허락서나 라이선스 계약서를 첨부해 주세요."
          />
          <p className="aq-option-note">허락 없는 샘플링은 저작권 침해가 될 수 있어요.</p>
        </div>
      )}
      {o.featured && (
        <div className="aq-option-detail">
          <h3>피처링·공동 실연</h3>
          <p className="aq-option-intro">참여자의 크레딧과 이용 허락을 확인해요.</p>
          <DocAttach
            id="aqFeaturedConsent" label="참여자 동의서 (보유 시)"
            fileName={o.featuredConsentFile}
            onSelect={v => setOpt('featuredConsentFile', v)}
            help="피처링 참여자의 동의서나 계약서를 첨부해 주세요."
          />
        </div>
      )}
      {o.shared && (
        <div className="aq-option-detail">
          <h3>공동 권리자·레이블 계약</h3>
          <p className="aq-option-intro">각 권리자와의 배급 위임 범위를 확인해요.</p>
          <DocAttach
            id="aqSharedContract" label="공동 권리 계약서"
            fileName={o.sharedContractFile}
            onSelect={v => setOpt('sharedContractFile', v)}
            help="배급 위임 범위가 명시된 계약서를 첨부해 주세요."
          />
        </div>
      )}
      {o.rerelease && (
        <div className="aq-option-detail">
          <h3>기존 발매 정보</h3>
          <div className="field">
            <label htmlFor="aqPreviousTitle">기존 발매명</label>
            <input
              id="aqPreviousTitle" maxLength={180} value={o.previousTitle}
              onChange={e => setOpt('previousTitle', e.target.value)}
              placeholder="기존 앨범·싱글 제목"
            />
          </div>
          <div className="field">
            <label htmlFor="f-upc">기존 UPC / EAN (있는 경우)</label>
            <input id="f-upc" value={form.upc} onChange={e => set('upc', e.target.value)} maxLength={20} placeholder="없으면 비워두세요." />
          </div>
          <div className="field">
            <label htmlFor="aqPreviousId">기존 ISRC (보유 시)</label>
            <input
              id="aqPreviousId" maxLength={100} value={o.previousId}
              onChange={e => setOpt('previousId', e.target.value)}
              placeholder="이전 식별자"
            />
          </div>
          <KoreanDateField
            id="aqOriginalDate" label="최초 발매일"
            value={form.originalDate}
            onChange={v => set('originalDate', v)}
          />
          <p className="help">기존 발매 중복과 스트리밍 매칭 여부를 별도로 확인해요.</p>
        </div>
      )}
      {o.ai && (
        <div className="aq-option-detail">
          <h3>AI 활용 내역</h3>
          <div className="field">
            <label htmlFor="aqAiTool">사용한 도구와 활용 방식 <span className="required">*</span></label>
            <textarea
              id="aqAiTool" maxLength={600} rows={2} value={o.aiTool}
              onChange={e => setOpt('aiTool', e.target.value)}
              placeholder="도구명, AI가 생성하거나 보조한 부분을 입력해 주세요."
            />
          </div>
          <p className="help">이용 약관과 상업적 이용 허가 자료를 요청할 수 있어요. 모든 배급 플랫폼이 AI 음악을 받는 것은 아니에요.</p>
        </div>
      )}
      <p className="aq-option-note">선택한 내용에 따라 권리·계약 서류가 별도로 준비돼요. 신속 발매와 미성년자 발매를 함께 선택할 수도 있어요.</p>
    </>
  );
}

function OptionDocsBanner({ options }: { options: ReleaseOptions }) {
  const chosen = selectedOptions(options);
  if (!chosen.length) return null;
  return (
    <div className="aq-req-banner">
      <h3>추가 확인이 필요한 항목</h3>
      <p>선택하신 발매 조건에 맞는 서류가 권리 증빙 메뉴에 준비돼요. 원본 확인과 검토는 별도로 진행돼요.</p>
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
  if (chosen.length) warnings.push('선택한 추가 옵션의 관련 서류는 권리 증빙에서 제출');
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

export function Upload() {
  const nav = useNavigate();
  const toast = useToast();
  const [step, setStep] = useState(0);
  const [form, setForm] = useState<WizardForm>(EMPTY);
  const [error, setError] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [draftId, setDraftId] = useState<string | null>(null);
  const [expandedTracks, setExpandedTracks] = useState<Set<string>>(new Set());
  const coverInputRef = useRef<HTMLInputElement>(null);
  const progressRef = useProgressFill(step);

  // 라이브와 동일: 위자드에서는 헤더 숨김 + 레이아웃 패딩 제거
  useEffect(() => {
    document.body.classList.add('wizard-mode');
    return () => document.body.classList.remove('wizard-mode');
  }, []);

  // 과거 발매 예정일이 저장돼 있으면 비움
  useEffect(() => {
    if (form.releaseDate && form.releaseDate < todayStr()) {
      setForm(f => ({ ...f, releaseDate: '' }));
    }
  }, [form.releaseDate]);

  const set = <K extends keyof WizardForm>(key: K, value: WizardForm[K]) =>
    setForm(f => ({ ...f, [key]: value }));

  const setTrack = (id: string, key: keyof Track, value: string | boolean | number) =>
    setForm(f => ({ ...f, tracks: f.tracks.map(t => (t.id === id ? { ...t, [key]: value } : t)) }));

  const fail = (msg: string, sel: string | null): boolean => {
    setError(msg);
    requestAnimationFrame(() => {
      if (sel) {
        const el = document.querySelector<HTMLElement>(sel);
        if (el) { el.focus(); return; }
      }
      // 포커스 대상이 없으면 에러 메시지로 스크롤
      document.getElementById('wizardError')?.scrollIntoView({ behavior: 'smooth', block: 'center' });
    });
    return false;
  };

  const validate = (): boolean => {
    if (step === 0) {
      if (!form.artist.trim()) return fail('아티스트명을 입력해 주세요.', '#f-artist');
      if (!form.title.trim()) return fail('발매 제목을 입력해 주세요.', '#f-title');
      const genreVal = form.genre === '__other__' ? form.genreCustom : form.genre;
      if (!genreVal.trim()) return fail('장르를 선택해 주세요.', '#f-genre');
    }
    if (step === 1) {
      for (let i = 0; i < form.tracks.length; i++) {
        const t = form.tracks[i];
        if (!t.title.trim()) return fail(`트랙 ${i + 1}의 곡 제목을 입력해 주세요.`, `#tr-${i}-title`);
        if (!t.composers.trim()) return fail(`트랙 ${i + 1}의 작곡자를 입력해 주세요.`, `#tr-${i}-composers`);
        if (!t.audioName) return fail(`트랙 ${i + 1}의 음원 파일을 선택해 주세요.`, `#trackFile-${i}`);
      }
    }
    if (step === 2 && !form.coverName) return fail('커버아트를 등록해 주세요.', '#coverFile');
    if (step === 3) {
      if (!form.releaseDate) return fail('발매일을 선택해 주세요.', '#f-releaseDate');
      if (form.releaseDate < todayStr()) return fail('발매 예정일은 오늘 이후로 선택해 주세요.', '#f-releaseDate');
      if (!form.platforms.length) return fail('배급할 플랫폼을 하나 이상 선택해 주세요.', null);
      const o = form.options;
      if (o.express && !o.expressAck) return fail('신속 발매 안내를 확인해 주세요.', '#aqExpressAck');
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
      if (o.ai && !o.aiTool.trim()) return fail('AI 도구명과 활용 방식을 입력해 주세요.', '#aqAiTool');
      if (o.cover) {
        if (!o.coverTracks.length) return fail('커버곡에 해당하는 트랙을 하나 이상 선택해 주세요.', null);
        for (const c of o.coverTracks) {
          const idx = form.tracks.findIndex(t => t.id === c.trackId);
          if (!c.originalTitle.trim()) return fail(`트랙 ${idx + 1}의 원곡 제목을 입력해 주세요.`, `#cover-orig-title-${idx}`);
          if (!c.originalArtist.trim()) return fail(`트랙 ${idx + 1}의 원곡 아티스트를 입력해 주세요.`, `#cover-orig-artist-${idx}`);
        }
        if (!o.coverRightsAck) return fail('커버곡 권리 확인을 체크해 주세요.', '#aqCoverRightsAck');
      }
    }
    if (step === 4) {
      if (!form.ownership.trim()) return fail('음원 권리자를 입력해 주세요.', '#f-ownership');
      if (!form.phonogram.trim()) return fail('℗ 표기를 입력해 주세요.', '#f-phonogram');
      if (!form.copyright.trim()) return fail('© 표기를 입력해 주세요.', '#f-copyright');
      if (!rightsOk(form)) return fail('권리 확인 항목을 모두 확인해 주세요.', null);
    }
    setError('');
    return true;
  };

  /** 라이브 ensureReleaseDocuments — 접수된 발매의 계약서·권리 서류를 준비 */
  const ensureReleaseDocuments = (f: WizardForm, releaseId: string) => {
    const title = f.title.trim() || '제목 없는 발매';
    const genreVal = f.genre === '__other__' ? f.genreCustom.trim() : f.genre;
    const at = stampNow();
    const lines = [
      '발매 신청 정보',
      `신청한 발매: ${title}`,
      `아티스트: ${f.artist.trim() || '미입력'}`,
      `앨범 구분: ${KINDS.find(k => k[0] === f.type)?.[1] || f.type}`,
      `장르: ${genreVal || '미입력'}`,
      `발매 희망일: ${f.releaseDate || '미정'}`,
      `레이블: ${f.label.trim() || '미입력'}`,
      `수록곡: ${f.tracks.map((t, i) => `${i + 1}. ${t.title.trim() || '제목 없음'} / ${t.duration || '길이 확인 전'} / ${t.composers.trim() || '작곡자 미입력'}`).join(' · ')}`,
      `배급 플랫폼: ${f.platforms.map(p => DSP.find(d => d[0] === p)?.[1] || p).join(', ')}`,
      `마스터 권리자: ${f.ownership.trim() || '미입력'}`,
      `℗ 권리 표기: ${f.phonogram.trim() || '미입력'}`,
      `© 권리 표기: ${f.copyright.trim() || '미입력'}`,
      `신청일: ${localStamp(at)}`,
      '권리 확인: 신청자는 음원, 작사·작곡, 커버 이미지 및 제3자 권리 이용에 필요한 허락을 확인하고 증빙 요청 시 제출할 것을 확인합니다.',
    ];
    const agreement: DocRecord = {
      id: 'doc' + Date.now(), kind: 'agreements',
      title: title + ' · 발매 신청 및 권리 확인서',
      releaseId, releaseTitle: title, version: '1.0', created: at,
      content: lines.join('\n'), fileName: '', checked: false, checkedAt: '',
      consentHistory: [],
      reviewHistory: [{ status: '접수 요청', time: at, detail: '신청서가 작성됐어요. 담당자 검토 접수는 전송 후 시작됩니다.' }],
      reviewStatus: 'prepared', reviewNote: '', signerName: '', localSignatureData: '', localSignatureAt: '',
    };
    const rights: DocRecord = {
      id: 'doc' + Date.now() + '-r', kind: 'rights',
      title: title + ' · 권리 증빙 제출',
      releaseId, releaseTitle: title, version: '1.0', created: at,
      content: '발매 권리를 확인할 수 있는 자료를 제출해 주세요. 해당하는 자료: 마스터 음원 제작 또는 이용 허락서, 커버아트 사용 허락서, 공동 창작·피처링 또는 커버곡의 권리 허락서. 필요한 자료만 제출하면 돼요.',
      fileName: '', checked: false, checkedAt: '',
      consentHistory: [],
      reviewHistory: [{ status: '서류 접수 대기', time: at, detail: '권리 관련 증빙이 필요한 경우 제출해 주세요.' }],
      reviewStatus: 'awaiting_documents', reviewNote: '', signerName: '', localSignatureData: '', localSignatureAt: '',
    };
    addDoc(rights);
    addDoc(agreement);
  };

  // 자동 임시 저장 (조용히, 토스트 없음)
  const autoSave = async () => {
    try {
      const data = {
        title: form.title.trim() || '제목 없음',
        release_date: form.releaseDate || '',
      };
      if (draftId) {
        await mockApi.updateRelease(draftId, data);
      } else {
        const r = await mockApi.createRelease(data);
        setDraftId(r.id);
      }
    } catch {
      // 자동 저장 실패는 조용히 무시
    }
  };

  const next = async () => {
    if (!validate()) return;
    // 단계 넘어갈 때 자동 임시 저장
    await autoSave();
    if (step === STEPS.length - 1) {
      if (submitting) return;
      setSubmitting(true);
      try {
        const r = await mockApi.submitRelease({
          title: form.title.trim(),
          artist: form.artist.trim(),
          type: form.type,
          genre: form.genre === '__other__' ? form.genreCustom.trim() : form.genre,
          label: form.label.trim(),
          upc: form.upc.trim(),
          notes: form.notes.trim(),
          coverName: form.coverName,
          release_date: form.releaseDate || '',
          tracks: form.tracks.map(t => ({
            id: t.id, title: t.title.trim(), isrc: t.isrc.trim(), duration: t.duration,
            version: t.version.trim(), composers: t.composers.trim(),
            lyricists: t.lyricists.trim(), audioName: t.audioName,
            producer: t.producer.trim(),
            lyrics: t.lyrics.trim(),
          })),
          territories: form.territories,
          platforms: form.platforms,
          ownership: form.ownership.trim(),
          phonogram: form.phonogram.trim(),
          copyright: form.copyright.trim(),
          rightsChecks: form.rightsChecks,
        });
        ensureReleaseDocuments(form, r.id);
        toast('발매 신청이 접수됐어요.');
        nav(`/releases/${r.id}`);
      } finally {
        setSubmitting(false);
      }
      return;
    }
    setStep(s => s + 1);
    window.scrollTo({ top: 0 });
  };

  const back = () => {
    if (step === 0) { nav('/'); return; }
    setStep(s => s - 1);
    setError('');
    window.scrollTo({ top: 0 });
  };

  const onCoverChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    // 커버 파일 타입 검증 (JPG/PNG/WEBP만 허용)
    const okType = /^image\/(jpeg|png|webp)$/i.test(file.type);
    const okExt = /\.(jpe?g|png|webp)$/i.test(file.name);
    if (!okType && !okExt) {
      toast('커버는 JPG, PNG, WEBP 파일만 등록할 수 있어요.');
      e.target.value = '';
      return;
    }
    const reader = new FileReader();
    reader.onload = () => {
      setForm(f => ({ ...f, coverName: file.name, coverData: String(reader.result) }));
      toast('커버 이미지가 등록됐어요.');
    };
    reader.readAsDataURL(file);
  };

  const onTrackAudio = (id: string, e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    // 오디오 duration 자동 추출
    const url = URL.createObjectURL(file);
    const audio = new Audio();
    audio.preload = 'metadata';
    audio.onloadedmetadata = () => {
      const secs = Math.round(audio.duration || 0);
      const mm = String(Math.floor(secs / 60)).padStart(2, '0');
      const ss = String(secs % 60).padStart(2, '0');
      setForm(f => ({
        ...f,
        tracks: f.tracks.map(t => (t.id === id ? { ...t, audioName: file.name, audioSize: file.size, duration: `${mm}:${ss}` } : t)),
      }));
      URL.revokeObjectURL(url);
      // 1곡 등록될 때마다 자동 임시 저장
      autoSave();
    };
    audio.onerror = () => {
      setForm(f => ({
        ...f,
        tracks: f.tracks.map(t => (t.id === id ? { ...t, audioName: file.name, audioSize: file.size } : t)),
      }));
      URL.revokeObjectURL(url);
      // 1곡 등록될 때마다 자동 임시 저장
      autoSave();
    };
    audio.src = url;
    toast('음원 파일이 등록됐어요.');
  };

  const s = STEPS[step];
  const genreIsCustom = form.genre === '__other__';
  const allPlatforms = DSP.every(d => form.platforms.includes(d[0]));

  const reviewSections: [string, string][] = [
    ['발매 정보', `${form.title || '제목 없음'} · ${form.artist || '아티스트 없음'} · ${KINDS.find(k => k[0] === form.type)?.[1] || form.type}`],
    ['트랙', form.tracks.map(t => t.title || '곡명 없음').join(' / ')],
    ['커버아트', form.coverName || '등록되지 않음'],
    ['발매일', form.releaseDate || '지정하지 않음'],
    ['배급 대상', form.platforms.map(p => DSP.find(d => d[0] === p)?.[1] || p).join(', ') || '선택하지 않음'],
    ['권리자', form.ownership || '미입력'],
    ['권리 확인', rightsOk(form) ? '필수 확인 완료' : '필수 확인 항목 누락'],
  ];

  return (
    <section id="view-new" className="view">
    <div className="wizard">
      <div className="wizard-topbar" aria-label="발매 신청 탐색">
        <button type="button" id="wizardTopBack" className="wizard-topback" aria-label="이전으로 돌아가기" onClick={back}>
          <BackIcon />
        </button>
        <span className="wizard-top-title">새로운 발매</span>
        <span className="wizard-top-count" id="wizardTopCount">{step + 1} / 6</span>
      </div>

      <div className="wizard-progress" id="wizardProgress" ref={progressRef} aria-label="발매 신청 진행 단계">
        {STEPS.map((s, i) => (
          <span key={i} className={`wizard-progress-seg${i <= step ? ' current' : ''}`}>
            {i === step && <em>{s.short}</em>}
          </span>
        ))}
      </div>

      <div className="wizard-header">
        <h1 id="newTitle" style={{ whiteSpace: 'pre-line' }}>{s.title}</h1>
        <p id="wizardSubtitle">{s.sub}</p>
      </div>

      <div id="wizardBody" aria-live="polite">
        {step === 0 && (
          <section className="step-section">
            <div className="form-grid">
              <div className="field">
                <label htmlFor="f-artist">아티스트명 <span className="required">*</span></label>
                <input id="f-artist" value={form.artist} onChange={e => set('artist', e.target.value)} maxLength={120} placeholder="예: AUDENIQ" />
              </div>
              <div className="field">
                <label htmlFor="f-title">발매 제목 <span className="required">*</span></label>
                <input id="f-title" value={form.title} onChange={e => set('title', e.target.value)} maxLength={180} placeholder="싱글 또는 앨범 제목" />
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
                      requestAnimationFrame(() => {
                        document.getElementById('f-genre-custom')?.focus();
                      });
                    }
                  }}
                  required aria-describedby="genreHelp"
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
                    style={{ marginTop: 10 }}
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
            </div>
          </section>
        )}

        {step === 1 && (
          <section className="step-section">
            <div id="trackEditors">
              {form.tracks.map((t, i) => {
                const expanded = expandedTracks.has(t.id);
                const toggleExpand = () => {
                  setExpandedTracks(prev => {
                    const next = new Set(prev);
                    if (next.has(t.id)) next.delete(t.id);
                    else next.add(t.id);
                    return next;
                  });
                };
                return (
                <article key={t.id} className="track-editor">
                  <div className="minor-actions">
                    <h3>트랙 {i + 1}</h3>
                    {form.tracks.length > 1 && (
                      <button
                        type="button" className="link-btn"
                        onClick={() => set('tracks', form.tracks.filter(x => x.id !== t.id))}
                      >삭제</button>
                    )}
                  </div>
                  <div className="field track-title-field">
                    <label htmlFor={`tr-${i}-title`}>곡 제목 <span className="required">*</span></label>
                    <input id={`tr-${i}-title`} value={t.title} onChange={e => setTrack(t.id, 'title', e.target.value)} placeholder="곡명을 입력해" maxLength={200} />
                  </div>
                  <div className="field">
                    <label htmlFor={`tr-${i}-composers`}>작곡 <span className="required">*</span></label>
                    <input id={`tr-${i}-composers`} value={t.composers} onChange={e => setTrack(t.id, 'composers', e.target.value)} placeholder="참여자 이름을 쉼표로 구분" maxLength={200} />
                  </div>
                  <div className="field">
                    <label htmlFor={`trackFile-${i}`}>음원 파일 <span className="required">*</span></label>
                    <input
                      type="file" id={`trackFile-${i}`}
                      accept="audio/wav,audio/x-wav,audio/flac,audio/aiff,audio/x-aiff,audio/mpeg,audio/mp4,audio/*"
                      onChange={e => onTrackAudio(t.id, e)}
                    />
                    <p className="help" id={`audioLabel-${i}`}>
                      {t.audioName ? `${t.audioName}${t.audioSize ? ` · ${Math.round(t.audioSize / 1024 / 1024 * 100) / 100}MB` : ''}` : '선택한 파일 없음'}. 권장: 무손실 WAV/FLAC 파일, 최종 QC 후 송출.
                    </p>
                  </div>
                  <div className="track-duration-label" aria-live="polite">
                    {t.duration ? `곡 길이 · ${t.duration}` : '음원을 선택하면 곡 길이를 자동으로 확인해요.'}
                  </div>
                  <button
                    type="button" className="track-detail-toggle"
                    onClick={toggleExpand}
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
                          <label htmlFor={`tr-${i}-lyricists`}>작사</label>
                          <input id={`tr-${i}-lyricists`} value={t.lyricists} onChange={e => setTrack(t.id, 'lyricists', e.target.value)} placeholder="가사가 없는 곡이라면 비워둬" maxLength={200} />
                        </div>
                        <div className="field">
                          <label htmlFor={`tr-${i}-arrangers`}>편곡</label>
                          <input id={`tr-${i}-arrangers`} value={t.arrangers} onChange={e => setTrack(t.id, 'arrangers', e.target.value)} placeholder="참여자 이름" maxLength={200} />
                        </div>
                        <div className="field">
                          <label htmlFor={`tr-${i}-performers`}>실연자 / 피처링</label>
                          <input id={`tr-${i}-performers`} value={t.performers} onChange={e => setTrack(t.id, 'performers', e.target.value)} placeholder="참여자 이름" maxLength={200} />
                        </div>
                        <div className="field">
                          <label htmlFor={`tr-${i}-producer`}>프로듀서</label>
                          <input id={`tr-${i}-producer`} value={t.producer} onChange={e => setTrack(t.id, 'producer', e.target.value)} placeholder="프로듀서 이름" maxLength={200} />
                        </div>
                      </div>
                      <div className="field">
                        <label htmlFor={`tr-${i}-lyrics`}>가사 전문</label>
                        <textarea
                          id={`tr-${i}-lyrics`} value={t.lyrics}
                          onChange={e => setTrack(t.id, 'lyrics', e.target.value)}
                          rows={4} maxLength={10000} placeholder="가사 전체를 입력해 주세요"
                        />
                      </div>
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
              })}
            </div>
            <div className="spaced-actions">
              <button
                type="button" className="button secondary"
                onClick={() => {
                  set('tracks', [...form.tracks, newTrack()]);
                  toast('곡을 추가했어요.');
                }}
              >
                ＋ 트랙 추가
              </button>
              <span className="small muted">곡명·참여자·음원 파일 확인</span>
            </div>
          </section>
        )}

        {step === 2 && (
          <section className="step-section">
            <div className="field">
              <label htmlFor="coverFile">커버아트 <span className="required">*</span></label>
              <input
                type="file" id="coverFile" ref={coverInputRef}
                accept="image/png,image/jpeg,image/webp"
                onChange={onCoverChange}
              />
              <p className="help">권장: 정사각형 고해상도 JPG/PNG 이미지. 파일 용량에 별도 제한을 두지 않으며, 최종 배급 규격은 플랫폼별로 확인해요.</p>
            </div>
            {form.coverData && (
              <div className="cover-dist-preview">
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
                    set('coverName', ''); set('coverData', '');
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
                value={form.releaseDate} min={todayStr()}
                onChange={v => set('releaseDate', v)}
              />
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
            <div className="distribution-default">
              <div>
                <strong>주요 음악 플랫폼에 모두 배급해요.</strong>
                <p className="help">배급 가능한 플랫폼을 기본으로 선택했어요. 필요한 경우 선택을 변경할 수 있어요.</p>
              </div>
              <label className="aq-switch">
                <input
                  type="checkbox" aria-label="플랫폼 모두 선택"
                  checked={allPlatforms}
                  onChange={e => set('platforms', e.target.checked ? DSP.map(d => d[0]) : [])}
                />
                <span />
              </label>
            </div>
            <details className="studio-expand">
              <summary>플랫폼 직접 선택 <span aria-hidden="true">＋</span></summary>
              <div className="distribution-options">
                {DSP.map(([key, label]) => (
                  <label key={key} className="check-line">
                    <input
                      type="checkbox"
                      checked={form.platforms.includes(key)}
                      onChange={e => set('platforms', e.target.checked
                        ? [...form.platforms, key]
                        : form.platforms.filter(p => p !== key))}
                    />
                    <span>{label}</span>
                  </label>
                ))}
              </div>
            </details>
            <OptionsSection form={form} set={set} />
          </section>
        )}

        {step === 4 && (
          <section className="step-section">
            <div className="form-grid">
              <div className="field">
                <label htmlFor="f-ownership">음원(마스터) 권리자 <span className="required">*</span></label>
                <input id="f-ownership" value={form.ownership} onChange={e => set('ownership', e.target.value)} maxLength={160} placeholder="개인명 또는 법인명" />
              </div>
              <div className="field">
                <label htmlFor="f-phonogram">℗ 음반제작자 권리 표기 <span className="required">*</span></label>
                <input id="f-phonogram" value={form.phonogram} onChange={e => set('phonogram', e.target.value)} maxLength={180} placeholder="예: 2026 권리자명" />
              </div>
              <div className="field">
                <label htmlFor="f-copyright">© 아트워크 / 앨범 권리 표기 <span className="required">*</span></label>
                <input id="f-copyright" value={form.copyright} onChange={e => set('copyright', e.target.value)} maxLength={180} placeholder="예: 2026 권리자명" />
              </div>
            </div>
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
              권리 확인 체크는 실제 계약 체결이나 저작권 확인을 대신하지 않아요. 권리 관련 증빙은 '계약서·권리' 메뉴에서 발매별로 등록해 주세요.
            </div>
            <OptionDocsBanner options={form.options} />
          </section>
        )}

        {step === 5 && (
          <section className="step-section">
            {reviewSections.map(([h, t]) => (
              <div key={h} className="review-section">
                <h3>{h}</h3>
                <p className="break">{t}</p>
              </div>
            ))}
            <div className="notice" style={{ marginTop: 24 }}>
              '접수하기'를 누르면 신청 내용과 권리 확인서가 생성돼요. 접수 번호 발급과 담당자 심사는 서버가 연결된 후 진행돼요.
            </div>
            <FinalReviewBanner form={form} />
          </section>
        )}
      </div>

      {error && <div id="wizardError" className="notice error" role="alert">{error}</div>}

      <div className="step-actions">
        <button type="button" id="wizardBack" className="button secondary" onClick={back}>
          {step === 0 ? '홈으로' : '이전'}
        </button>
        <button type="button" id="wizardNext" className="button" onClick={next} disabled={submitting}>
          {step === STEPS.length - 1 ? '접수하기' : '다음으로'}
        </button>
      </div>
    </div>
    </section>
  );
}
