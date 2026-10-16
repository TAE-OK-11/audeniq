// 본인인증 창 — 인증 방법 고르기 → 정보 입력 → (문자: 인증번호 / PASS: 앱 승인 / 카드사: ARS 전화) → 완료.
// 체험 모드: 인증 기관에 요청하지 않고 흐름만 보여 주며, 입력한 이름을 실명으로 브라우저에 저장한다.
// 실서버 모드: 서버(/api/me/identity)가 인증 기관 거래를 확인하고, 확인된 이름을 실명으로 저장한다.
// 인증 기관이 연결되기 전에는 서버가 거절하고, 테스트 기관(test)에서는 이름·생년월일로 테스트 거래를 만든다.
import { useEffect, useRef, useState } from 'react';
import { useModalClose } from './Modal';
import { Glyph } from './Glyph';
import { getAccountSnapshot, patchAccount, VERIFY_LABEL, type VerifyMethod } from '../store/account';
import { MOCK } from '../api/client';
import { verifyAccountIdentity } from '../api/portal';
import { errorMessage } from '../api/errors';

const METHODS: { id: VerifyMethod; title: string; sub: string; icon: string }[] = [
  { id: 'pass', title: 'PASS 인증', sub: '통신사 PASS 앱으로 인증', icon: 'lock' },
  { id: 'sms', title: '휴대폰 문자 인증', sub: '문자로 받은 인증번호 입력', icon: 'mail' },
  { id: 'ipin', title: '아이핀 인증', sub: '아이핀 아이디로 인증', icon: 'user' },
  { id: 'card', title: '카드사 인증', sub: '카드번호 입력 후 ARS 전화로 인증', icon: 'card' },
];
const CARRIERS = ['SKT', 'KT', 'LG U+', 'SKT 알뜰폰', 'KT 알뜰폰', 'LG U+ 알뜰폰'];
const CARD_COMPANIES = ['신한카드', 'KB국민카드', '삼성카드', '현대카드', '롯데카드', '하나카드', '우리카드', 'BC카드', 'NH농협카드'];

type Step = 'method' | 'form' | 'code' | 'approve';

const digits = (v: string, n: number) => v.replace(/\D/g, '').slice(0, n);
const newCaptcha = () => String(Math.floor(100000 + Math.random() * 900000));
/** 주민등록번호 앞 6자리 + 뒷자리 첫 숫자 → YYYY-MM-DD (1·2·5·6: 1900년대, 3·4·7·8: 2000년대, 9·0: 1800년대) */
export function birthFrom(yymmdd: string, sex: string): string | null {
  if (!/^\d{6}$/.test(yymmdd) || !/^\d$/.test(sex)) return null;
  const century = '1256'.includes(sex) ? '19' : '3478'.includes(sex) ? '20' : '18';
  const iso = `${century}${yymmdd.slice(0, 2)}-${yymmdd.slice(2, 4)}-${yymmdd.slice(4, 6)}`;
  const d = new Date(`${iso}T00:00:00Z`);
  return !Number.isNaN(d.getTime()) && d.toISOString().slice(0, 10) === iso ? iso : null;
}
const maskPhone = (p: string) => p.replace(/^(\d{3})(\d{3,4})(\d{4})$/, (_, a: string, b: string, c: string) => `${a}-${'*'.repeat(b.length)}-${c}`);

export function IdentityVerify({ onDone }: { onDone: (name: string, method: VerifyMethod) => void }) {
  const close = useModalClose();
  const [step, setStep] = useState<Step>('method');
  const [method, setMethod] = useState<VerifyMethod>('pass');
  const [f, setF] = useState({ name: '', birth: '', sex: '', carrier: CARRIERS[0], phone: '', card: CARD_COMPANIES[0], cardNo: '', captcha: '', ipinId: '', ipinPw: '', code: '' });
  // 카드사 인증(ARS): 보안문자와 서비스 이용 동의
  const [captcha, setCaptcha] = useState(newCaptcha);
  const [cardAgree, setCardAgree] = useState(false);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [codeFocus, setCodeFocus] = useState(false);
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
    if (!MOCK) { void finishServer(); return; }
    setBusy(true);
    timer.current = window.setTimeout(() => {
      patchAccount({ realName: f.name.trim(), verified: { method, at: new Date().toISOString() } });
      onDone(f.name.trim(), method);
      close();
    }, 700);
  };

  const finishServer = async () => {
    const provider = getAccountSnapshot().provider;
    if (!provider?.ready) return setError('본인인증 서비스(휴대폰·간편인증)를 연결하고 있어요. 연결되면 인증할 수 있어요.');
    if (provider.name !== 'test') return setError('본인인증 창을 여는 연결이 아직 준비되지 않았어요.');
    const birth = birthFrom(f.birth, f.sex);
    if (!birth) return setError(method === 'ipin' ? '테스트 환경에서는 아이핀 대신 휴대폰·PASS·카드사 인증을 써 주세요.' : '생년월일을 확인해 주세요.');
    setBusy(true);
    try {
      const r = await verifyAccountIdentity(`test:${f.name.trim()}:${birth}`);
      patchAccount(r);
      onDone(r.realName, method);
      close();
    } catch (err) {
      setError(errorMessage(err, '본인인증을 마치지 못했어요. 다시 시도해 주세요.'));
    } finally {
      setBusy(false);
    }
  };

  // 한 번에 다 보여 주지 않고, 앞 칸을 채우면 다음 칸이 나타난다 (처음엔 이름만)
  const nameOk = f.name.trim().length >= 2;
  const birthOk = f.birth.length === 6 && f.sex.length === 1;
  const phoneOk = /^01\d{8,9}$/.test(f.phone);
  const shown: string[] = ['name'];
  if (method === 'ipin') { if (nameOk) shown.push('ipin'); }
  else {
    if (nameOk) shown.push('birth');
    if (nameOk && birthOk) shown.push('phone');
    if (method === 'card' && nameOk && birthOk && phoneOk) shown.push('cardNo');
    if (method === 'card' && nameOk && birthOk && phoneOk && f.cardNo.length === 8) shown.push('captcha');
  }
  const has = (g: string) => shown.includes(g);
  // 마지막 칸까지 다 나타나기 전에는 버튼이 '다음' — 누르면 비어 있는 다음 칸으로 간다
  const allShown = has(method === 'ipin' ? 'ipin' : method === 'card' ? 'captcha' : 'phone');
  // 길이가 정해진 칸을 다 채우면 새로 나타난 칸으로 커서를 옮긴다 (이름은 쓰는 중에 빼앗지 않는다 — Enter로 넘어간다)
  const shownCount = useRef(shown.length);
  useEffect(() => {
    const prev = shownCount.current;
    shownCount.current = shown.length;
    if (step !== 'form' || shown.length <= prev) return;
    const last = shown[shown.length - 1];
    const target = last === 'phone' ? 'vfPhone' : last === 'captcha' ? 'vfCaptcha' : '';
    if (target) window.setTimeout(() => document.getElementById(target)?.focus(), 30);
  }, [shown.length, step]); // eslint-disable-line react-hooks/exhaustive-deps
  const nextFromName = (e: React.KeyboardEvent) => {
    if (e.key !== 'Enter' || !nameOk) return;
    e.preventDefault();
    document.getElementById(method === 'ipin' ? 'vfIpinId' : 'vfBirth')?.focus();
  };

  const submitForm = (e: React.FormEvent) => {
    e.preventDefault();
    if (!allShown) {
      const next = !nameOk ? 'vfName' : method === 'ipin' ? 'vfIpinId' : f.birth.length !== 6 ? 'vfBirth' : f.sex.length !== 1 ? 'vfSex' : !phoneOk ? 'vfPhone' : 'vfCardNo';
      if (!nameOk) setError('이름을 입력해 주세요.');
      document.getElementById(next)?.focus();
      return;
    }
    if (!f.name.trim()) return setError('이름을 입력해 주세요.');
    if (method === 'ipin') {
      if (!f.ipinId.trim() || !f.ipinPw) return setError('아이핀 아이디와 비밀번호를 입력해 주세요.');
      return finish();
    }
    if (f.birth.length !== 6 || f.sex.length !== 1) return setError('생년월일 6자리와 주민등록번호 뒷자리 첫 숫자를 입력해 주세요.');
    if (!/^01\d{8,9}$/.test(f.phone)) return setError('휴대폰 번호를 확인해 주세요.');
    if (method === 'card') {
      if (f.cardNo.length !== 8) return setError('카드번호 뒤 8자리를 입력해 주세요.');
      if (f.captcha !== captcha) { setCaptcha(newCaptcha()); set('captcha', ''); return setError('보안문자가 맞지 않아요. 새 보안문자를 입력해 주세요.'); }
      if (!cardAgree) return setError('카드 본인확인 서비스 이용에 동의해 주세요.');
    }
    setStep(method === 'sms' ? 'code' : 'approve');
  };

  const submitCode = (e: React.FormEvent) => {
    e.preventDefault();
    if (left === 0) return setError('인증 시간이 지났어요. 인증번호를 다시 받아 주세요.');
    if (f.code.length !== 6) return setError('인증번호 6자리를 입력해 주세요.');
    finish();
  };

  const notice = MOCK ? <p className="aq-verify-demo">체험용 화면이에요. 실제 본인인증은 진행되지 않아요.</p> : null;

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
          <input id="vfName" autoFocus autoComplete="name" maxLength={40} placeholder="실명" enterKeyHint="next" value={f.name} onChange={e => set('name', e.target.value)} onKeyDown={nextFromName} />
        </div>
        {has('ipin') && (
          <div className="aq-verify-step" data-expand>
            <div className="ui-field">
              <label htmlFor="vfIpinId">아이핀 아이디</label>
              <input id="vfIpinId" autoComplete="username" maxLength={40} value={f.ipinId} onChange={e => set('ipinId', e.target.value)} />
            </div>
            <div className="ui-field">
              <label htmlFor="vfIpinPw">비밀번호</label>
              <input id="vfIpinPw" type="password" autoComplete="current-password" maxLength={40} value={f.ipinPw} onChange={e => set('ipinPw', e.target.value)} />
            </div>
          </div>
        )}
        {has('birth') && (
          <div className="ui-field" data-expand>
            <label htmlFor="vfBirth">주민등록번호 앞 7자리</label>
            <div className="aq-verify-rrn">
              <input id="vfBirth" inputMode="numeric" placeholder="생년월일 6자리" value={f.birth} onChange={e => { const v = digits(e.target.value, 6); set('birth', v); if (v.length === 6) document.getElementById('vfSex')?.focus(); }} />
              <span aria-hidden="true">-</span>
              <input id="vfSex" aria-label="주민등록번호 뒷자리 첫 숫자" inputMode="numeric" placeholder="0" value={f.sex} onChange={e => set('sex', digits(e.target.value, 1))} />
              <span className="aq-verify-dots" aria-hidden="true">●●●●●●</span>
            </div>
          </div>
        )}
        {has('phone') && (
          <div className="aq-verify-step" data-expand>
            {method === 'card' && (
              <div className="ui-field">
                <label htmlFor="vfCard">카드사</label>
                <select id="vfCard" value={f.card} onChange={e => set('card', e.target.value)}>
                  {CARD_COMPANIES.map(c => <option key={c}>{c}</option>)}
                </select>
              </div>
            )}
            <div className="ui-field">
              <label htmlFor="vfPhone">휴대폰 번호</label>
              {method === 'card'
                ? <input id="vfPhone" inputMode="numeric" autoComplete="tel-national" placeholder="숫자만 입력" value={f.phone} onChange={e => { const v = digits(e.target.value, 11); set('phone', v); if (v.length === 11) window.setTimeout(() => document.getElementById('vfCardNo')?.focus(), 30); }} />
                : (
                  <div className="aq-verify-phone">
                    <select aria-label="통신사" value={f.carrier} onChange={e => set('carrier', e.target.value)}>
                      {CARRIERS.map(c => <option key={c}>{c}</option>)}
                    </select>
                    <input id="vfPhone" inputMode="numeric" autoComplete="tel-national" placeholder="숫자만 입력" value={f.phone} onChange={e => set('phone', digits(e.target.value, 11))} />
                  </div>
                )}
            </div>
          </div>
        )}
        {has('cardNo') && (
          // 실제 카드 본인확인(ARS)과 같은 입력 — 카드번호 뒤 8자리 · 보안문자 · 이용 동의
          <div className="ui-field" data-expand>
            <label htmlFor="vfCardNo">카드번호 뒤 8자리</label>
            <input id="vfCardNo" className="aq-verify-cardno" inputMode="numeric" autoComplete="off" placeholder="0000 0000"
              value={f.cardNo.replace(/^(\d{4})(\d)/, '$1 $2')} onChange={e => set('cardNo', digits(e.target.value, 8))} />
            <p className="ui-field-help">본인 명의 신용·체크카드만 돼요.</p>
          </div>
        )}
        {has('captcha') && (
          <div className="aq-verify-step" data-expand>
            <div className="ui-field">
              <label htmlFor="vfCaptcha">보안문자</label>
              <div className="aq-verify-captcha">
                <span className="aq-captcha-img" aria-label={`보안문자 ${captcha.split('').join(' ')}`} role="img">
                  {captcha.split('').map((d, i) => <i key={i} style={{ ['--r' as string]: `${(i % 2 ? 1 : -1) * (6 + i * 2)}deg` }}>{d}</i>)}
                </span>
                <button type="button" className="aq-captcha-refresh" onClick={() => { setCaptcha(newCaptcha()); set('captcha', ''); }} aria-label="보안문자 새로 받기"><Glyph name="undo" size={16} /></button>
                <input id="vfCaptcha" inputMode="numeric" autoComplete="off" placeholder="숫자 6자리" value={f.captcha} onChange={e => set('captcha', digits(e.target.value, 6))} />
              </div>
            </div>
            <label className="ui-check aq-verify-agree">
              <input type="checkbox" checked={cardAgree} onChange={e => { setCardAgree(e.target.checked); setError(''); }} />
              <span>카드 본인확인 서비스 이용약관과 개인정보 수집·이용에 동의해요 (필수)</span>
            </label>
          </div>
        )}
        {error && <p className="ui-field-help aq-help-error" role="alert">{error}</p>}
        <button type="submit" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy}>
          {!allShown ? '다음'
            : method === 'sms' ? '인증번호 받기'
            : method === 'ipin' ? (busy ? '인증하는 중' : '인증하기')
            : method === 'pass' ? 'PASS 앱으로 인증 요청'
            : 'ARS 인증 요청'}
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
          <label htmlFor="vfCode" className="sr-only">인증번호 6자리</label>
          {/* 입력은 칸 하나로 받고(문자 자동 채우기·붙여넣기 유지), 화면에는 숫자를 한 칸씩 나눠 보여 준다 */}
          <div className="aq-otp">
            <input
              id="vfCode" autoFocus inputMode="numeric" autoComplete="one-time-code" maxLength={6}
              className="aq-otp-input" value={f.code}
              onChange={e => set('code', digits(e.target.value, 6))}
              onFocus={() => setCodeFocus(true)} onBlur={() => setCodeFocus(false)}
            />
            {Array.from({ length: 6 }, (_, i) => (
              <span
                key={i} aria-hidden="true"
                className={`aq-otp-cell${f.code[i] ? ' is-filled' : ''}${codeFocus && i === Math.min(f.code.length, 5) ? ' is-active' : ''}`}
              >
                {f.code[i] ?? ''}
              </span>
            ))}
          </div>
        </div>
        <div className="aq-otp-meta">
          <span className={left < 30 ? 'is-low' : ''}>남은 시간 {mm}:{ss}</span>
          <button type="button" className="aq-verify-resend" onClick={() => { setLeft(180); set('code', ''); }}>인증번호 다시 받기</button>
        </div>
        {error && <p className="ui-field-help aq-help-error" role="alert">{error}</p>}
        <button type="submit" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy}>{busy ? '확인하는 중' : '확인'}</button>
      </form>
    );
  }

  // 카드사: ARS 전화를 받아 카드 비밀번호 앞 2자리를 누르는 단계
  if (method === 'card') {
    return (
      <div className="aq-verify aq-verify-wait">
        {notice}
        <span className="aq-verify-pulse" aria-hidden="true"><Glyph name="bell" size={26} /></span>
        <strong>{f.card} 인증 전화가 곧 와요</strong>
        <p>{maskPhone(f.phone)}로 전화를 걸고 있어요. 전화를 받고 안내에 따라 <b>카드 비밀번호 앞 2자리</b>를 휴대폰 버튼으로 눌러 주세요.</p>
        <button type="button" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy} onClick={finish}>{busy ? '확인하는 중' : '통화를 마쳤어요'}</button>
        <button type="button" className="aq-verify-resend" onClick={() => { setStep('form'); setCaptcha(newCaptcha()); set('captcha', ''); }}>전화가 오지 않나요? 다시 요청하기</button>
      </div>
    );
  }

  // PASS: 앱에서 승인하기를 기다리는 화면
  return (
    <div className="aq-verify aq-verify-wait">
      {notice}
      <span className="aq-verify-pulse" aria-hidden="true"><Glyph name="lock" size={26} /></span>
      <strong>PASS 앱에서 인증을 완료해 주세요</strong>
      <p>휴대폰으로 받은 알림을 눌러 인증을 마친 뒤, 아래 버튼을 눌러 주세요.</p>
      <button type="button" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy} onClick={finish}>{busy ? '확인하는 중' : '인증 완료'}</button>
      <button type="button" className="aq-verify-resend" onClick={() => setStep('form')}>다른 정보로 다시 요청</button>
    </div>
  );
}
