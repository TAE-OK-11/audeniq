// AUDENIQ 수요조사 — 기존 정적 HTML·app.js를 React로 옮긴 화면. 마크업·클래스·문구는 원본과 같다.
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type FormEvent } from 'react';
import { STEPS, type Question } from './survey';
import {
  applyChoice, emptyState, noDistributorExperience, otherOpen, payload, selected, validateAll, validateStep,
  type SurveyState,
} from './logic';
import { animateSurveyComplete, animateSurveyStep, initializeMotion } from './motion';

declare global {
  interface Window {
    turnstile?: {
      render: (el: string | HTMLElement, opts: Record<string, unknown>) => string;
      reset: (id?: string) => void;
    };
  }
}

const EXPERIENCE_KEYS = new Set(['q5', 'q6', 'q12']);

function QuestionBlock({ q, st, skip, onChoice, onOther, extra }: {
  q: Question; st: SurveyState; skip: boolean;
  onChoice: (key: string, value: string, checked: boolean) => void;
  onOther: (key: string, value: string) => void;
  extra?: React.ReactNode;
}) {
  const hidden = skip && EXPERIENCE_KEYS.has(q.key);
  const picked = selected(st, q.key);
  const n = picked.length;
  const type = q.kind === 'multi' ? 'checkbox' : 'radio';
  const showOther = otherOpen(st, q);
  return (
    <div
      className="question" id={`question-${q.key}`} data-question={q.key} data-kind={q.kind}
      data-required={String(q.required)} data-max={q.max} hidden={hidden}
    >
      <div className="question-header">
        <span className="question-id">{q.qid}</span>
        {q.tag && <span className="required-tag">{q.tag}</span>}
      </div>
      <h3 className="question-title">{q.title}</h3>
      {q.key === 'beta' ? (
        q.hint && <p className="question-hint">{q.hint}</p>
      ) : q.kind !== 'text' && (
        <div className="question-instructions">
          {q.hint && <p className="question-hint">{q.hint}</p>}
          {q.count && (
            <span className={`selection-count${n ? ' has-selection' : ''}`} data-count-for={q.key} aria-live="polite">
              {n ? `${n}개 선택됨` : '0개 선택'}
            </span>
          )}
        </div>
      )}
      {q.options && (
        <div className={`choices${q.twoCols ? ' two-cols' : ''}`}>
          {q.options.map((label, i) => {
            const value = String(i);
            const checked = picked.includes(value);
            return (
              <label key={value} className={`choice${checked ? ' is-selected' : ''}`}>
                <input
                  type={type} name={q.key} value={value} checked={checked} disabled={hidden}
                  required={type === 'radio' && q.required && q.key !== 'beta' && i === 0}
                  onChange={e => onChoice(q.key, value, e.target.checked)}
                />
                <span className="choice-label">{label}</span>
                <span className="choice-tick" aria-hidden="true">✓</span>
              </label>
            );
          })}
        </div>
      )}
      {q.other && (
        <label className="other-field" id={`${q.key}-other-wrap`} hidden={!showOther}>
          기타 내용을 적어 주세요{' '}
          <input
            name={`${q.key}_other`} type="text" maxLength={160} autoComplete="off" placeholder="직접 입력"
            aria-label={`${q.key} 기타 입력`} disabled={!showOther}
            value={st.others[q.key] ?? ''} onChange={e => onOther(q.key, e.target.value)}
          />
        </label>
      )}
      {extra}
    </div>
  );
}

export function App() {
  const [st, setSt] = useState<SurveyState>(emptyState);
  const [current, setCurrent] = useState(0);
  const [error, setErrorState] = useState('');
  const [busy, setBusy] = useState(false);
  const [token, setToken] = useState('');
  const [security, setSecurity] = useState('보안 확인을 준비하고 있어요.');
  const [receipt, setReceipt] = useState<string | null>(null);
  const [copyLabel, setCopyLabel] = useState('접수번호 복사');
  const submissionId = useRef(crypto.randomUUID());
  const siteKey = useRef('');
  const widgetId = useRef<string | null>(null);
  const turnstileLoading = useRef(false);
  const errorRef = useRef<HTMLParagraphElement>(null);
  const stepsRef = useRef<(HTMLElement | null)[]>([]);
  const completeRef = useRef<HTMLDivElement>(null);
  const firstRender = useRef(true);
  const last = STEPS.length - 1;
  const skip = noDistributorExperience(st);
  const serviceReady = !!siteKey.current;

  const setError = useCallback((message: string) => {
    setErrorState(message);
    if (message) requestAnimationFrame(() => errorRef.current?.scrollIntoView({ block: 'nearest', behavior: 'smooth' }));
  }, []);

  // 공개 Site Key는 정적 HTML 메타태그에서 읽는다 (페이지 조회에 Worker 호출 없음)
  useEffect(() => {
    const publicKey = document.querySelector<HTMLMetaElement>('meta[name="turnstile-site-key"]')?.content || '';
    if (!publicKey || publicKey.includes('REPLACE_WITH')) {
      setSecurity('설문 설정을 준비하고 있어요. 잠시 후 다시 방문해 주세요.');
      return;
    }
    siteKey.current = publicKey; // 서버는 제출 때 D1·비밀 키·토큰을 따로 확인한다
    setSecurity('마지막 단계에서 보안 확인을 진행해 주세요.');
  }, []);

  useEffect(() => { initializeMotion(); }, []);

  const renderTurnstile = useCallback(async () => {
    if (!siteKey.current || widgetId.current !== null || turnstileLoading.current) return;
    turnstileLoading.current = true;
    try {
      await new Promise<void>((resolve, reject) => {
        const script = document.createElement('script');
        script.src = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit';
        script.async = true;
        script.onload = () => resolve();
        script.onerror = reject;
        document.head.append(script);
      });
      // 마지막 단계가 보인 뒤에 그린다 (숨겨진 영역에서는 실패할 수 있음)
      widgetId.current = window.turnstile!.render('#turnstile-widget', {
        sitekey: siteKey.current,
        action: 'audeniq_survey',
        callback(value: string) { setToken(value); setSecurity('보안 확인이 완료됐어요.'); },
        'expired-callback'() { setToken(''); setSecurity('보안 확인이 만료됐어요. 다시 확인해 주세요.'); },
        'error-callback'() { setToken(''); setSecurity('보안 확인에 문제가 있어요. 페이지를 새로고침해 주세요.'); },
      });
      setSecurity('아래 보안 확인을 완료해 주세요.');
    } catch {
      setSecurity('보안 확인을 불러오지 못했어요. 네트워크를 확인하고 새로고침해 주세요.');
    } finally {
      turnstileLoading.current = false;
    }
  }, []);

  // 단계 전환: 진행률·포커스·스크롤·애니메이션 (첫 화면은 제외)
  useLayoutEffect(() => {
    if (firstRender.current) { firstRender.current = false; return; }
    const step = stepsRef.current[current];
    animateSurveyStep(step);
    step?.querySelector('h2')?.focus({ preventScroll: true });
    document.querySelector('.form-shell')?.scrollIntoView({ block: 'start', behavior: 'smooth' });
    if (current === last) void renderTurnstile();
  }, [current, last, renderTurnstile]);

  useLayoutEffect(() => {
    if (receipt) {
      animateSurveyComplete(completeRef.current);
      completeRef.current?.scrollIntoView({ block: 'center', behavior: 'smooth' });
    }
  }, [receipt]);

  const onChoice = (key: string, value: string, checked: boolean) => {
    const r = applyChoice(st, key, value, checked);
    setSt(r.state);
    if (r.error !== null) setError(r.error);
  };
  const onOther = (key: string, value: string) => setSt(s => ({ ...s, others: { ...s.others, [key]: value } }));

  const show = (index: number) => { setError(''); setCurrent(index); };
  const next = () => {
    const e = validateStep(st, STEPS[current]);
    if (e) return setError(e);
    show(current + 1);
  };

  const resetTurnstile = () => {
    setToken('');
    if (widgetId.current !== null && window.turnstile) window.turnstile.reset(widgetId.current);
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    const e = validateAll(st);
    if (e) return setError(e);
    if (!serviceReady || !token) return setError('보안 확인을 완료해 주세요.');
    setBusy(true);
    setError('');
    try {
      const response = await fetch('/api/responses', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload(st, submissionId.current, token)), cache: 'no-store',
      });
      const data = await response.json() as { ok?: boolean; receipt?: string; error?: string };
      if (!response.ok || !data.ok) throw new Error(data.error || '설문 제출에 실패했어요.');
      setReceipt(data.receipt ?? '');
    } catch (err) {
      setError(err instanceof Error ? err.message : '제출에 실패했어요.');
      resetTurnstile();
    } finally {
      setBusy(false);
    }
  };

  const copyReceipt = async () => {
    try {
      await navigator.clipboard.writeText(receipt ?? '');
      setCopyLabel('복사했어요 ✓');
    } catch {
      setCopyLabel('접수번호를 길게 눌러 복사해 주세요.');
    }
  };

  const done = receipt !== null;
  const q4NoDistributor = selected(st, 'q4').includes('9');
  const commentLength = st.comment.length.toLocaleString('ko-KR');

  return (
    <>
      <a className="skip" href="#survey-main">설문으로 건너뛰기</a>
      <header className="topbar">
        <div className="topbar-inner">
          <a href="#top" className="brand" aria-label="설문 페이지 맨 위로 이동"><img src="/assets/AUDENIQ_Logo_Light.svg" alt="AUDENIQ" /></a>
          <span className="header-label">리서치 · 사전조사</span>
          <a className="back-home" href="https://audeniq.com/">공식 홈페이지 <span aria-hidden="true">↗</span></a>
        </div>
      </header>
      <main id="survey-main" className="layout">
        <div className="intro">
          <span className="eyebrow"><span className="pulse" aria-hidden="true" />AUDENIQ RESEARCH · 사전 수요조사</span>
          <h1>당신의 음악이<br /><span className="accent">더 멀리 가도록.</span></h1>
          <p className="intro-lead">국내 아티스트 음원 유통·배급 서비스 수요조사</p>
          <p className="intro-callout">아티스트 여러분의 경험과 의견이<br /><strong>AUDENIQ의 서비스 방향을 만듭니다.</strong></p>
          <div className="intro-summary">
            <p>안녕하세요. 현재 학생 창업가로서 국내 아티스트를 위한 음원 배급 및 아티스트 인프라 플랫폼 <strong>AUDENIQ</strong>를 준비하고 있습니다.</p>
            <p>합리적인 수수료, 빠른 처리, 투명한 정산과 음원 배급·통계·가사·아티스트 지원 기능을 하나로 제공하는 것을 목표로 합니다. 정식 출시 전, 실제 아티스트분들의 의견을 듣고 싶습니다.</p>
          </div>
          <div className="intro-purpose">
            <span className="purpose-mark" aria-hidden="true">↗</span>
            <p><strong>응답 활용 안내</strong><br />응답은 서비스 기획·시장 수요 분석 및 향후 DSP·관련 업체 협의를 위한 통계 자료로 활용될 수 있습니다.</p>
          </div>
          <div className="intro-foot"><span>◷ &nbsp;약 5~10분</span><span>✓ &nbsp;익명 참여 가능</span><span>↗ &nbsp;정식 가입과 별개</span></div>
        </div>
        <section className="form-shell" aria-label="AUDENIQ 사전조사">
          <div className="progress-head" hidden={done}>
            <span className="mini-label">YOUR VOICE MATTERS</span>
            <span className="progress-number"><strong id="progress-now">{String(current + 1).padStart(2, '0')}</strong>{` / ${String(STEPS.length).padStart(2, '0')}`}</span>
          </div>
          <div
            className="progress-track" role="progressbar" aria-label="설문 진행률" aria-valuemin={1}
            aria-valuemax={STEPS.length} aria-valuenow={current + 1} id="progress" hidden={done}
          >
            <div className="progress-fill" id="progress-fill" style={{ width: `${((current + 1) / STEPS.length) * 100}%` }} />
          </div>
          <form id="survey-form" noValidate onSubmit={submit} hidden={done}>
            {STEPS.map((step, i) => (
              <section
                key={step.kicker} className="step" data-step={i} hidden={i !== current} aria-label={step.title}
                ref={el => { stepsRef.current[i] = el; }}
              >
                <div className="step-kicker">{step.kicker}</div>
                <h2 tabIndex={-1}>{step.title}</h2>
                <p className="step-desc">{step.desc}</p>
                {step.questions.map(q => {
                  if (q.key === 'q14') {
                    return (
                      <div key={q.key} className="question" id="question-q14" data-question="q14" data-kind="text" data-required="false">
                        <div className="question-header"><span className="question-id">{q.qid}</span><span className="required-tag">{q.tag}</span></div>
                        <h3 className="question-title">{q.title}</h3>
                        <textarea
                          className="text-field" id="q14" name="q14" rows={5} maxLength={2000} placeholder="이런 부분이 바뀌면 좋겠어요."
                          value={st.comment} onChange={e => setSt(s => ({ ...s, comment: e.target.value }))}
                        />
                        <div className="char-count" id="char-count">{`${commentLength} / 2,000`}</div>
                      </div>
                    );
                  }
                  let extra: React.ReactNode = null;
                  if (q.key === 'q4') {
                    extra = (
                      <>
                        <label className="input-label" htmlFor="q4_provider">이용한 배급사 이름이 있다면 적어주세요 <span>선택</span></label>
                        <input
                          className="text-field" id="q4_provider" name="q4_provider" type="text" maxLength={160} autoComplete="off"
                          placeholder="예: 이용했던 배급사 이름" disabled={q4NoDistributor}
                          value={st.provider} onChange={e => setSt(s => ({ ...s, provider: e.target.value }))}
                        />
                      </>
                    );
                  }
                  if (q.key === 'beta') {
                    extra = (
                      <>
                        <label className="input-label" htmlFor="contact">이메일 또는 Instagram 계정 <span>선택 · 베타 및 출시 안내용</span></label>
                        <input
                          className="text-field" id="contact" name="contact" type="text" maxLength={160} autoComplete="off"
                          placeholder="이메일 또는 @인스타그램계정" value={st.contact}
                          onChange={e => setSt(s => ({ ...s, contact: e.target.value }))}
                        />
                        <label className="agree beta-agree">
                          <input
                            id="beta_contact_consent" type="checkbox" name="beta_contact_consent" checked={st.betaConsent}
                            onChange={e => setSt(s => ({ ...s, betaConsent: e.target.checked }))}
                          />
                          <span>연락처를 입력한 경우, 베타테스트 및 서비스 출시 관련 안내를 위한 연락처 수집·이용에 동의해요. <em>(선택)</em></span>
                        </label>
                        <div className="privacy">
                          <div className="privacy-title">개인정보 및 응답 수집 안내</div>
                          <p>수집 주체: AUDENIQ · 설문 응답 목적: 서비스 기획, 시장 수요 분석 및 관련 업체 협의를 위한 통계. 선택적으로 입력한 연락처는 별도 동의한 경우에만 베타테스트·서비스 출시 안내에 이용합니다. 수집 항목: 설문 답변(자유입력 포함), 선택적으로 입력한 연락처. 보유 기간: 제출일로부터 최대 12개월이며 목적 달성 시 조기 삭제할 수 있습니다. 열람·삭제 문의: <a href="mailto:audeniq.official@gmail.com">audeniq.official@gmail.com</a>. 제출 후 표시되는 접수번호를 함께 알려주시면 응답을 찾는 데 도움이 됩니다. 제3자에게 통계 결과를 제공할 때는 개별 연락처를 포함하지 않습니다.</p>
                        </div>
                        <label className="agree required-agree">
                          <input
                            id="survey_consent" type="checkbox" name="survey_consent" required checked={st.surveyConsent}
                            onChange={e => setSt(s => ({ ...s, surveyConsent: e.target.checked }))}
                          />
                          <span>위 안내에 따른 설문 응답 수집·이용에 동의해요. <strong>(필수)</strong></span>
                        </label>
                        <div className="turnstile-area">
                          <div id="turnstile-widget" aria-label="자동 제출 방지 보안 확인" />
                          <p className="security-status" id="security-status" role="status">{security}</p>
                        </div>
                        <p className="final-foot">참여는 정식 회원가입이나 음원 배급 신청이 아니에요.</p>
                      </>
                    );
                  }
                  return <QuestionBlock key={q.key} q={q} st={st} skip={skip} onChoice={onChoice} onOther={onOther} extra={extra} />;
                })}
              </section>
            ))}
            <p className="form-error" id="form-error" role="alert" hidden={!error} ref={errorRef}>{error}</p>
            <div className="form-controls">
              <button type="button" className="previous" id="prev-btn" hidden={current === 0} onClick={() => show(current - 1)}>← 이전</button>
              <button type="button" className="next" id="next-btn" hidden={current === last} onClick={next}>다음으로 <span aria-hidden="true">→</span></button>
              <button
                type="submit" className="next" id="submit-btn" hidden={current !== last}
                disabled={busy || !serviceReady || !token || !st.surveyConsent}
              >
                {busy ? '제출하는 중…' : <>설문 제출하기 <span aria-hidden="true">↗</span></>}
              </button>
            </div>
          </form>
          <div id="complete" className="complete" hidden={!done} ref={completeRef}>
            <div className="complete-icon" aria-hidden="true">✓</div>
            <span className="step-kicker">THANK YOU</span>
            <h2>당신의 이야기가<br />AUDENIQ의 다음을 만들어요.</h2>
            <p>의견을 보내줘서 고마워요. 더 나은 음악 배급 서비스를 준비하는 데 소중하게 활용할게요.</p>
            <div className="receipt">접수번호 <code id="receipt-id">{receipt}</code></div>
            <button type="button" id="copy-receipt" className="copy-btn" onClick={() => void copyReceipt()}>{copyLabel}</button>
            <a className="complete-home" href="https://audeniq.com/">AUDENIQ 홈페이지로 돌아가기 ↗</a>
          </div>
        </section>
        <p className="below-note">음원을 발매한 경험이 없어도 참여할 수 있어요. 설문 응답은 익명으로 제출할 수 있어요.</p>
      </main>
      <footer>
        <div>© AUDENIQ · 당신의 음악, 더 넓은 세상으로</div>
        <div><a href="https://audeniq.com/">공식 홈페이지</a><a href="mailto:audeniq.official@gmail.com">문의하기</a></div>
      </footer>
    </>
  );
}
