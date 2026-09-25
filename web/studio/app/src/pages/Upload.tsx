import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useToast } from '../components/Toast';
import { mockApi } from '../api/mock';
import { addDoc, type DocRecord } from '../store/docs';
import { localStamp } from '../lib/format';

const STEPS = [
  { kicker: '01 / 06 · 발매 정보', title: '어떤 음악을\n발매할까요?', sub: '발매 정보와 아티스트명을 입력해 주세요.' },
  { kicker: '02 / 06 · 트랙 등록', title: '발매할 곡을\n등록해 주세요.', sub: '곡별 음원 파일과 크레딧을 입력해 주세요.' },
  { kicker: '03 / 06 · 커버아트', title: '커버아트를\n등록해 주세요.', sub: '정사각형 커버아트를 등록해 주세요.' },
  { kicker: '04 / 06 · 배급 설정', title: '언제, 어디에\n발매할까요?', sub: '발매일과 배급할 플랫폼을 선택해 주세요.' },
  { kicker: '05 / 06 · 권리 확인', title: '음악의 권리를\n확인해 주세요.', sub: '권리자 정보와 필요한 증빙을 준비해 주세요.' },
  { kicker: '06 / 06 · 최종 확인', title: '발매 정보를\n마지막으로 확인해 주세요.', sub: '입력한 정보와 빠진 항목을 확인해 주세요.' },
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
  ['rightsSamples', '샘플링·커버곡·피처링 등에 제3자 권리가 있다면 필요한 허락을 확보했어요.'],
  ['rightsConsent', '입력한 정보가 정확하며 필요한 권리 증빙을 요청받으면 제출할 수 있어요.'],
];

interface Track {
  id: string;
  title: string; version: string; isrc: string;
  composers: string; lyricists: string; arrangers: string; performers: string;
  audioName: string; audioSize: number; explicit: boolean; duration: string;
}

interface ReleaseOptions {
  express: boolean; expressAck: boolean; expressReason: string;
  minor: boolean; guardian: string; guardianRelation: string; guardianContact: string; guardianFileName: string;
  cover: boolean; sample: boolean; featured: boolean;
  ai: boolean; aiTool: string;
  shared: boolean;
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
  audioName: '', audioSize: 0, explicit: false, duration: '',
});

const EMPTY_OPTIONS: ReleaseOptions = {
  express: false, expressAck: false, expressReason: '',
  minor: false, guardian: '', guardianRelation: '', guardianContact: '', guardianFileName: '',
  cover: false, sample: false, featured: false,
  ai: false, aiTool: '',
  shared: false,
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

const OPTIONS_CATALOG: [keyof ReleaseOptions, string, string][] = [
  ['express', '신속 발매 요청', '희망 일정과 우선 검토 가능 여부를 확인해요.'],
  ['minor', '미성년 아티스트·권리자', '법정대리인의 동의와 권한 확인이 필요해요.'],
  ['cover', '커버곡', '원곡의 작사·작곡 저작물 이용 권한을 확인해요.'],
  ['sample', '샘플링·타인 음원 사용', '원본 음원과 저작물의 이용 허락을 확인해요.'],
  ['featured', '피처링·공동 실연', '참여자 크레딧 및 필요한 이용 허락을 확인해요.'],
  ['ai', 'AI 생성·보조 제작', '제작 방식과 각 플랫폼의 수용 기준을 검토해요.'],
  ['shared', '공동 권리자·레이블 계약', '각 권리자와 배급 위임 범위를 확인해요.'],
  ['rerelease', '기존 발매 이전·재발매', '이전 발매의 식별자와 중복 송출 여부를 확인해요.'],
];

function rightsOk(f: WizardForm): boolean {
  return RIGHTS_CHECKS.every(([k]) => f.rightsChecks[k]);
}

function stampNow(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
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

function OptionsSection({ form, set }: {
  form: WizardForm;
  set: <K extends keyof WizardForm>(key: K, value: WizardForm[K]) => void;
}) {
  const o = form.options;
  const setOpt = <K extends keyof ReleaseOptions>(key: K, value: ReleaseOptions[K]) =>
    set('options', { ...o, [key]: value });

  return (
    <>
      <h2 className="subhead" style={{ marginTop: 25 }}>발매 추가 옵션</h2>
      <p className="aq-option-intro">해당하는 항목만 선택해 주세요. 필요한 확인 사항과 서류가 자동으로 안내돼요.</p>
      <div className="aq-options">
        {OPTIONS_CATALOG.map(([id, title, sub]) => (
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
          <p className="aq-option-intro">본인 및 권리자의 동의 범위를 확인할 수 있도록 보호자 정보를 입력해 주세요.</p>
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
          <div className="field">
            <label htmlFor="aqGuardianFile">법정대리인 동의서 (제출할 수 있다면)</label>
            <input
              type="file" id="aqGuardianFile"
              accept=".pdf,.png,.jpg,.jpeg,.webp,application/pdf,image/*"
              onChange={e => {
                const f = e.target.files?.[0];
                if (f) setOpt('guardianFileName', f.name);
              }}
            />
            <p className="help">
              {o.guardianFileName
                ? `선택한 서류 · ${o.guardianFileName}`
                : '아직 동의서를 첨부하지 않았다면, 발매 접수 후 권리 증빙 메뉴에서 추가할 수 있어요.'}
            </p>
          </div>
          <p className="aq-option-note">법정대리인 정보 입력만으로 본인 확인이나 동의 검증이 완료되지는 않아요. 담당자 확인 후 서명 단계를 안내해요.</p>
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
            <label htmlFor="aqPreviousId">기존 UPC / ISRC (보유 시)</label>
            <input
              id="aqPreviousId" maxLength={100} value={o.previousId}
              onChange={e => setOpt('previousId', e.target.value)}
              placeholder="이전 식별자"
            />
          </div>
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
  const coverInputRef = useRef<HTMLInputElement>(null);
  const progressRef = useRef<HTMLDivElement>(null);
  const prevStepRef = useRef(0);

  // 라이브와 동일: 위자드에서는 헤더 숨김 + 레이아웃 패딩 제거
  useEffect(() => {
    document.body.classList.add('wizard-mode');
    return () => document.body.classList.remove('wizard-mode');
  }, []);

  // 라이브와 동일: 다음 단계로 넘어갈 때 새 진행 구간이 왼쪽에서 차오르는 애니메이션
  useEffect(() => {
    const el = progressRef.current;
    if (step > prevStepRef.current && el) {
      const spans = el.querySelectorAll('span');
      const newSpan = spans[step] as HTMLElement | undefined;
      if (newSpan && !window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
        newSpan.animate(
          [{ transform: 'scaleX(0)' }, { transform: 'scaleX(1)' }],
          { duration: 280, easing: 'cubic-bezier(.22,1,.36,1)' }
        );
      }
    }
    prevStepRef.current = step;
  }, [step]);

  const set = <K extends keyof WizardForm>(key: K, value: WizardForm[K]) =>
    setForm(f => ({ ...f, [key]: value }));

  const setTrack = (id: string, key: keyof Track, value: string | boolean | number) =>
    setForm(f => ({ ...f, tracks: f.tracks.map(t => (t.id === id ? { ...t, [key]: value } : t)) }));

  const fail = (msg: string, sel: string | null): boolean => {
    setError(msg);
    if (sel) {
      requestAnimationFrame(() => {
        document.querySelector<HTMLElement>(sel)?.focus();
      });
    }
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
      if (!form.platforms.length) return fail('배급할 플랫폼을 하나 이상 선택해 주세요.', null);
      const o = form.options;
      if (o.express && !o.expressAck) return fail('신속 발매 안내를 확인해 주세요.', '#aqExpressAck');
      if (o.minor) {
        if (!o.guardian.trim()) return fail('법정대리인 성명을 입력해 주세요.', '#aqGuardian');
        if (!o.guardianRelation) return fail('법정대리인과의 관계를 선택해 주세요.', '#aqGuardianRelation');
        if (!o.guardianContact.trim()) return fail('법정대리인의 연락처를 입력해 주세요.', '#aqGuardianContact');
      }
      if (o.ai && !o.aiTool.trim()) return fail('AI 도구명과 활용 방식을 입력해 주세요.', '#aqAiTool');
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

  const saveDraft = async () => {
    try {
      await mockApi.createRelease({
        title: form.title.trim() || '제목 없음',
        release_date: form.releaseDate || '',
      });
      toast('임시 저장했어요.');
    } catch {
      toast('임시 저장에 실패했어요.');
    }
  };

  const next = async () => {
    if (!validate()) return;
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
    const reader = new FileReader();
    reader.onload = () => {
      setForm(f => ({ ...f, coverName: file.name, coverData: String(reader.result) }));
    };
    reader.readAsDataURL(file);
  };

  const onTrackAudio = (id: string, e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    setForm(f => ({
      ...f,
      tracks: f.tracks.map(t => (t.id === id ? { ...t, audioName: file.name, audioSize: file.size } : t)),
    }));
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

      <div className="row-actions">
        <button type="button" id="saveDraft" className="link-btn" onClick={saveDraft}>임시 저장</button>
      </div>

      <div className="wizard-progress" id="wizardProgress" ref={progressRef} aria-label="발매 신청 진행 단계">
        {STEPS.map((_, i) => (
          <span key={i} className={i <= step ? 'current' : ''} />
        ))}
      </div>

      <div className="wizard-header">
        <p className="wizard-kicker" id="wizardKicker">{s.kicker}</p>
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
                  onChange={e => set('genre', e.target.value)}
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
                <p className="help" id="genreHelp">발매할 음악의 장르를 선택해 주세요.</p>
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
              {form.tracks.map((t, i) => (
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
                  <div className="form-grid">
                    <div className="field">
                      <label htmlFor={`tr-${i}-title`}>곡 제목 <span className="required">*</span></label>
                      <input id={`tr-${i}-title`} value={t.title} onChange={e => setTrack(t.id, 'title', e.target.value)} placeholder="곡명을 입력해" maxLength={200} />
                    </div>
                    <div className="field">
                      <label htmlFor={`tr-${i}-version`}>버전 / 부제</label>
                      <input id={`tr-${i}-version`} value={t.version} onChange={e => setTrack(t.id, 'version', e.target.value)} placeholder="예: Acoustic Version" maxLength={200} />
                    </div>
                    <div className="field">
                      <label htmlFor={`tr-${i}-isrc`}>ISRC (보유 시)</label>
                      <input id={`tr-${i}-isrc`} value={t.isrc} onChange={e => setTrack(t.id, 'isrc', e.target.value)} placeholder="예: KR-ABC-26-00001" maxLength={200} />
                    </div>
                    <div className="track-duration-label" aria-live="polite">
                      {t.duration ? `곡 길이 · ${t.duration}` : '음원을 선택하면 곡 길이를 자동으로 확인해요.'}
                    </div>
                    <div className="field">
                      <label htmlFor={`tr-${i}-composers`}>작곡 <span className="required">*</span></label>
                      <input id={`tr-${i}-composers`} value={t.composers} onChange={e => setTrack(t.id, 'composers', e.target.value)} placeholder="참여자 이름을 쉼표로 구분" maxLength={200} />
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
                  <label className="check-line">
                    <input
                      type="checkbox" checked={t.explicit}
                      onChange={e => setTrack(t.id, 'explicit', e.target.checked)}
                    />
                    <span>청소년 이용불가 / Explicit 가사가 포함돼요.</span>
                  </label>
                </article>
              ))}
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
            <div id="coverInfo">
              {form.coverData ? (
                <div className="cover-preview">
                  <img src={form.coverData} alt="등록한 커버 미리보기" />
                  <div>
                    <strong>{form.coverName}</strong>
                    <p className="help">커버 원본을 보관하고 화면에는 최적화된 미리보기를 표시해요.</p>
                    <button
                      type="button" className="link-btn"
                      onClick={() => {
                        set('coverName', ''); set('coverData', '');
                        if (coverInputRef.current) coverInputRef.current.value = '';
                      }}
                    >커버 삭제</button>
                  </div>
                </div>
              ) : (
                <p className="muted small">아직 등록된 커버 이미지가 없어요.</p>
              )}
            </div>
          </section>
        )}

        {step === 3 && (
          <section className="step-section">
            <div className="form-grid">
              <div className="field">
                <label htmlFor="f-releaseDate">발매 예정일 <span className="required">*</span></label>
                <input id="f-releaseDate" type="date" value={form.releaseDate} onChange={e => set('releaseDate', e.target.value)} />
              </div>
              <div className="field">
                <label htmlFor="f-originalDate">최초 발매일 (재발매인 경우)</label>
                <input id="f-originalDate" type="date" value={form.originalDate} onChange={e => set('originalDate', e.target.value)} />
              </div>
              <div className="field">
                <label htmlFor="f-upc">UPC / EAN (있는 경우)</label>
                <input id="f-upc" value={form.upc} onChange={e => set('upc', e.target.value)} maxLength={20} placeholder="없으면 비워두세요." />
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
