import type { ReactNode } from 'react';
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

/** 비밀번호 보기/숨기기 입력 */
export function PasswordInput(props: React.InputHTMLAttributes<HTMLInputElement> & { show: boolean; onToggle: () => void }) {
  const { show, onToggle, ...rest } = props;
  return (
    <div className="aq-password">
      <input {...rest} type={show ? 'text' : 'password'} />
      <button type="button" className="aq-password-toggle" onClick={onToggle} aria-label={show ? '비밀번호 숨기기' : '비밀번호 보기'} aria-pressed={show}>
        {show ? '숨기기' : '보기'}
      </button>
    </div>
  );
}
