// 본인인증 창 — 인증 방법 고르기 → 정보 입력 → (문자: 인증번호 / PASS·카드사: 앱 승인) → 완료.
// 체험 단계라 실제 인증 기관에 요청하지 않고 흐름만 보여 준다. 완료하면 입력한 이름을 실명으로 저장한다.
import { useEffect, useRef, useState } from 'react';
import { useModalClose } from './Modal';
import { Glyph } from './Glyph';
import { patchAccount, VERIFY_LABEL, type VerifyMethod } from '../store/account';

const METHODS: { id: VerifyMethod; title: string; sub: string; icon: string }[] = [
  { id: 'pass', title: 'PASS 인증', sub: '통신사 PASS 앱으로 인증', icon: 'lock' },
  { id: 'sms', title: '휴대폰 문자 인증', sub: '문자로 받은 인증번호 입력', icon: 'mail' },
  { id: 'ipin', title: '아이핀 인증', sub: '아이핀 아이디로 인증', icon: 'user' },
  { id: 'card', title: '카드사 인증', sub: '본인 명의 카드사 앱으로 인증', icon: 'card' },
];
const CARRIERS = ['SKT', 'KT', 'LG U+', 'SKT 알뜰폰', 'KT 알뜰폰', 'LG U+ 알뜰폰'];
const CARD_COMPANIES = ['신한카드', 'KB국민카드', '삼성카드', '현대카드', '롯데카드', '하나카드', '우리카드', 'BC카드', 'NH농협카드'];

type Step = 'method' | 'form' | 'code' | 'approve';

const digits = (v: string, n: number) => v.replace(/\D/g, '').slice(0, n);

export function IdentityVerify({ onDone }: { onDone: (name: string, method: VerifyMethod) => void }) {
  const close = useModalClose();
  const [step, setStep] = useState<Step>('method');
  const [method, setMethod] = useState<VerifyMethod>('pass');
  const [f, setF] = useState({ name: '', birth: '', sex: '', carrier: CARRIERS[0], phone: '', card: CARD_COMPANIES[0], ipinId: '', ipinPw: '', code: '' });
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [left, setLeft] = useState(180);
  const timer = useRef(0);
  const set = (k: keyof typeof f, v: string) => { setF(p => ({ ...p, [k]: v })); setError(''); };

  // 인증번호 유효 시간(3분) 표시
  useEffect(() => {
    if (step !== 'code') return;
    setLeft(180);
    const t = window.setInterval(() => setLeft(s => Math.max(0, s - 1)), 1000);
    return () => window.clearInterval(t);
  }, [step]);
  useEffect(() => () => window.clearTimeout(timer.current), []);

  const finish = () => {
    setBusy(true);
    timer.current = window.setTimeout(() => {
      patchAccount({ realName: f.name.trim(), verified: { method, at: new Date().toISOString() } });
      onDone(f.name.trim(), method);
      close();
    }, 700);
  };

  const submitForm = (e: React.FormEvent) => {
    e.preventDefault();
    if (!f.name.trim()) return setError('이름을 입력해 주세요.');
    if (method === 'ipin') {
      if (!f.ipinId.trim() || !f.ipinPw) return setError('아이핀 아이디와 비밀번호를 입력해 주세요.');
      return finish();
    }
    if (f.birth.length !== 6 || f.sex.length !== 1) return setError('생년월일 6자리와 주민등록번호 뒷자리 첫 숫자를 입력해 주세요.');
    if ((method === 'pass' || method === 'sms') && !/^01\d{8,9}$/.test(f.phone)) return setError('휴대폰 번호를 확인해 주세요.');
    setStep(method === 'sms' ? 'code' : 'approve');
  };

  const submitCode = (e: React.FormEvent) => {
    e.preventDefault();
    if (left === 0) return setError('인증 시간이 지났어요. 인증번호를 다시 받아 주세요.');
    if (f.code.length !== 6) return setError('인증번호 6자리를 입력해 주세요.');
    finish();
  };

  const notice = <p className="aq-verify-demo">체험용 화면이에요. 실제 본인인증은 진행되지 않아요.</p>;

  if (step === 'method') {
    return (
      <div className="aq-verify">
        {notice}
        <p className="aq-verify-lead">본인 명의로 인증할 방법을 골라 주세요.</p>
        <div className="aq-verify-methods" role="radiogroup" aria-label="인증 방법">
          {METHODS.map(m => (
            <label key={m.id} className={`aq-verify-method${method === m.id ? ' is-on' : ''}`}>
              <input type="radio" name="verifyMethod" checked={method === m.id} onChange={() => setMethod(m.id)} />
              <span className="aq-verify-ic" aria-hidden="true"><Glyph name={m.icon} size={20} /></span>
              <strong>{m.title}</strong>
              <small>{m.sub}</small>
            </label>
          ))}
        </div>
        <button type="button" className="ui-btn is-primary is-block" onClick={() => setStep('form')}>다음</button>
      </div>
    );
  }

  if (step === 'form') {
    return (
      <form className="aq-verify" onSubmit={submitForm} noValidate>
        {notice}
        <button type="button" className="aq-verify-back" onClick={() => { setStep('method'); setError(''); }}>
          <Glyph name="chevron-left" size={16} /> {VERIFY_LABEL[method]} 인증
        </button>
        <div className="ui-field">
          <label htmlFor="vfName">이름</label>
          <input id="vfName" autoFocus autoComplete="name" maxLength={40} placeholder="실명" value={f.name} onChange={e => set('name', e.target.value)} />
        </div>
        {method === 'ipin' ? (
          <>
            <div className="ui-field">
              <label htmlFor="vfIpinId">아이핀 아이디</label>
              <input id="vfIpinId" autoComplete="username" maxLength={40} value={f.ipinId} onChange={e => set('ipinId', e.target.value)} />
            </div>
            <div className="ui-field">
              <label htmlFor="vfIpinPw">비밀번호</label>
              <input id="vfIpinPw" type="password" autoComplete="current-password" maxLength={40} value={f.ipinPw} onChange={e => set('ipinPw', e.target.value)} />
            </div>
          </>
        ) : (
          <>
            <div className="ui-field">
              <label htmlFor="vfBirth">생년월일 · 주민등록번호 뒷자리 첫 숫자</label>
              <div className="aq-verify-rrn">
                <input id="vfBirth" inputMode="numeric" placeholder="YYMMDD" value={f.birth} onChange={e => set('birth', digits(e.target.value, 6))} />
                <span aria-hidden="true">-</span>
                <input aria-label="주민등록번호 뒷자리 첫 숫자" inputMode="numeric" placeholder="0" value={f.sex} onChange={e => set('sex', digits(e.target.value, 1))} />
                <span className="aq-verify-dots" aria-hidden="true">●●●●●●</span>
              </div>
            </div>
            {method === 'card' ? (
              <div className="ui-field">
                <label htmlFor="vfCard">카드사</label>
                <select id="vfCard" value={f.card} onChange={e => set('card', e.target.value)}>
                  {CARD_COMPANIES.map(c => <option key={c}>{c}</option>)}
                </select>
              </div>
            ) : (
              <div className="ui-field">
                <label htmlFor="vfPhone">휴대폰 번호</label>
                <div className="aq-verify-phone">
                  <select aria-label="통신사" value={f.carrier} onChange={e => set('carrier', e.target.value)}>
                    {CARRIERS.map(c => <option key={c}>{c}</option>)}
                  </select>
                  <input id="vfPhone" inputMode="numeric" autoComplete="tel-national" placeholder="01012345678" value={f.phone} onChange={e => set('phone', digits(e.target.value, 11))} />
                </div>
              </div>
            )}
          </>
        )}
        {error && <p className="ui-field-help aq-help-error" role="alert">{error}</p>}
        <button type="submit" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy}>
          {method === 'sms' ? '인증번호 받기'
            : method === 'ipin' ? (busy ? '인증하는 중' : '인증하기')
            : method === 'pass' ? 'PASS 앱으로 인증 요청'
            : `${f.card} 앱으로 인증 요청`}
        </button>
      </form>
    );
  }

  if (step === 'code') {
    const mm = String(Math.floor(left / 60));
    const ss = String(left % 60).padStart(2, '0');
    return (
      <form className="aq-verify" onSubmit={submitCode} noValidate>
        {notice}
        <p className="aq-verify-lead">{f.phone.replace(/^(\d{3})(\d{3,4})(\d{4})$/, '$1-$2-$3')}로 보낸 인증번호 6자리를 입력해 주세요.</p>
        <div className="ui-field">
          <label htmlFor="vfCode" className="sr-only">인증번호</label>
          <div className="aq-verify-code">
            <input id="vfCode" autoFocus inputMode="numeric" autoComplete="one-time-code" placeholder="인증번호 6자리" value={f.code} onChange={e => set('code', digits(e.target.value, 6))} />
            <span className={left < 30 ? 'is-low' : ''}>{mm}:{ss}</span>
          </div>
        </div>
        <button type="button" className="aq-verify-resend" onClick={() => { setLeft(180); set('code', ''); }}>인증번호 다시 받기</button>
        {error && <p className="ui-field-help aq-help-error" role="alert">{error}</p>}
        <button type="submit" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy}>{busy ? '확인하는 중' : '확인'}</button>
      </form>
    );
  }

  // PASS·카드사: 앱에서 승인하기를 기다리는 화면
  return (
    <div className="aq-verify aq-verify-wait">
      {notice}
      <span className="aq-verify-pulse" aria-hidden="true"><Glyph name={method === 'card' ? 'card' : 'lock'} size={26} /></span>
      <strong>{method === 'card' ? `${f.card} 앱` : 'PASS 앱'}에서 인증을 완료해 주세요</strong>
      <p>휴대폰으로 받은 알림을 눌러 인증을 마친 뒤, 아래 버튼을 눌러 주세요.</p>
      <button type="button" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy} onClick={finish}>{busy ? '확인하는 중' : '인증 완료'}</button>
      <button type="button" className="aq-verify-resend" onClick={() => setStep('form')}>다른 정보로 다시 요청</button>
    </div>
  );
}
