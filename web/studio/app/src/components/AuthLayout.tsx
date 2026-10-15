import type { ReactNode } from 'react';
import { Link } from '../lib/router';
import { Glyph, type GlyphName } from './Glyph';

/** 넓은 화면 왼쪽 브랜드 패널 — 스튜디오에서 할 수 있는 일 */
const FEATURES: { icon: GlyphName; title: string; sub: string }[] = [
  { icon: 'release', title: '발매 신청부터 검토까지', sub: '신청서 작성, 담당자 검토, 보완 요청을 한 화면에서' },
  { icon: 'globe', title: '국내외 플랫폼 배급', sub: 'Spotify · Apple Music · 멜론 등 플랫폼별 진행 확인' },
  { icon: 'won', title: '투명한 정산', sub: '플랫폼별 수익과 정산 내역, 수령까지' },
  { icon: 'users', title: '공동작업자 수익 분배', sub: '곡마다 지분을 나누고 각자 정산받기' },
];

export function AuthLayout({ title, sub, children }: { title: string; sub: string; children: ReactNode }) {
  return (
    <div className="auth-page">
      <div className="aq-auth-glow" aria-hidden="true" />
      <div className="auth-split">
        <aside className="aq-auth-aside" aria-label="AUDENIQ STUDIO 소개">
          <div className="aq-auth-aside-brand">
            <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
            <span>STUDIO</span>
          </div>
          <div>
            <h2>발매부터 정산까지,<br />한 곳에서.</h2>
            <ul className="aq-auth-features">
              {FEATURES.map(f => (
                <li key={f.title}>
                  <span className="aq-auth-feature-icon" aria-hidden="true"><Glyph name={f.icon} size={18} /></span>
                  <span><b>{f.title}</b><small>{f.sub}</small></span>
                </li>
              ))}
            </ul>
          </div>
          <p className="aq-auth-aside-foot">아티스트와 레이블을 위한 음원 배급 스튜디오</p>
        </aside>
        <div className="auth-card">
          <Link to="/login" className="aq-auth-brand" aria-label="AUDENIQ STUDIO">
            <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
            <span>STUDIO</span>
          </Link>
          <h1>{title}</h1>
          <p className="auth-sub">{sub}</p>
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
