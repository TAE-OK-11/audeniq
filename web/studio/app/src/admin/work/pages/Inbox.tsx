// 업무함 — 출근하면 이것 하나만 본다. 내 업무 · 승인 요청 · 보완 요청 · 멘션 · 실패한 자동화 · SLA 초과 · 미처리 문의 · 권리 이슈.
import { useState } from 'react';
import { useNavigate, useSearchParams } from '../../../lib/router';
import { useToast } from '../../../components/Toast';
import { errorMessage } from '../../../api/errors';
import { useAsync } from '../../../hooks/useAsync';
import { Glyph } from '../../../components/Glyph';
import { CountUp, Empty, ErrorBox, Filters, PageHead, Seg, Skeleton, useStaff } from '../../ui';
import { ago } from '../../labels';
import { workApi } from '../api';
import { INBOX_KIND, INBOX_KINDS } from '../labels';
import { remaining } from '../sla';
import { MiniStats, PriorityChip } from '../ui';
import type { InboxItem, InboxKind } from '../types';

export function Inbox() {
  const nav = useNavigate();
  const toast = useToast();
  const [params, setParams] = useSearchParams();
  const kind = (params.get('kind') as InboxKind | null) ?? '';
  const [scope, setScope] = useState<'unread' | 'all'>('unread');
  const { work, refreshWork } = useStaff();
  const { data, loading, error, reload } = useAsync(() => workApi.inbox(kind, scope === 'unread'), [kind, scope]);
  const items = data?.items ?? [];

  const open = async (i: InboxItem) => {
    if (!i.read) void workApi.readInbox([i.id]).then(refreshWork).catch(() => {});
    nav(i.link);
  };
  const readAll = async () => {
    const ids = items.filter(i => !i.read).map(i => i.id);
    if (!ids.length) return;
    try {
      await workApi.readInbox(ids);
      toast(`${ids.length}건을 읽음으로 표시했어요.`, 'success');
      reload(); refreshWork();
    } catch (e) {
      toast(errorMessage(e, '읽음으로 표시하지 못했어요.'), 'error');
    }
  };

  const byKind = work?.by_kind ?? {};
  return (
    <div className="view-enter">
      <PageHead
        eyebrow="업무"
        title="업무함"
        sub="나에게 온 일을 한곳에 모았어요. 급하고 기한이 가까운 일부터 보여요."
        actions={<>
          <button type="button" className="adm-btn soft small" onClick={readAll} disabled={!items.some(i => !i.read)}>모두 읽음</button>
          <button type="button" className="adm-btn soft small" onClick={() => { reload(); refreshWork(); }}>새로고침</button>
        </>}
      />
      <MiniStats items={[
        { label: '안 읽은 일', value: work ? <CountUp value={work.inbox_unread} /> : '–', hint: `전체 ${work?.inbox_total ?? 0}건` },
        { label: '내 진행 업무', value: work ? <CountUp value={work.my_open} /> : '–', hint: '할 일·진행·검토·막힘', onClick: () => nav('/admin/tasks') },
        { label: '기한 임박', value: work ? <CountUp value={work.due_soon} /> : '–', tone: work?.due_soon ? 'warn' : 'muted', hint: '남은 시간 25% 이하', onClick: () => nav('/admin/sla') },
        { label: 'SLA 초과', value: work ? <CountUp value={work.overdue} /> : '–', tone: work?.overdue ? 'alert' : 'muted', hint: '자동으로 escalation 돼요', onClick: () => nav('/admin/sla') },
      ]} />
      <Filters
        label="업무함 종류"
        value={kind}
        onChange={v => setParams(v ? { kind: v } : {}, { replace: true })}
        options={[{ value: '' as InboxKind | '', label: '전체', count: work?.inbox_unread }, ...INBOX_KINDS.map(k => ({ value: k as InboxKind | '', label: INBOX_KIND[k].label, count: byKind[k] }))]}
      />
      <div className="adm-queue-tools">
        <Seg label="보기" value={scope} onChange={setScope} options={[['unread', '안 읽은 일'], ['all', '전체']] as const} />
      </div>
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <Skeleton rows={5} /> : items.length === 0 ? (
        <Empty icon={<Glyph name="inbox" size={22} />} title={scope === 'unread' ? '새로 처리할 일이 없어요' : '업무함이 비어 있어요'}>
          {scope === 'unread' ? '배정·멘션·자동화 실패·SLA 초과가 생기면 바로 여기에 모여요.' : undefined}
        </Empty>
      ) : (
        <div className="adm-list">
          {items.map(i => {
            const k = INBOX_KIND[i.kind];
            const due = i.due_at ? remaining(i.due_at) : '';
            const late = !!i.due_at && Date.parse(i.due_at) < Date.now();
            return (
              <button key={i.id} type="button" className={`adm-row adm-w-inbox-row${i.read ? ' is-read' : ''}`} onClick={() => open(i)}>
                <span className={`adm-row-icon plain adm-w-kind is-${k.tone}`} aria-hidden="true"><Glyph name={k.icon} size={20} /></span>
                <span className="adm-min">
                  <span className="adm-row-title">{!i.read && <span className="adm-w-unread" aria-label="안 읽음" />}{i.title}</span>
                  <span className="adm-row-meta"><span>{k.label}</span><span>{i.summary}</span></span>
                </span>
                <span className="adm-row-end">
                  <PriorityChip value={i.priority} />
                  <small className={late ? 'is-late' : undefined}>{due || ago(i.at)}</small>
                </span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
