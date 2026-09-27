import { useEffect } from 'react';
import { useNavigate } from '../lib/router';
import { Icon } from '../AdminApp';
import { DUTY_LABEL, ROLE_LABEL } from '../labels';
import { PageHead, useStaff } from '../ui';
import type { Overview } from '../api/staff';

interface Tile { key: keyof Overview; label: string; hint: string; to: string; icon: string; alert?: boolean }

const TILES: Tile[] = [
  { key: 'review', label: '심사 대기', hint: '새 발매 신청과 2차 검사에서 판단이 필요한 발매', to: '/reviews', icon: 'review' },
  { key: 'second_approvals', label: '2차 승인', hint: '권리·중복 등 민감 항목의 두 번째 확인', to: '/approvals', icon: 'approval' },
  { key: 'inquiries', label: '문의 답변 대기', hint: '아티스트가 남긴 답변 대기 문의', to: '/inquiries', icon: 'inquiry' },
  { key: 'documents', label: '서류 검토', hint: '요청한 권리 증빙 검토 대기', to: '/documents', icon: 'doc' },
  { key: 'deliveries_blocked', label: '배급 문제', hint: '발매 내용 문제로 막힌 플랫폼 배급', to: '/deliveries', icon: 'delivery', alert: true },
  { key: 'correction', label: '보완 진행 중', hint: '아티스트가 수정 중인 발매', to: '/reviews', icon: 'review' },
  { key: 'in_pipeline', label: '자동 검사 중', hint: '접수부터 배급 준비까지 시스템이 처리 중', to: '/reviews', icon: 'dsp' },
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
        eyebrow="AUDENIQ ADMIN"
        title="오늘의 업무"
        sub={<>역할 <b>{ROLE_LABEL[me.role]}</b> · 담당 업무 {me.duties.map(d => DUTY_LABEL[d]).join(', ') || '조회 전용'}</>}
      />

      <div className="adm-hero">
        <div>
          <h2>{greet}.<br />{counts ? (urgent ? `처리할 일이 ${urgent}건 있어요.` : '밀린 일이 없어요.') : '대기열을 확인하는 중이에요.'}</h2>
          <p>심사·2차 승인·서류·문의 대기 건수예요. 카드를 누르면 해당 대기열로 이동해요. 숫자는 1분마다 새로 고쳐져요.</p>
        </div>
        <div className="adm-hero-num" aria-hidden="true">{counts ? urgent : '–'}<small>처리 대기</small></div>
      </div>

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
                <strong>{n ?? '–'}</strong>
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
          <button type="button" className="adm-btn soft small" onClick={() => nav('/payouts')}>지급 요청 보기</button>
        </div>
      )}
    </div>
  );
}
