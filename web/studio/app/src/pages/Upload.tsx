import { useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useToast } from '../components/Toast';

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

interface WizardForm {
  artist: string; title: string; type: string; language: string; genre: string;
  genreCustom: string; label: string; notes: string;
  tracks: Track[];
  coverName: string; coverData: string;
  releaseDate: string; originalDate: string; upc: string;
  territories: string[]; platforms: string[];
  ownership: string; phonogram: string; copyright: string;
  rightsChecks: Record<string, boolean>;
}

const newTrack = (): Track => ({
  id: 't' + Math.random().toString(36).slice(2, 9),
  title: '', version: '', isrc: '', composers: '', lyricists: '', arrangers: '', performers: '',
  audioName: '', audioSize: 0, explicit: false, duration: '',
});

const EMPTY: WizardForm = {
  artist: '', title: '', type: 'single', language: 'ko', genre: '', genreCustom: '',
  label: '', notes: '',
  tracks: [newTrack()],
  coverName: '', coverData: '',
  releaseDate: '', originalDate: '', upc: '',
  territories: ['WORLD'], platforms: DSP.map(d => d[0]),
  ownership: '', phonogram: '', copyright: '',
  rightsChecks: {},
};

function rightsOk(f: WizardForm): boolean {
  return RIGHTS_CHECKS.every(([k]) => f.rightsChecks[k]);
}

function BackIcon() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" width="24" height="24" fill="none"
      stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="m15 18-6-6 6-6" />
    </svg>
  );
}

export function Upload() {
  const nav = useNavigate();
  const toast = useToast();
  const [step, setStep] = useState(0);
  const [form, setForm] = useState<WizardForm>(EMPTY);
  const [error, setError] = useState('');
  const [done, setDone] = useState(false);
  const coverInputRef = useRef<HTMLInputElement>(null);

  const set = <K extends keyof WizardForm>(key: K, value: WizardForm[K]) =>
    setForm(f => ({ ...f, [key]: value }));

  const setTrack = (id: string, key: keyof Track, value: string | boolean | number) =>
    setForm(f => ({ ...f, tracks: f.tracks.map(t => (t.id === id ? { ...t, [key]: value } : t)) }));

  const validate = (): boolean => {
    if (step === 0) {
      if (!form.artist.trim()) { setError('아티스트명을 입력해 주세요.'); return false; }
      if (!form.title.trim()) { setError('발매 제목을 입력해 주세요.'); return false; }
      const genreVal = form.genre === '__other__' ? form.genreCustom : form.genre;
      if (!genreVal.trim()) { setError('장르를 선택해 주세요.'); return false; }
    }
    if (step === 1) {
      for (const t of form.tracks) {
        if (!t.title.trim()) { setError('곡 제목을 입력해 주세요.'); return false; }
        if (!t.composers.trim()) { setError('작곡 참여자를 입력해 주세요.'); return false; }
      }
    }
    if (step === 2 && !form.coverName) { setError('커버아트를 등록해 주세요.'); return false; }
    if (step === 3 && !form.releaseDate) { setError('발매 예정일을 입력해 주세요.'); return false; }
    if (step === 4) {
      if (!form.ownership.trim()) { setError('음원(마스터) 권리자를 입력해 주세요.'); return false; }
      if (!form.phonogram.trim()) { setError('℗ 음반제작자 권리 표기를 입력해 주세요.'); return false; }
      if (!form.copyright.trim()) { setError('© 아트워크 / 앨범 권리 표기를 입력해 주세요.'); return false; }
      if (!rightsOk(form)) { setError('필수 확인 항목을 모두 체크해 주세요.'); return false; }
    }
    setError('');
    return true;
  };

  const next = () => {
    if (!validate()) return;
    if (step === STEPS.length - 1) {
      setDone(true);
      toast('발매 신청이 접수됐어요.');
      return;
    }
    setStep(s => s + 1);
    window.scrollTo({ top: 0 });
  };

  const back = () => {
    if (step === 0) { nav('/'); return; }
    setStep(s => s - 1);
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

  if (done) {
    return (
      <div className="wizard">
        <div className="wizard-topbar" aria-label="발매 신청 탐색">
          <button type="button" className="wizard-topback" aria-label="이전으로 돌아가기" onClick={() => nav('/')}>
            <BackIcon />
          </button>
          <span className="wizard-top-title">새로운 발매</span>
          <span className="wizard-top-count">완료</span>
        </div>
        <div className="empty-page" style={{ marginTop: 40 }}>
          <h3>발매 신청이 접수됐어요.</h3>
          <p>신청 내용과 권리 확인서가 생성됐어요. (테스트 모드)</p>
          <div style={{ marginTop: 20, display: 'flex', gap: 10, justifyContent: 'center' }}>
            <button type="button" className="button" onClick={() => nav('/contracts')}>배급신청서 보기</button>
            <button type="button" className="button secondary" onClick={() => nav('/')}>홈으로</button>
          </div>
        </div>
      </div>
    );
  }

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
    <div className="wizard">
      <div className="wizard-topbar" aria-label="발매 신청 탐색">
        <button type="button" className="wizard-topback" aria-label="이전으로 돌아가기" onClick={back}>
          <BackIcon />
        </button>
        <span className="wizard-top-title">새로운 발매</span>
        <span className="wizard-top-count">{step + 1} / 6</span>
      </div>

      <div className="wizard-progress" aria-label="발매 신청 진행 단계">
        {STEPS.map((_, i) => (
          <span key={i} className={i <= step ? 'current' : ''} />
        ))}
      </div>

      <div className="wizard-header">
        <p className="wizard-kicker">{s.kicker}</p>
        <h1 style={{ whiteSpace: 'pre-line' }}>{s.title}</h1>
        <p>{s.sub}</p>
      </div>

      <div aria-live="polite">
        {step === 0 && (
          <section className="step-section">
            <div className="form-grid">
              <div className="field">
                <label htmlFor="f-artist">아티스트명 <span className="required">*</span></label>
                <input id="f-artist" value={form.artist} onChange={e => set('artist', e.target.value)} maxLength={120} placeholder="활동명을 입력해 주세요." />
              </div>
              <div className="field">
                <label htmlFor="f-title">발매 제목 <span className="required">*</span></label>
                <input id="f-title" value={form.title} onChange={e => set('title', e.target.value)} maxLength={180} placeholder="싱글 또는 앨범 제목을 입력해 주세요." />
              </div>
              <div className="field">
                <label htmlFor="f-type">발매 유형</label>
                <select id="f-type" value={form.type} onChange={e => set('type', e.target.value)}>
                  {KINDS.map(([id, title]) => <option key={id} value={id}>{title}</option>)}
                </select>
              </div>
              <div className="field">
                <label htmlFor="f-genre">장르 <span className="required">*</span></label>
                <select
                  id="f-genre" value={genreIsCustom ? '__other__' : form.genre}
                  onChange={e => set('genre', e.target.value)}
                  aria-describedby="genreHelp"
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
            </div>
            <details className="studio-expand">
              <summary>발매 정보 더 입력하기 <span aria-hidden="true">＋</span></summary>
              <div className="form-grid">
                <div className="field">
                  <label htmlFor="f-language">주요 언어</label>
                  <select id="f-language" value={form.language} onChange={e => set('language', e.target.value)}>
                    {LANGUAGES.map(([id, title]) => <option key={id} value={id}>{title}</option>)}
                  </select>
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
                  rows={3} maxLength={1500} placeholder="발매 소개를 작성해 주세요."
                />
              </div>
            </details>
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
                  <div className="field">
                    <label htmlFor={`tr-${i}-title`}>곡 제목 <span className="required">*</span></label>
                    <input id={`tr-${i}-title`} value={t.title} onChange={e => setTrack(t.id, 'title', e.target.value)} placeholder="곡 제목을 입력해 주세요." maxLength={200} />
                  </div>
                  <div className="field">
                    <label htmlFor={`trackFile-${i}`}>음원 파일 <span className="required">*</span></label>
                    <input
                      type="file" id={`trackFile-${i}`}
                      accept="audio/*,.wav,.flac,.aiff,.mp3"
                      onChange={e => onTrackAudio(t.id, e)}
                    />
                    <p className="help" id={`audioLabel-${i}`}>
                      {t.audioName ? `${t.audioName}${t.audioSize ? ` · ${Math.round(t.audioSize / 1024 / 1024 * 100) / 100}MB` : ''}` : '선택한 파일 없음'}
                    </p>
                    <div className="track-duration-label">
                      {t.duration ? `곡 길이 · ${t.duration}` : '음원을 선택하면 곡 길이를 자동으로 확인해요.'}
                    </div>
                  </div>
                  <div className="field">
                    <label htmlFor={`tr-${i}-composers`}>작곡자 <span className="required">*</span></label>
                    <input id={`tr-${i}-composers`} value={t.composers} onChange={e => setTrack(t.id, 'composers', e.target.value)} placeholder="작곡자를 입력해 주세요." maxLength={200} />
                  </div>
                  <details className="studio-expand">
                    <summary>참여자·곡 정보 더 입력하기 <span aria-hidden="true">＋</span></summary>
                    <div className="form-grid">
                      <div className="field">
                        <label htmlFor={`tr-${i}-version`}>버전 / 부제</label>
                        <input id={`tr-${i}-version`} value={t.version} onChange={e => setTrack(t.id, 'version', e.target.value)} placeholder="버전 / 부제을 입력해 주세요." maxLength={200} />
                      </div>
                      <div className="field">
                        <label htmlFor={`tr-${i}-isrc`}>ISRC (보유 시)</label>
                        <input id={`tr-${i}-isrc`} value={t.isrc} onChange={e => setTrack(t.id, 'isrc', e.target.value)} placeholder="ISRC (보유 시)을 입력해 주세요." maxLength={200} />
                      </div>
                      <div className="field">
                        <label htmlFor={`tr-${i}-lyricists`}>작사가</label>
                        <input id={`tr-${i}-lyricists`} value={t.lyricists} onChange={e => setTrack(t.id, 'lyricists', e.target.value)} placeholder="작사가을 입력해 주세요." maxLength={200} />
                      </div>
                      <div className="field">
                        <label htmlFor={`tr-${i}-arrangers`}>편곡자</label>
                        <input id={`tr-${i}-arrangers`} value={t.arrangers} onChange={e => setTrack(t.id, 'arrangers', e.target.value)} placeholder="편곡자을 입력해 주세요." maxLength={200} />
                      </div>
                      <div className="field">
                        <label htmlFor={`tr-${i}-performers`}>실연자 / 피처링</label>
                        <input id={`tr-${i}-performers`} value={t.performers} onChange={e => setTrack(t.id, 'performers', e.target.value)} placeholder="실연자 / 피처링을 입력해 주세요." maxLength={200} />
                      </div>
                    </div>
                    <label className="check-line">
                      <input
                        type="checkbox" checked={t.explicit}
                        onChange={e => setTrack(t.id, 'explicit', e.target.checked)}
                      />
                      <span>청소년 이용불가 / Explicit 가사가 포함돼요.</span>
                    </label>
                  </details>
                  <div className="aq-explicit-note">
                    <i aria-hidden="true">!</i>
                    <span>
                      <strong>Explicit 표기와 국가별 청소년 제한</strong>
                      선정적인 가사나 표현을 Explicit로 선택하면 대한민국 등 일부 국가에서 청소년 이용 제한 또는 노출 제한이 적용될 수 있어요. 발매 후에는 원본을 유지하면서 Clean 버전을 함께 제출할 수 있어요.
                    </span>
                  </div>
                </article>
              ))}
            </div>
            <div className="spaced-actions">
              <button type="button" className="button secondary" onClick={() => set('tracks', [...form.tracks, newTrack()])}>
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
          </section>
        )}
      </div>

      {error && <div className="notice error" role="alert" style={{ marginTop: 16 }}>{error}</div>}

      <div className="step-actions">
        <button type="button" className="button secondary" onClick={back}>
          {step === 0 ? '홈으로' : '이전으로'}
        </button>
        <button type="button" className="button" onClick={next}>
          {step === STEPS.length - 1 ? '접수하기' : '다음으로'}
        </button>
      </div>
    </div>
  );
}
