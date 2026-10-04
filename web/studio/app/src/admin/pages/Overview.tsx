import { useEffect } from 'react';
import { Link } from '../../lib/router';
import { useAsync } from '../../hooks/useAsync';
import { Glyph } from '../../components/Glyph';
import { workApi } from '../work/api';
import { ENTITY_KIND } from '../work/labels';
import { ago } from '../labels';
import { useNavigate } from '../../lib/router';
import { Icon } from '../AdminApp';
import { DUTY_LABEL, ROLE_LABEL } from '../labels';
import { CountUp, PageHead, Section, useStaff } from '../ui';
import type { Overview } from '../api';

interface Tile { key: keyof Overview; label: string; hint: string; to: string; icon: string; alert?: boolean }

const TILES: Tile[] = [
  { key: 'review', label: '심사 대기', hint: '판단이 필요한 발매', to: '/admin/reviews', icon: 'review' },
  { key: 'second_approvals', label: '2차 승인', hint: '민감 항목 한 번 더 확인', to: '/admin/approvals', icon: 'approval' },
  { key: 'inquiries', label: '문의 답변 대기', hint: '답을 기다리는 문의', to: '/admin/inquiries', icon: 'inquiry' },
  { key: 'documents', label: '서류 검토', hint: '확인할 권리 증빙', to: '/admin/documents', icon: 'doc' },
  { key: 'deliveries_blocked', label: '배급 문제', hint: '전송이 막힌 플랫폼', to: '/admin/deliveries', icon: 'delivery', alert: true },
  { key: 'correction', label: '보완 진행 중', hint: '아티스트가 고치는 중', to: '/admin/reviews', icon: 'review' },
  { key: 'in_pipeline', label: '자동 검사 중', hint: '시스템이 자동으로 처리 중', to: '/admin/reviews', icon: 'dsp' },
];

export function OverviewPage() {
  const nav = useNavigate();
  const { me, counts, refreshCounts } = useStaff();
  useEffect(refreshCounts, [refreshCounts]);

  const urgent = counts ? counts.review + counts.second_approvals + counts.inquiries + counts.documents : 0;
  const hour = new Date().getHours();
  const greet = hour < 12 ? '좋은 아침이에요' : hour < 18 ? '오늘도 수고 많아요' : '늦게까지 고생 많아요';

  return (
    <div className="view-enter">
      <PageHead
        eyebrow="관리자"
        title="오늘의 업무"
        sub={<>역할 <b>{ROLE_LABEL[me.role]}</b> · 담당 업무 {me.duties.map(d => DUTY_LABEL[d]).join(', ') || '조회 전용'}</>}
      />

      <div className="adm-hero">
        <div>
          <h2>{greet}.<br />{counts ? (urgent ? `처리할 일이 ${urgent}건 있어요.` : '밀린 일이 없어요.') : '대기열을 확인하는 중이에요.'}</h2>
          <p>카드를 누르면 바로 이동해요. 숫자는 1분마다 새로 고쳐져요.</p>
        </div>
        <div className="adm-hero-num" aria-hidden="true">{counts ? <CountUp value={urgent} /> : '–'}<small>처리 대기</small></div>
      </div>

      <MentionsPeek />

      <div className="adm-stats">
        {TILES.map(t => {
          const n = counts ? Number(counts[t.key] ?? 0) : null;
          const cls = ['adm-stat', n === 0 ? 'is-muted' : '', t.alert && n ? 'is-alert' : ''].filter(Boolean).join(' ');
          return (
            <button key={t.key} type="button" className={cls} onClick={() => nav(t.to)}>
              <span className="adm-stat-top">
                <span className="adm-stat-icon"><Icon name={t.icon} /></span>
                
              </span>
              <span>
                <strong>{n == null ? '–' : <CountUp value={n} />}</strong>
                <span className="adm-stat-label">{t.label}</span>
                <span className="small muted adm-stat-hint">{t.hint}</span>
              </span>
            </button>
          );
        })}
      </div>

      {me.role === 'ADMIN' && counts && counts.payout_requests > 0 && (
        <div className="adm-alert adm-alert-row">
          <span>지급 요청 <b>{counts.payout_requests}건</b>이 운영 처리를 기다리고 있어요. (지급 실행은 운영 도구에서만 할 수 있어요)</span>
          <button type="button" className="adm-btn soft small" onClick={() => nav('/admin/payouts')}>지급 요청 보기</button>
        </div>
      )}
    </div>
  );
}

/** 나를 언급한 내부 메모 (해결 전) — 업무함(Phase 2)이 생기기 전까지 오늘의 업무에서 바로 본다 */
function MentionsPeek() {
  const { data } = useAsync(() => workApi.mentions(), []);
  const items = (data?.items ?? []).slice(0, 5);
  if (!data || items.length === 0) return null;
  const link = (kind: string, id: string) => (kind === 'task' ? `/admin/tasks/${id}` : kind === 'release' ? `/admin/reviews/${id}` : kind === 'inquiry' ? `/admin/inquiries/${id}` : null);
  return (
    <Section title="나를 언급한 메모" meta={`해결 전 ${data.items.length}건`}>
      <div className="adm-list">
        {items.map(n => {
          const to = link(n.target_kind, n.target_id);
          const inner = (
            <>
              <span className="adm-row-icon plain" aria-hidden="true"><Glyph name="at" size={20} /></span>
              <span className="adm-min">
                <span className="adm-row-title">{n.body || '첨부파일'}</span>
                <span className="adm-row-meta"><span>{ENTITY_KIND[n.target_kind].label}</span>{n.target?.label && <span>{n.target.label}</span>}</span>
              </span>
              <span className="adm-row-end"><small>{ago(n.created_at)}</small></span>
            </>
          );
          return to ? <Link key={n.id} to={to} className="adm-row">{inner}</Link> : <div key={n.id} className="adm-row">{inner}</div>;
        })}
      </div>
    </Section>
  );
}
