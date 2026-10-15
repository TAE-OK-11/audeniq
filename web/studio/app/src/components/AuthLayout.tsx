import type { ReactNode } from 'react';
import { Link } from '../lib/router';
import { BrandMark } from './BrandMark';
import { ReleaseCover } from './ReleaseCover';
import { Badge } from '../ui';

const SHOW_PLATFORMS: [string, string][] = [['spotify', 'Spotify'], ['apple', 'Apple Music'], ['youtube', 'YouTube Music'], ['melon', '멜론'], ['genie', '지니']];
const SHOW_BARS = [38, 52, 44, 66, 58, 84];

/** 넓은 화면 왼쪽 — 스튜디오 화면 조각(발매 카드 · 플랫폼 · 정산)을 띄워 무엇을 하는 곳인지 보여 준다 */
function AuthShowcase() {
  return (
    <aside className="aq-auth-showcase" aria-hidden="true">
      <div className="aq-auth-copy">
        <span className="aq-auth-eyebrow">AUDENIQ STUDIO</span>
        <h2>내 음악을 세상에,<br />쉽고 투명하게.</h2>
        <p>발매 신청부터 플랫폼 배급, 정산까지 한곳에서.</p>
      </div>
      <div className="aq-auth-stage">
        <div className="aq-auth-float is-release">
          <ReleaseCover id="aq-auth-demo" className="cover aq-auth-float-cover" />
          <div className="min-0">
            <b>첫 번째 싱글</b>
            <small>싱글 · 2026-10-28</small>
          </div>
          <Badge tone="green">발매 완료</Badge>
        </div>
        <div className="aq-auth-float is-platforms">
          <small>서비스 중인 플랫폼</small>
          <div className="aq-auth-logos">
            {SHOW_PLATFORMS.map(([id, label]) => <BrandMark key={id} id={id} label={label} size={34} />)}
          </div>
        </div>
        <div className="aq-auth-float is-payout">
          <small>이번 달 정산 예정</small>
          <b>₩1,284,500</b>
          <div className="aq-auth-bars">{SHOW_BARS.map((h, i) => <i key={i} style={{ height: `${h}%` }} />)}</div>
        </div>
      </div>
    </aside>
  );
}

export function AuthLayout({ title, sub, children }: { title: string; sub: string; children: ReactNode }) {
  return (
    <div className="auth-page">
      <div className="aq-auth-glow" aria-hidden="true" />
      <AuthShowcase />
      <div className="auth-split">
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
