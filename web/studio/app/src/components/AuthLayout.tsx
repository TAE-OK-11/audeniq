import { useState, type ReactNode } from 'react';
import { Link } from '../lib/router';
import { Glyph, type GlyphName } from './Glyph';
import { IconTile } from '../ui';

/** 로그인·회원가입·계정 찾기 틀 — 스튜디오 머리와 같은 로고 줄 + 스튜디오 카드(ui-card)와 같은 면.
 *  카드 머리는 발매 상세 '관리' 카드처럼 아이콘 칸 + 제목 + 설명. */
export function AuthLayout({ title, sub, icon = 'user', children }: { title: string; sub: string; icon?: GlyphName; children: ReactNode }) {
  return (
    <div className="auth-page">
      <div className="aq-auth-glow" aria-hidden="true" />
      <Link to="/login" className="aq-auth-brand" aria-label="AUDENIQ STUDIO">
        <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
        <span className="ui-product">STUDIO</span>
      </Link>
      <div className="auth-split">
        <div className="auth-card ui-card">
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
    </div>
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

/** 칸 아래 빨간 안내 — 칸의 aria-describedby로 이어 화면 읽기 프로그램도 읽는다 */
export function FieldError({ id, children }: { id: string; children?: ReactNode }) {
  if (!children) return null;
  return <p className="ui-field-help aq-help-error" id={id} role="alert">{children}</p>;
}

/** 검사에서 걸린 첫 칸으로 커서를 옮긴다 */
export function focusFirst(ids: (string | false | undefined)[]) {
  const id = ids.find(Boolean);
  if (id) document.getElementById(id)?.focus();
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
