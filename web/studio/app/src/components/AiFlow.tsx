// AI 앱 화면의 공통 틀 — 연결(/ai/connect)·권한 추가 승인(/ai/approve/:id)·작업 승인(/ai/actions/:id).
// 발매 신청서(pages/Upload.tsx)와 같은 틀: 스튜디오 메뉴 없이 흰 바탕, 위에 고정된 제목 막대(로그인 계정),
// 신청서의 단계 제목, 아래 고정 버튼(step-actions). AI 앱의 앱 안 브라우저에서 한 화면으로 열린다.
import { useEffect } from 'react';
import { useAuth } from '../api/auth';
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
  return (
    <>
      <div className="aq-wiz-top">
        <div className="wizard-topbar" aria-label={title}>
          {onBack
            ? <button type="button" className="wizard-topback" aria-label="이전으로 돌아가기" onClick={onBack}><BackIcon /></button>
            : <span className="wizard-topback aq-connect-topspace" aria-hidden="true" />}
          <span className="wizard-top-title">{title}</span>
          <span className="aq-save-slot">
            {user?.email && <span className="aq-connect-account" title="로그인한 계정">{user.email}</span>}
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
export function AiAppRow({ name, detail, tag }: { name: string; detail?: React.ReactNode; tag?: React.ReactNode }) {
  return (
    <div className="aq-connect-app-row">
      <span className="aq-connect-app-mark" aria-hidden="true">{(name.match(/[\p{L}\p{N}]/u)?.[0] ?? 'A').toUpperCase()}</span>
      <span className="min-0">
        <strong>{name}</strong>
        {detail && <small>{detail}</small>}
      </span>
      {tag}
    </div>
  );
}

/** 승인 화면 맨 위 — 앱 첫 글자(또는 결과 표시)와 큰 제목. 신청서 단계 제목과 같은 글씨 */
export function AiFlowHero({ mark, tone, kicker, title, sub }: {
  mark: React.ReactNode; tone?: 'ok' | 'off' | 'warn'; kicker: string; title: React.ReactNode; sub?: React.ReactNode;
}) {
  return (
    <div className="aq-flow-hero">
      <span className={`aq-flow-mark${tone ? ` is-${tone}` : ''}`} aria-hidden="true">{mark}</span>
      <AiFlowHeader kicker={kicker} title={title} sub={sub} />
    </div>
  );
}

export const appInitial = (name: string) => (name.match(/[\p{L}\p{N}]/u)?.[0] ?? 'A').toUpperCase();

/** 신청서 '접수 전 확인'과 같은 줄 목록 — 왼쪽 이름, 오른쪽 값 */
export function AiFlowRows({ rows }: { rows: [string, React.ReactNode][] }) {
  return (
    <dl className="aq-flow-rows">
      {rows.map(([k, v]) => <div key={k}><dt>{k}</dt><dd>{v}</dd></div>)}
    </dl>
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
