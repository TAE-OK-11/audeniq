import { useState, type ReactNode } from 'react';
import { Link } from '../lib/router';
import { Glyph, type GlyphName } from './Glyph';
import { IconTile } from '../ui';
import { revealField } from '../lib/motion';

/** 로그인·회원가입·계정 찾기 틀 — 스튜디오 머리와 같은 로고 줄 + 스튜디오 카드(ui-card)와 같은 면.
 *  카드 머리는 발매 상세 '관리' 카드처럼 아이콘 칸 + 제목 + 설명. */
export function AuthLayout({ title, sub, icon = 'user', step, children }: {
  title: string; sub: string; icon?: GlyphName; children: ReactNode;
  /** 여러 단계로 나눈 흐름(회원가입)이면 카드 맨 위에 몇 번째 단계인지 */
  step?: { at: number; of: number };
}) {
  return (
    // 로그인·가입 화면에도 본문 영역(main)을 — 화면 읽기 프로그램이 바로 건너갈 곳이 없었다
    <main className="auth-page">
      <div className="aq-auth-glow" aria-hidden="true" />
      <Link to="/login" className="aq-auth-brand" aria-label="AUDENIQ STUDIO">
        <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
        <span className="ui-product">STUDIO</span>
      </Link>
      <div className="auth-split">
        <div className="auth-card ui-card">
          {step && (
            <div className="aq-auth-steps" role="progressbar" aria-label="가입 단계" aria-valuemin={1} aria-valuemax={step.of} aria-valuenow={step.at}>
              {Array.from({ length: step.of }, (_, i) => <span key={i} className={i < step.at ? 'is-on' : undefined} />)}
              <small>{step.at} / {step.of}</small>
            </div>
          )}
          <header className="aq-auth-head">
            <IconTile size="lg"><Glyph name={icon} size={22} /></IconTile>
            <h1>{title}</h1>
            <p className="auth-sub">{sub}</p>
          </header>
          {children}
        </div>
      </div>
      <p className="auth-foot">
        <span>© AUDENIQ</span>
        <span aria-hidden="true">·</span>
        <Link to="/find-account">도움이 필요하신가요?</Link>
        <span aria-hidden="true">·</span>
        <Link to="/terms">이용약관</Link>
      </p>
    </main>
  );
}

/** 마우스·트랙패드가 있는 기기에서만 첫 칸에 바로 커서를 둔다.
 *  휴대폰은 자동 포커스로 키보드가 뜨지 않은 채 칸만 회색(입력 중)으로 보여 헷갈린다. */
export const AUTO_FOCUS = typeof window !== 'undefined' && !!window.matchMedia?.('(pointer: fine)').matches;

/** 이메일 칸 공통 속성 — 휴대폰 키보드가 첫 글자를 대문자로 바꾸거나 맞춤법 교정을 하지 않게 */
export const EMAIL_INPUT = {
  type: 'email', inputMode: 'email', autoCapitalize: 'none', autoCorrect: 'off', spellCheck: false, enterKeyHint: 'next',
} as const;

export const isEmail = (v: string) => /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(v.trim());

// 자주 쓰는 메일 도메인 — 'gamil.com'·'naver.co'처럼 한두 글자 틀린 주소를 칸 아래에서 바로잡게 한다
const MAIL_DOMAINS = ['gmail.com', 'naver.com', 'daum.net', 'hanmail.net', 'kakao.com', 'nate.com', 'icloud.com', 'outlook.com', 'hotmail.com', 'yahoo.com'];

function editDistance(a: string, b: string): number {
  const d = Array.from({ length: a.length + 1 }, (_, i) => [i, ...Array(b.length).fill(0)] as number[]);
  for (let j = 1; j <= b.length; j++) d[0][j] = j;
  for (let i = 1; i <= a.length; i++) for (let j = 1; j <= b.length; j++) {
    d[i][j] = Math.min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
    if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) d[i][j] = Math.min(d[i][j], d[i - 2][j - 2] + 1);
  }
  return d[a.length][b.length];
}

/** 'kim@gamil.com' → 'kim@gmail.com'. 맞는 도메인이거나 비슷한 게 없으면 '' */
export function emailSuggestion(v: string): string {
  const m = /^([^\s@]+)@([^\s@]+)$/.exec(v.trim().toLowerCase());
  if (!m) return '';
  const [, user, domain] = m;
  if (MAIL_DOMAINS.includes(domain)) return '';
  let best = '', bestD = 3;
  for (const d of MAIL_DOMAINS) { const n = editDistance(domain, d); if (n < bestD) { best = d; bestD = n; } }
  return best && bestD <= (domain.length > 6 ? 2 : 1) ? `${user}@${best}` : '';
}

/** 이메일 칸 아래 '혹시 …인가요?' — 누르면 그 주소로 바꾼다 */
export function EmailSuggest({ value, onApply }: { value: string; onApply: (v: string) => void }) {
  const hint = emailSuggestion(value);
  if (!hint) return null;
  return (
    <p className="ui-field-help aq-email-suggest" role="status">
      혹시 <b>{hint}</b>인가요? <button type="button" className="aq-inline-link" onClick={() => onApply(hint)}>바꾸기</button>
    </p>
  );
}

/** 칸 아래 빨간 안내 — 칸의 aria-describedby로 이어 화면 읽기 프로그램도 읽는다 */
export function FieldError({ id, children }: { id: string; children?: ReactNode }) {
  if (!children) return null;
  return <p className="ui-field-help aq-help-error" id={id} role="alert">{children}</p>;
}

/** 키보드의 '다음'(enterKeyHint=next) — Enter가 폼을 제출해 빈 칸 오류가 한꺼번에 뜨던 것을 다음 칸으로 옮기게.
 *  한글 조합 중의 Enter(글자 확정)는 건드리지 않는다 */
export function nextOnEnter(nextId: string) {
  return (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key !== 'Enter' || e.nativeEvent.isComposing || e.keyCode === 229) return;
    e.preventDefault();
    document.getElementById(nextId)?.focus();
  };
}

/** 검사에서 걸린 첫 칸으로 부드럽게 옮겨 커서를 두고 한 번 흔든다 */
export function focusFirst(ids: (string | false | undefined)[]) {
  const id = ids.find(Boolean);
  if (id) revealField(document.getElementById(id));
}

/** 비밀번호 보기/숨기기 입력 — Caps Lock이 켜져 있으면 칸 아래에 알려 준다 */
export function PasswordInput(props: React.InputHTMLAttributes<HTMLInputElement> & { show: boolean; onToggle: () => void }) {
  const { show, onToggle, onKeyUp, onBlur, ...rest } = props;
  const [caps, setCaps] = useState(false);
  return (
    <>
      <div className="aq-password">
        <input
          {...rest} type={show ? 'text' : 'password'}
          onKeyUp={e => { setCaps(e.getModifierState?.('CapsLock') ?? false); onKeyUp?.(e); }}
          onBlur={e => { setCaps(false); onBlur?.(e); }}
        />
        <button type="button" className="aq-password-toggle" onClick={onToggle} aria-label={show ? '비밀번호 숨기기' : '비밀번호 보기'} aria-pressed={show}>
          {show ? '숨기기' : '보기'}
        </button>
      </div>
      {caps && <p className="ui-field-help is-warn" role="status">Caps Lock이 켜져 있어요.</p>}
    </>
  );
}
