// 신청서 틀 — 배급 신청서(pages/Upload.tsx)의 화면을 그대로 다른 신청서(곡 정보·저작권 관리 신청)에서 쓴다.
// 위에 고정된 막대(뒤로 · 제목 · 저장 상태)와 진행 칸, 넓은 화면의 왼쪽 단계 목록, 단계 제목(번호 · 큰 제목 · 안내), 아래 고정 버튼(step-actions).
// 모양은 #view-new 아래 규칙(styles/split/Upload.css 등)을 그대로 받는다
import type { ReactNode } from 'react';
import { Glyph } from './Glyph';
import '../styles/split/Upload.css';

export interface WizardStep { short: string; title: string; sub?: ReactNode }

function BackIcon() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" width="24" height="24" fill="none"
      stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="m15 18-6-6 6-6" />
    </svg>
  );
}

export function WizardShell({
  label, title, steps, step, reached = step, onStep, onBack, saveLabel, summary, railNote, notice, children, actions,
}: {
  /** 흐름 이름 — '저작권 관리 신청' (읽어 주기·왼쪽 진행 막대) */
  label: string;
  /** 위 막대 가운데 제목 */
  title: string;
  steps: readonly WizardStep[];
  step: number;
  /** 지나온 단계 — 그 단계까지는 진행 칸·단계 목록을 눌러 돌아갈 수 있다 */
  reached?: number;
  onStep: (i: number) => void;
  onBack: () => void;
  saveLabel?: string;
  /** 왼쪽 위 요약 (커버 · 제목 · 한 줄) */
  summary?: { cover?: ReactNode; title: ReactNode; lines: ReactNode[] };
  railNote?: ReactNode;
  /** 단계 제목 아래, 본문 위에 붙는 안내 (이어 쓰기 등) */
  notice?: ReactNode;
  children: ReactNode;
  /** 아래 고정 버튼들 — 이전 버튼에는 id="wizardBack", 다음 버튼에는 id="wizardNext" */
  actions: ReactNode;
}) {
  const s = steps[step];
  const n = String(steps.length).padStart(2, '0');
  return (
    <section id="view-new" className="view">
      <div className="wizard">
        <div className="aq-wiz-top">
          <div className="wizard-topbar" aria-label={`${label} 탐색`}>
            <button type="button" className="wizard-topback" aria-label="이전으로 돌아가기" onClick={onBack}><BackIcon /></button>
            <span className="wizard-top-title">{title}</span>
            <span className="aq-save-slot" aria-live="polite">
              {saveLabel && <span key={saveLabel} className="aq-save-state is-saved">{saveLabel}</span>}
            </span>
          </div>
          <div className="wizard-progress" aria-label={`${label} 진행 단계 ${step + 1} / ${steps.length}`}>
            {steps.map((st, i) => {
              const go = i <= reached && i !== step;
              return (
                <span key={st.short} className={`wizard-progress-seg${i <= step ? ' current' : ''}${go ? ' aq-seg-link' : ''}`}
                  onClick={go ? () => onStep(i) : undefined} title={go ? `${st.short}(으)로 이동` : st.short}>
                  {i === step && <em>{st.short}</em>}
                </span>
              );
            })}
          </div>
        </div>

        <div className="aq-wiz-layout">
          <aside className="aq-wiz-rail" aria-label={`${label} 단계`}>
            {summary && (
              <div className="aq-wiz-summary">
                {summary.cover ?? <span className="aq-wiz-summary-cover" aria-hidden="true"><Glyph name="music" size={18} /></span>}
                <div className="min-0">
                  <strong>{summary.title}</strong>
                  {summary.lines.map((l, i) => <span key={i}>{l}</span>)}
                </div>
              </div>
            )}
            <div className="aq-wiz-rail-progress" aria-hidden="true">
              <span>{label}</span>
              <b>{step + 1} / {steps.length}</b>
              <i style={{ ['--p' as string]: `${((step + 1) / steps.length) * 100}%` }} />
            </div>
            <ol className="aq-wiz-steps" style={{ ['--cur' as string]: step }}>
              {steps.map((st, i) => (
                <li key={st.short} className={i === step ? 'is-current' : i <= reached ? 'is-done' : ''}>
                  <button type="button" disabled={i > reached} aria-current={i === step ? 'step' : undefined}
                    onClick={i <= reached && i !== step ? () => onStep(i) : undefined}>
                    <span className="aq-wiz-step-no" aria-hidden="true">
                      {i !== step && i <= reached ? <svg viewBox="0 0 16 16" width="12" height="12"><path d="M3.5 8.5l3 3 6-7" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" /></svg> : i + 1}
                    </span>
                    <span className="aq-wiz-step-label">{st.short}</span>
                  </button>
                </li>
              ))}
            </ol>
            {railNote && <p className="aq-wiz-rail-note">{railNote}</p>}
          </aside>
          <div className="aq-wiz-main">
            <div className="wizard-header" key={`h-${step}`}>
              <p className="aq-step-kicker">{String(step + 1).padStart(2, '0')} / {n} · {s.short}</p>
              <h1 className="aq-step-title">{s.title}</h1>
              {s.sub && <p>{s.sub}</p>}
            </div>
            {notice}
            <div id="wizardBody" key={`b-${step}`}>
              <section className="step-section">{children}</section>
            </div>
          </div>
        </div>

        <div className="step-actions">{actions}</div>
      </div>
    </section>
  );
}

/** 마지막 확인 단계의 한 줄 — 배급 신청서 '최종 확인'과 같은 카드 */
export function ReviewCard({ head, children, missing, onEdit }: { head: string; children: ReactNode; missing?: boolean; onEdit: () => void }) {
  return (
    <div className={`aq-review-card${missing ? ' is-missing' : ''}`}>
      <div className="aq-review-body">
        <strong>{head}</strong>
        <div className="break aq-review-value">{children}</div>
      </div>
      <button type="button" className="ui-btn is-white is-xs aq-review-edit" onClick={onEdit} aria-label={`${head} ${missing ? '채우기' : '수정'}`}>{missing ? '채우기' : '수정'}</button>
    </div>
  );
}
