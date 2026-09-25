import { useState } from 'react';
import { useNavigate } from 'react-router-dom';

// 라이브 HTML의 wizard 단계 정의
const STEPS = [
  { kicker: '01 / 06 · 발매 정보', title: '어떤 음악을\n발매할까요?', sub: '발매 정보와 아티스트명을 입력해 주세요.' },
  { kicker: '02 / 06 · 트랙 등록', title: '발매할 곡을\n등록해 주세요.', sub: '곡별 음원 파일과 크레딧을 입력해 주세요.' },
  { kicker: '03 / 06 · 커버아트', title: '커버아트를\n등록해 주세요.', sub: '정사각형 커버아트를 등록해 주세요.' },
  { kicker: '04 / 06 · 배급 설정', title: '언제, 어디에\n발매할까요?', sub: '발매일과 배급할 플랫폼을 선택해 주세요.' },
  { kicker: '05 / 06 · 권리 확인', title: '음악의 권리를\n확인해 주세요.', sub: '권리자 정보와 필요한 증빙을 준비해 주세요.' },
  { kicker: '06 / 06 · 최종 확인', title: '발매 정보를\n마지막으로 확인해 주세요.', sub: '입력한 정보와 빠진 항목을 확인해 주세요.' },
];

const PLATFORMS = ['Spotify', 'Apple Music', 'Melon', 'YouTube Music', 'FLO', 'Genie'];

interface WizardForm {
  title: string;
  artist: string;
  genre: string;
  tracks: string[];
  coverName: string;
  releaseDate: string;
  platforms: string[];
  rightsAgreed: boolean;
}

const EMPTY: WizardForm = {
  title: '', artist: '', genre: '',
  tracks: [''], coverName: '',
  releaseDate: '', platforms: [], rightsAgreed: false,
};

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
  const [step, setStep] = useState(0);
  const [form, setForm] = useState<WizardForm>(EMPTY);
  const [error, setError] = useState('');
  const [done, setDone] = useState(false);

  const set = <K extends keyof WizardForm>(key: K, value: WizardForm[K]) =>
    setForm(f => ({ ...f, [key]: value }));

  const validate = (): boolean => {
    if (step === 0 && !form.title.trim()) { setError('발매 제목을 입력해 주세요.'); return false; }
    if (step === 1 && !form.tracks.some(t => t.trim())) { setError('한 곡 이상 등록해 주세요.'); return false; }
    if (step === 4 && !form.rightsAgreed) { setError('권리 확인 항목에 동의해 주세요.'); return false; }
    setError('');
    return true;
  };

  const next = () => {
    if (!validate()) return;
    if (step === STEPS.length - 1) {
      // mock 모드: 제출 시 성공 메시지만 표시
      setDone(true);
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
          <p>테스트 모드이므로 실제 제출 없이 성공 메시지만 표시합니다.</p>
          <div style={{ marginTop: 20, display: 'flex', gap: 10, justifyContent: 'center' }}>
            <button type="button" className="button" onClick={() => nav('/releases')}>발매 목록 보기</button>
            <button type="button" className="button secondary" onClick={() => nav('/')}>홈으로</button>
          </div>
        </div>
      </div>
    );
  }

  const s = STEPS[step];

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
          <div>
            <div className="field">
              <label htmlFor="wTitle">발매 제목 <span className="required">*</span></label>
              <input id="wTitle" value={form.title} onChange={e => set('title', e.target.value)} placeholder="예: 첫 번째 싱글" />
            </div>
            <div className="field">
              <label htmlFor="wArtist">아티스트명</label>
              <input id="wArtist" value={form.artist} onChange={e => set('artist', e.target.value)} placeholder="아티스트명" />
            </div>
            <div className="field">
              <label htmlFor="wGenre">장르</label>
              <select id="wGenre" value={form.genre} onChange={e => set('genre', e.target.value)}>
                <option value="">선택해 주세요</option>
                <option>Pop</option><option>Hip-hop</option><option>R&B</option>
                <option>Rock</option><option>Electronic</option><option>Indie</option><option>기타</option>
              </select>
            </div>
          </div>
        )}

        {step === 1 && (
          <div>
            {form.tracks.map((t, i) => (
              <div className="field" key={i}>
                <label htmlFor={`wTrack${i}`}>트랙 {i + 1}</label>
                <input
                  id={`wTrack${i}`} value={t}
                  onChange={e => {
                    const nextTracks = [...form.tracks];
                    nextTracks[i] = e.target.value;
                    set('tracks', nextTracks);
                  }}
                  placeholder="곡 제목"
                />
              </div>
            ))}
            <button
              type="button" className="button secondary"
              onClick={() => set('tracks', [...form.tracks, ''])}
            >트랙 추가</button>
          </div>
        )}

        {step === 2 && (
          <div>
            <div className="field">
              <label htmlFor="wCover">커버아트 파일</label>
              <input
                id="wCover" type="file" accept="image/*"
                onChange={e => set('coverName', e.target.files?.[0]?.name ?? '')}
              />
              {form.coverName && <p className="small muted" style={{ marginTop: 8 }}>{form.coverName}</p>}
            </div>
            <div className="notice">정사각형 이미지(최소 3000×3000px 권장)를 등록해 주세요.</div>
          </div>
        )}

        {step === 3 && (
          <div>
            <div className="field">
              <label htmlFor="wDate">발매 희망일</label>
              <input id="wDate" type="date" value={form.releaseDate} onChange={e => set('releaseDate', e.target.value)} />
            </div>
            <div className="field">
              <label>배급 플랫폼</label>
              {PLATFORMS.map(p => (
                <label key={p} className="check-line" style={{ display: 'flex', gap: 10, alignItems: 'center', marginBottom: 12, fontSize: 15 }}>
                  <input
                    type="checkbox" checked={form.platforms.includes(p)}
                    onChange={e => set('platforms', e.target.checked
                      ? [...form.platforms, p]
                      : form.platforms.filter(x => x !== p))}
                  />
                  {p}
                </label>
              ))}
            </div>
          </div>
        )}

        {step === 4 && (
          <div>
            <div className="notice">
              제출하는 음원·커버·크레딧에 대한 권리를 보유하고 있거나, 적법한 허락을 받았는지 확인해 주세요.
            </div>
            <label className="check-line" style={{ display: 'flex', gap: 10, alignItems: 'flex-start', marginTop: 20, fontSize: 15 }}>
              <input
                type="checkbox" checked={form.rightsAgreed}
                onChange={e => set('rightsAgreed', e.target.checked)}
                style={{ marginTop: 4 }}
              />
              <span>음원·커버·크레딧의 권리를 확인했고, AUDENIQ를 통한 배급에 필요한 권한이 있음을 확인합니다.</span>
            </label>
          </div>
        )}

        {step === 5 && (
          <section className="surface">
            <div className="section-top"><h2>입력 내용 확인</h2></div>
            <div className="data-list">
              {[
                ['발매 제목', form.title || '(미입력)'],
                ['아티스트', form.artist || '(미입력)'],
                ['장르', form.genre || '(미선택)'],
                ['트랙', form.tracks.filter(t => t.trim()).join(', ') || '(미입력)'],
                ['커버아트', form.coverName || '(미등록)'],
                ['발매 희망일', form.releaseDate || '(미정)'],
                ['플랫폼', form.platforms.join(', ') || '(미선택)'],
              ].map(([k, v]) => (
                <div key={k} className="track-row">
                  <div><span className="row-sub">{k}</span></div>
                  <div><span className="row-name">{v}</span></div>
                  <div />
                </div>
              ))}
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
          {step === STEPS.length - 1 ? '제출하기' : '다음으로'}
        </button>
      </div>
    </div>
  );
}
