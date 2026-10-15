import { useEffect } from 'react';
import { Link } from '../../lib/router';
import { useAsync } from '../../hooks/useAsync';
import { Glyph } from '../../components/Glyph';
import { workApi } from '../work/api';
import { INBOX_CATEGORY } from '../work/labels';
import { DueChip } from '../work/ui';
import { ago } from '../labels';
import { ADMIN_INQUIRIES_EVENT, ADMIN_WORK_EVENT } from '../liveEvents';
import { useNavigate } from '../../lib/router';
import { Icon } from '../AdminApp';
import { DUTY_LABEL, ROLE_LABEL } from '../labels';
import { CountUp, PageHeader, Section, useDualApproval, useStaff } from '../ui';
import { IconTile, StatGrid, StatTile } from '../../ui';
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
  const dual = useDualApproval();
  useEffect(refreshCounts, [refreshCounts]);

  const urgent = counts ? counts.review + counts.second_approvals + counts.inquiries + counts.documents : 0;
  const hour = new Date().getHours();
  const greet = hour < 12 ? '좋은 아침이에요' : hour < 18 ? '오늘도 수고 많아요' : '늦게까지 고생 많아요';

  return (
    <div className="view-enter">
      <PageHeader
        eyebrow="관리자"
        title="오늘의 업무"
        sub={<>역할 <b>{ROLE_LABEL[me.role]}</b> · 담당 업무 {me.duties.map(d => DUTY_LABEL[d]).join(', ') || '조회 전용'}</>}
      />

      <div className="ui-hero adm-hero">
        <div>
          <h2>{greet}.<br />{counts ? (urgent ? `처리할 일이 ${urgent}건 있어요.` : '밀린 일이 없어요.') : '대기열을 확인하는 중이에요.'}</h2>
          <p>카드를 누르면 바로 이동해요. 숫자는 1분마다 새로 고쳐져요.</p>
        </div>
        <div className="adm-hero-num" aria-hidden="true">{counts ? <CountUp value={urgent} /> : '–'}<small>처리 대기</small></div>
      </div>

      <StatGrid>
        {TILES.filter(t => dual || t.key !== 'second_approvals').map(t => {
          const n = counts ? Number(counts[t.key] ?? 0) : null;
          return (
            <StatTile
              key={t.key} onClick={() => nav(t.to)}
              tone={n === 0 ? 'muted' : t.alert && n ? 'alert' : undefined}
              label={t.label} hint={t.hint}
              icon={<IconTile size="sm"><Icon name={t.icon} /></IconTile>}
              value={n == null ? '–' : <CountUp value={n} />}
            />
          );
        })}
      </StatGrid>

      {/* 업무함은 늦게 불러와지므로 숫자 칸 아래에 둔다 — 위에 끼어들며 이미 보이는 칸들을 밀어내지 않게 */}
      <InboxPeek />

      {me.role === 'ADMIN' && counts && counts.payout_requests > 0 && (
        <div className="ui-callout is-row">
          <span>지급 요청 <b>{counts.payout_requests}건</b>이 운영 처리를 기다리고 있어요. (지급 실행은 운영 도구에서만 할 수 있어요)</span>
          <button type="button" className="ui-btn is-soft is-sm" onClick={() => nav('/admin/payouts')}>지급 요청 보기</button>
        </div>
      )}
    </div>
  );
}

/** 업무함 미리 보기 — 나에게 온 일 중 읽지 않은 것 (멘션·검토 요청·SLA·예외 포함) */
function InboxPeek() {
  const { data, reload } = useAsync(() => workApi.inbox({ unread: true }), []);
  useEffect(() => {
    window.addEventListener(ADMIN_WORK_EVENT, reload);
    window.addEventListener(ADMIN_INQUIRIES_EVENT, reload);
    return () => { window.removeEventListener(ADMIN_WORK_EVENT, reload); window.removeEventListener(ADMIN_INQUIRIES_EVENT, reload); };
  }, [reload]);
  const items = (data?.items ?? []).slice(0, 5);
  if (!data || items.length === 0) return null;
  return (
    <Section title="업무함" meta={`읽지 않은 일 ${data.unread}건`} action={<Link to="/admin/inbox" className="ui-btn is-soft is-sm">전체 보기</Link>}>
      <div className="ui-list">
        {items.map(i => {
          const c = INBOX_CATEGORY[i.category];
          return (
            <Link key={i.id} to={i.link} className="ui-row" onClick={() => { void workApi.markRead([i.id]).catch(() => {}); }}>
              <span className="ui-cover is-plain" aria-hidden="true"><Glyph name={c.icon} size={20} /></span>
              <span className="adm-min">
                <span className="ui-row-title">{i.title}</span>
                <span className="ui-row-meta"><span>{c.label}</span>{i.subtitle && <span className="adm-w-ellipsis">{i.subtitle}</span>}</span>
              </span>
              <span className="ui-row-end">{i.due_at ? <DueChip due={i.due_at} /> : <small>{ago(i.at)}</small>}</span>
            </Link>
          );
        })}
      </div>
    </Section>
  );
}
