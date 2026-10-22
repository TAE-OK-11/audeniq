// AI 앱 화면의 공통 틀 — 연결(/ai/connect)·권한 추가 승인(/ai/approve/:id)·작업 승인(/ai/actions/:id).
// 발매 신청서(pages/Upload.tsx)와 같은 틀: 스튜디오 메뉴 없이 흰 바탕, 위에 고정된 제목 막대(로그인 계정),
// 신청서의 단계 제목, 아래 고정 버튼(step-actions). AI 앱의 앱 안 브라우저에서 한 화면으로 열린다.
import { useEffect, useRef, useState } from 'react';
import { useAuth } from '../api/auth';
import { useProfile } from '../store/profile';
import { aiActionRequests, aiPermissionRequests } from '../api/aiConnections';
import { Link } from '../lib/router';
import { MOCK } from '../lib/mode';
import { IconTile, type Tone } from '../ui';
import { Glyph } from './Glyph';
import { BrandMark } from './BrandMark';
import '../styles/ai-connect.css';

export const workspaceName = (name: string) => (name === 'Personal workspace' ? '내 작업 공간' : name);

function BackIcon() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" width="24" height="24" fill="none"
      stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="m15 18-6-6 6-6" />
    </svg>
  );
}

/** 신청서와 같은 화면 모드(흰 바탕, 아래 고정 버튼 자리)와 좁은 한 단 */
export function AiFlowPage({ children }: { children: React.ReactNode }) {
  useEffect(() => {
    document.body.classList.add('wizard-mode');
    return () => document.body.classList.remove('wizard-mode');
  }, []);
  return (
    <div className="portal-layout aq-connect-layout">
      <section id="view-new" className="view aq-connect-view">
        <div className="wizard">{children}</div>
      </section>
    </div>
  );
}

/** 신청서와 같은 위 막대(제목·로그인 계정). 한 화면에서 끝나 진행 막대는 없다 */
export function AiFlowFrame({ title = 'AI 앱 연결', onBack, children }: { title?: string; onBack?: () => void; children: React.ReactNode }) {
  const { user } = useAuth();
  const profile = useProfile();
  return (
    <>
      <div className="aq-wiz-top">
        <div className="wizard-topbar" aria-label={title}>
          {onBack
            ? <button type="button" className="wizard-topback" aria-label="이전으로 돌아가기" onClick={onBack}><BackIcon /></button>
            : <span className="wizard-topback aq-connect-topspace" aria-hidden="true" />}
          <span className="wizard-top-title">{title}</span>
          <span className="aq-save-slot">
            {/* 스튜디오 머리글과 같은 계정 아바타 — 이메일은 길어서 잘리니 첫 글자만, 전체는 읽어 주기·툴팁으로 */}
            {user && (
              <span className="ui-avatar aq-flow-me" title={user.email} role="img" aria-label={`로그인한 계정 ${user.email ?? ''}`}>
                {(profile.name || user.email || 'A').slice(0, 1).toUpperCase()}
              </span>
            )}
          </span>
        </div>
      </div>
      {children}
    </>
  );
}

export function AiFlowHeader({ kicker, title, sub }: { kicker: string; title: React.ReactNode; sub?: React.ReactNode }) {
  return (
    <div className="wizard-header">
      <p className="aq-step-kicker">{kicker}</p>
      <h1 className="aq-step-title">{title}</h1>
      {sub && <p>{sub}</p>}
    </div>
  );
}

export function AiFlowLoading({ text }: { text: string }) {
  return <div className="ui-callout aq-loading-notice" role="status"><span className="aq-spinner" aria-hidden="true" /> {text}</div>;
}

/** 요청한 AI 앱 한 줄 — 첫 글자 표시, 이름, 작은 설명 */
export function AiAppRow({ name, brand, detail, tag }: { name: string; brand?: string | null; detail?: React.ReactNode; tag?: React.ReactNode }) {
  return (
    <div className="aq-connect-app-row">
      {brand
        ? <BrandMark id={brand} label={name} size={40} className="aq-flow-brand" />
        : <span className="aq-connect-app-mark" aria-hidden="true">{appInitial(name)}</span>}
      <span className="min-0">
        <strong>{name}</strong>
        {detail && <small>{detail}</small>}
      </span>
      {tag}
    </div>
  );
}

/** 승인 화면 맨 위 — 요청한 앱의 아바타(계정 정보 화면과 같은 모양)와 큰 제목. 처리가 끝났으면 아바타 대신 결과 아이콘 칸(IconTile) */
export function AiFlowHero({ icon, app, brand, kicker, title, sub }: {
  icon?: { kind: 'ok' | 'off' | 'warn'; tone: Tone }; app?: string; brand?: string | null; kicker: string; title: React.ReactNode; sub?: React.ReactNode;
}) {
  return (
    <div className="aq-flow-hero">
      {icon && <IconTile tone={icon.tone} size="lg"><AiFlowStatusIcon kind={icon.kind} /></IconTile>}
      {!icon && app && brand && <BrandMark id={brand} label={app} size={56} className="aq-flow-brand aq-flow-avatar-logo" />}
      {!icon && app && !brand && <span className="ui-avatar is-xl aq-profile-avatar aq-flow-avatar" aria-hidden="true">{appInitial(app)}</span>}
      <AiFlowHeader kicker={kicker} title={title} sub={sub} />
    </div>
  );
}

export const appInitial = (name: string) => (name.match(/[\p{L}\p{N}]/u)?.[0] ?? 'A').toUpperCase();

/** 계정 정보 화면의 묶음 카드(aq-set-card) — 회색 소제목 + 줄 */
export function AiSetCard({ title, children }: { title: React.ReactNode; children: React.ReactNode }) {
  return (
    <section className="aq-set-card aq-flow-set">
      <h2>{title}</h2>
      {children}
    </section>
  );
}

/** 줄 — 왼쪽 이름, 오른쪽 값 (눌러서 고치는 줄이 아니라 보여 주기만). stack: 긴 글은 이름 아래에 */
export function AiSetRow({ label, value, stack }: { label: React.ReactNode; value: React.ReactNode; stack?: boolean }) {
  return (
    <div className={`aq-set-row aq-flow-row${stack ? ' is-stack' : ''}`}>
      <span className="aq-set-label">{label}</span>
      <span className="aq-set-value">{value}</span>
    </div>
  );
}

/** 두 줄 항목 — 굵은 이름(꼬리표) 아래 설명 */
export function AiSetItem({ title, tag, children }: { title: React.ReactNode; tag?: React.ReactNode; children?: React.ReactNode }) {
  return (
    <div className="aq-flow-item">
      <strong>{title}{tag}</strong>
      {children && <span>{children}</span>}
    </div>
  );
}

/** 결과 표시 — 마침(✓)·거절(✕)·멈춤(!) */
export function AiFlowStatusIcon({ kind }: { kind: 'ok' | 'off' | 'warn' }) {
  return (
    <svg viewBox="0 0 24 24" width="26" height="26" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round">
      {kind === 'ok' && <path d="M5 12.5 9.5 17 19 7.5" />}
      {kind === 'off' && <path d="M7 7l10 10M17 7 7 17" />}
      {kind === 'warn' && <path d="M12 7v6M12 17h.01" />}
    </svg>
  );
}

/** 지금 시각 — 남은 시간을 세려고 every ms마다 다시 그린다 */
export function useNow(every = 15_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), every);
    return () => window.clearInterval(t);
  }, [every]);
  return now;
}

/** 남은 시간 한 줄 — '47분 남았어요', '23시간 10분 남았어요', '1분도 안 남았어요' */
export function timeLeft(expiresAt: string, now: number): string {
  const min = Math.floor((new Date(expiresAt).getTime() - now) / 60_000);
  if (min < 1) return '1분도 안 남았어요';
  if (min < 60) return `${min}분 남았어요`;
  const h = Math.floor(min / 60), m = min % 60;
  return m ? `${h}시간 ${m}분 남았어요` : `${h}시간 남았어요`;
}

/** 열어 둔 화면을 최신으로 — 다른 기기·탭에서 답했을 수 있으니 화면으로 돌아오면 다시 읽고,
 *  처리 중(poll)이면 끝날 때까지 3초마다(최대 1분) 다시 읽는다 */
export function useFreshRequest(load: () => void, { active, poll }: { active: boolean; poll: boolean }) {
  const loadRef = useRef(load);
  loadRef.current = load;
  useEffect(() => {
    if (!active || MOCK) return;
    const onVisible = () => { if (document.visibilityState === 'visible') loadRef.current(); };
    document.addEventListener('visibilitychange', onVisible);
    window.addEventListener('focus', onVisible);
    return () => { document.removeEventListener('visibilitychange', onVisible); window.removeEventListener('focus', onVisible); };
  }, [active]);
  useEffect(() => {
    if (!poll || MOCK) return;
    let n = 0;
    const t = window.setInterval(() => { if (++n > 20) window.clearInterval(t); else loadRef.current(); }, 3000);
    return () => window.clearInterval(t);
  }, [poll]);
}

/** 답을 기다리는 다른 요청 — 하나에 답한 뒤 다음 요청으로 바로 가게 */
export function AiNextRequest({ current }: { current: string }) {
  const [next, setNext] = useState<{ to: string; count: number } | null>(null);
  useEffect(() => {
    if (MOCK) return;
    let alive = true;
    const now = Date.now();
    Promise.all([aiPermissionRequests().catch(() => []), aiActionRequests().catch(() => [])]).then(([perms, actions]) => {
      const open = [
        ...(perms ?? []).map(r => ({ ...r, to: `/ai/approve/${r.id}` })),
        ...(actions ?? []).map(r => ({ ...r, to: `/ai/actions/${r.id}` })),
      ].filter(r => r.id !== current && r.status === 'PENDING' && !r.connection_revoked && new Date(r.expires_at).getTime() > now)
        .sort((a, b) => a.expires_at.localeCompare(b.expires_at));
      if (alive) setNext(open.length ? { to: open[0].to, count: open.length } : null);
    }).catch(() => {}); // 다음 요청 안내는 덤 — 못 불러오면 보여 주지 않는다
    return () => { alive = false; };
  }, [current]);
  if (!next) return null;
  return (
    <section className="aq-set-card aq-flow-set">
      <h2>답을 기다리는 요청이 {next.count}개 더 있어요</h2>
      <Link className="aq-set-row" to={next.to}>
        <span className="aq-set-label">{next.count > 1 ? '시간이 가장 적게 남은 요청 보기' : '다음 요청 보기'}</span>
        <span className="aq-set-value" />
        <Glyph name="chevron-right" size={16} className="aq-set-chev" />
      </Link>
    </section>
  );
}
