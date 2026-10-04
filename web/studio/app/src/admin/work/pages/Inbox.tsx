// 업무함 — 나에게 온 일을 한곳에서: 배정된 업무, 검토·승인 요청, 멘션, 자동화 실패, SLA 임박·초과,
// 문의 지연, 권리 문제, 배급 실패. 항목은 원천(업무·심사·문의·예외)에서 계산되고 읽음 표시만 저장된다.
import { useEffect, useState } from 'react';
import { useNavigate, useSearchParams } from '../../../lib/router';
import { useToast } from '../../../components/Toast';
import { errorMessage } from '../../../api/errors';
import { useAsync } from '../../../hooks/useAsync';
import { Glyph } from '../../../components/Glyph';
import { Chip, Empty, ErrorBox, Filters, PageHead, Seg, Skeleton, useStaff, WorkTabs } from '../../ui';
import { ago } from '../../labels';
import { workApi } from '../api';
import { ENTITY_KIND, EXCEPTION_KIND, INBOX_CATEGORIES, INBOX_CATEGORY, INBOX_FLAG, PRIORITIES, PRIORITY, QUEUE_TYPE, WORK_TYPE } from '../labels';
import { DueChip, MiniStats, PriorityChip } from '../ui';
import { ADMIN_WORK_EVENT, ADMIN_INQUIRIES_EVENT } from '../../liveEvents';
import type { EntityKind, InboxCategory, InboxItem, InboxQuery, Priority } from '../types';

const typeLabel = (t: string) =>
  (WORK_TYPE as Record<string, string>)[t] ?? (QUEUE_TYPE as Record<string, string>)[t]
  ?? (EXCEPTION_KIND as Record<string, { label: string }>)[t]?.label ?? (t === 'NOTE' ? '메모' : t);

function Row({ i, onOpen }: { i: InboxItem; onOpen: (i: InboxItem) => void }) {
  const c = INBOX_CATEGORY[i.category];
  return (
    <button type="button" className={`adm-row adm-w-inbox-row${i.read ? ' is-read' : ''}`} onClick={() => onOpen(i)}>
      <span className={`adm-w-inbox-icon adm-t-${c.tone}`} aria-hidden="true"><Glyph name={c.icon} size={16} /></span>
      <span className="adm-min">
        <span className="adm-row-title">
          {!i.read && <span className="adm-w-unread-dot" aria-label="읽지 않음" />}
          {i.title}
        </span>
        <span className="adm-row-meta">
          <span>{c.label}</span>
          <span>{typeLabel(i.type)}</span>
          {i.subtitle && <span className="adm-w-ellipsis">{i.subtitle}</span>}
          {i.entity && i.entity.kind !== 'task' && <span>{ENTITY_KIND[i.entity.kind]?.label}</span>}
          <span>{ago(i.at)}</span>
        </span>
      </span>
      <span className="adm-row-end">
        <span className="adm-codes">
          {i.flags.map(f => <Chip key={f} tone={f === 'ESCALATED' ? 'red' : 'amber'}>{INBOX_FLAG[f] ?? f}</Chip>)}
          <PriorityChip value={i.priority} />
        </span>
        <DueChip due={i.due_at} />
      </span>
    </button>
  );
}

export function Inbox() {
  const nav = useNavigate();
  const toast = useToast();
  const { refreshCounts } = useStaff();
  const [params, setParams] = useSearchParams();
  const category = (params.get('category') as InboxCategory | null) ?? '';
  const [unread, setUnread] = useState<'all' | 'unread'>('all');
  const [priority, setPriority] = useState<Priority | ''>('');
  const [due, setDue] = useState<NonNullable<InboxQuery['due']>>('');
  const [entityKind, setEntityKind] = useState<EntityKind | ''>('');
  const { data, loading, error, reload } = useAsync(
    () => workApi.inbox({ category, priority, due, entity_kind: entityKind, unread: unread === 'unread' }),
    [category, priority, due, entityKind, unread],
  );
  // 업무·문의가 바뀌면(실시간) 바로 다시 받는다
  useEffect(() => {
    const on = () => reload();
    window.addEventListener(ADMIN_WORK_EVENT, on);
    window.addEventListener(ADMIN_INQUIRIES_EVENT, on);
    return () => { window.removeEventListener(ADMIN_WORK_EVENT, on); window.removeEventListener(ADMIN_INQUIRIES_EVENT, on); };
  }, [reload]);

  const items = data?.items ?? [];
  const counts = data?.counts ?? {};
  const setCategory = (v: InboxCategory | '') => {
    const next = new URLSearchParams(params);
    if (v) next.set('category', v); else next.delete('category');
    setParams(next, { replace: true });
  };
  const open = (i: InboxItem) => {
    if (!i.read) void workApi.markRead([i.id]).then(refreshCounts).catch(() => {});
    nav(i.link);
  };
  const readAll = async () => {
    const ids = items.filter(i => !i.read).map(i => i.id);
    if (!ids.length) return;
    try {
      await workApi.markRead(ids);
      toast(`${ids.length}건을 읽음으로 표시했어요.`, 'success');
      reload();
      refreshCounts();
    } catch (e) {
      toast(errorMessage(e, '읽음 표시를 하지 못했어요.'), 'error');
    }
  };
  const n = (c: InboxCategory) => counts[c]?.total ?? 0;
  const present = INBOX_CATEGORIES.filter(c => n(c) > 0);

  return (
    <div className="view-enter">
      <PageHead
        eyebrow="업무"
        title="업무함"
        sub="배정된 업무, 검토·승인 요청, 멘션, SLA 임박·초과, 자동화·배급 실패까지 나에게 온 일을 모아 보여 줘요."
        actions={<>
          <button type="button" className="adm-btn soft small" onClick={reload}>새로고침</button>
          <button type="button" className="adm-btn soft small" onClick={readAll} disabled={!items.some(i => !i.read)}>모두 읽음</button>
        </>}
      />
      <WorkTabs />
      <MiniStats items={[
        { label: '전체', value: data?.total ?? '–', hint: data ? `읽지 않음 ${data.unread}` : undefined, onClick: () => setCategory(''), active: !category },
        { label: '기한 초과', value: n('OVERDUE') + n('SUPPORT_ESCALATION'), tone: n('OVERDUE') + n('SUPPORT_ESCALATION') ? 'alert' : 'muted', onClick: () => setCategory('OVERDUE'), active: category === 'OVERDUE' },
        { label: 'SLA 임박', value: n('SLA_WARNING'), tone: n('SLA_WARNING') ? 'warn' : 'muted', onClick: () => setCategory('SLA_WARNING'), active: category === 'SLA_WARNING' },
        { label: '검토·승인 요청', value: n('REVIEW_REQUEST') + n('APPROVAL_REQUEST'), onClick: () => setCategory('REVIEW_REQUEST'), active: category === 'REVIEW_REQUEST' },
      ]} />
      {present.length > 0 && (
        <Filters label="분류" value={category} onChange={setCategory}
          options={[{ value: '' as InboxCategory | '', label: '전체', count: data?.total ?? 0 }, ...present.map(c => ({ value: c as InboxCategory | '', label: INBOX_CATEGORY[c].label, count: n(c) }))]} />
      )}
      <div className="adm-queue-tools adm-w-filter-row">
        <Seg label="읽음" value={unread} onChange={setUnread} options={[['all', '전체'], ['unread', '읽지 않음']] as const} />
        <select className="adm-select" aria-label="우선순위" value={priority} onChange={e => setPriority(e.target.value as Priority | '')}>
          <option value="">모든 우선순위</option>
          {PRIORITIES.map(p => <option key={p} value={p}>{PRIORITY[p][0]}</option>)}
        </select>
        <select className="adm-select" aria-label="기한" value={due} onChange={e => setDue(e.target.value as NonNullable<InboxQuery['due']>)}>
          <option value="">모든 기한</option>
          <option value="overdue">기한 지남</option>
          <option value="today">오늘까지</option>
          <option value="week">7일 안</option>
        </select>
        <select className="adm-select" aria-label="연결 대상" value={entityKind} onChange={e => setEntityKind(e.target.value as EntityKind | '')}>
          <option value="">모든 대상</option>
          {(['task', 'release', 'inquiry', 'document', 'dsp', 'settlement_report'] as EntityKind[]).map(k => <option key={k} value={k}>{ENTITY_KIND[k].label}</option>)}
        </select>
      </div>
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <Skeleton rows={6} /> : items.length === 0 ? (
        <Empty title={unread === 'unread' ? '읽지 않은 일이 없어요' : '지금 처리할 일이 없어요'}>
          새 업무·멘션·SLA 경고가 생기면 여기에 바로 나타나요.
        </Empty>
      ) : (
        <>
          <p className="adm-queue-count">{items.length}건</p>
          <div className="adm-list">{items.map(i => <Row key={i.id} i={i} onOpen={open} />)}</div>
        </>
      )}
    </div>
  );
}
