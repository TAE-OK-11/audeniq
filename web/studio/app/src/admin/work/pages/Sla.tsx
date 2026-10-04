// SLA · 마감 — 업무 유형별 제한 시간과 Due Soon / Overdue, 담당자 미처리 시간, 자동 escalation, 긴급도 자동 상승.
import { useState } from 'react';
import { Link, useSearchParams } from '../../../lib/router';
import { useToast } from '../../../components/Toast';
import { errorMessage } from '../../../api/errors';
import { useAsync } from '../../../hooks/useAsync';
import { Glyph } from '../../../components/Glyph';
import { Empty, ErrorBox, Filters, PageHead, Section, Seg, Skeleton, useStaff } from '../../ui';
import { ROLE_LABEL } from '../../labels';
import { workApi } from '../api';
import { TASK_TYPE } from '../labels';
import { durationLabel } from '../sla';
import { MiniStats, Person, PriorityChip, SlaChip, SwitchRow, dueOf, usePeople } from '../ui';
import type { SlaPolicy, WorkTask } from '../types';

type Unit = 'min' | 'hour' | 'bday';
const UNIT_MIN: Record<Unit, number> = { min: 1, hour: 60, bday: 540 };
const splitTarget = (p: SlaPolicy): [number, Unit] =>
  p.business_hours && p.target_minutes % 540 === 0 ? [p.target_minutes / 540, 'bday'] : p.target_minutes % 60 === 0 ? [p.target_minutes / 60, 'hour'] : [p.target_minutes, 'min'];

function PolicyCard({ policy, onSaved }: { policy: SlaPolicy; onSaved: () => void }) {
  const toast = useToast();
  const { me } = useStaff();
  const [p, setP] = useState(policy);
  const [[amount, unit], setTarget] = useState(splitTarget(policy));
  const [busy, setBusy] = useState(false);
  const target = Math.round(amount * UNIT_MIN[unit]);
  const dirty = JSON.stringify({ ...p, target_minutes: target }) !== JSON.stringify(policy);
  const admin = me.role === 'ADMIN';

  const save = async () => {
    if (busy || target < 15) return;
    setBusy(true);
    try {
      await workApi.saveSla({ ...p, target_minutes: target, business_hours: unit === 'bday' ? true : p.business_hours });
      toast(`${TASK_TYPE[p.type]} SLA를 저장했어요. 진행 중인 업무 기한도 다시 계산했어요.`, 'success');
      onSaved();
    } catch (err) {
      toast(errorMessage(err, 'SLA를 저장하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={`adm-card adm-w-policy${p.enabled ? '' : ' is-off'}`}>
      <div className="adm-check-top">
        <h3>{TASK_TYPE[p.type]}</h3>
        <span className="adm-w-policy-target">{p.enabled ? durationLabel(target, unit === 'bday' || p.business_hours) : '꺼짐'}</span>
      </div>
      <fieldset disabled={!admin} className="adm-w-policy-body">
        <div className="adm-w-inline-row">
          <span className="adm-field-label">제한 시간</span>
          <span className="adm-w-inline">
            <input type="number" className="adm-input" min={1} max={999} value={amount} aria-label="제한 시간" onChange={e => setTarget([Math.max(1, Number(e.target.value) || 1), unit])} />
            <select className="adm-select" aria-label="단위" value={unit} onChange={e => setTarget([amount, e.target.value as Unit])}>
              <option value="min">분</option><option value="hour">시간</option><option value="bday">영업일</option>
            </select>
          </span>
        </div>
        <div className="adm-w-inline-row">
          <span className="adm-field-label">Due Soon</span>
          <Seg label="기한 임박 기준" value={String(p.warn_ratio)} onChange={v => setP({ ...p, warn_ratio: Number(v) })} options={[['0.1', '10%'], ['0.25', '25%'], ['0.5', '50%']] as const} />
        </div>
        <div className="adm-w-inline-row">
          <span className="adm-field-label">미처리 알림</span>
          <span className="adm-w-inline">
            <input type="number" className="adm-input" min={10} max={10000} step={10} value={p.idle_alert_minutes} aria-label="미처리 알림(분)" onChange={e => setP({ ...p, idle_alert_minutes: Math.max(10, Number(e.target.value) || 10) })} />
            <span>분 동안 시작 안 하면</span>
          </span>
        </div>
        <div className="adm-w-inline-row">
          <span className="adm-field-label">Escalation</span>
          <select className="adm-select" aria-label="escalation 대상" value={p.escalate_to} onChange={e => setP({ ...p, escalate_to: e.target.value as SlaPolicy['escalate_to'] })}>
            {(['ADMIN', 'REVIEWER', 'OPERATOR', 'SUPPORT'] as const).map(r => <option key={r} value={r}>{ROLE_LABEL[r]}에게</option>)}
          </select>
        </div>
        <SwitchRow title="영업시간만 계산" hint="평일 09–18시 (한국 시간)" checked={unit === 'bday' || p.business_hours} disabled={unit === 'bday'} onChange={v => setP({ ...p, business_hours: v })} />
        <SwitchRow title="긴급도 자동 상승" hint="임박하면 한 단계, 초과하면 긴급" checked={p.auto_raise_priority} onChange={v => setP({ ...p, auto_raise_priority: v })} />
        <SwitchRow title="SLA 사용" checked={p.enabled} onChange={v => setP({ ...p, enabled: v })} />
      </fieldset>
      {admin && dirty && (
        <div className="adm-form-actions">
          <button type="button" className="adm-btn soft small" onClick={() => { setP(policy); setTarget(splitTarget(policy)); }}>되돌리기</button>
          <button type="button" className="adm-btn primary small" disabled={busy || target < 15} onClick={save}>{busy ? '저장 중…' : '저장'}</button>
        </div>
      )}
    </div>
  );
}

function DueRow({ t, people, idle }: { t: WorkTask; people: ReturnType<typeof usePeople>; idle?: boolean }) {
  const idleMin = t.assigned_at ? Math.round((Date.now() - Date.parse(t.assigned_at)) / 60000) : 0;
  return (
    <Link to={`/admin/tasks/${t.id}`} className="adm-row adm-w-task-row">
      <Person id={t.assignee_id} people={people} compact />
      <span className="adm-min">
        <span className="adm-row-title"><span className="adm-w-key">{t.key}</span>{t.title}</span>
        <span className="adm-row-meta">
          <span>{TASK_TYPE[t.type]}</span>
          <span>{people.get(t.assignee_id)?.name ?? '담당자 없음'}</span>
          {idle && <span className="is-late-text">맡은 지 {idleMin >= 1440 ? `${Math.round(idleMin / 1440)}일` : `${Math.round(idleMin / 60)}시간`} · 시작 전</span>}
          {t.escalation_level >= 2 && <span className="is-late-text">책임자 escalation</span>}
        </span>
      </span>
      <span className="adm-row-end">
        <PriorityChip value={t.priority} raisedFrom={t.priority_raised_from} />
        <SlaChip state={t.sla_state} due={dueOf(t)} short />
      </span>
    </Link>
  );
}

export function Sla() {
  const [params, setParams] = useSearchParams();
  const tab = (params.get('tab') as 'board' | 'policy') || 'board';
  const people = usePeople();
  const tasks = useAsync(() => workApi.tasks({ view: 'all', status: 'open' }), []);
  const policies = useAsync(() => workApi.sla(), []);
  const all = tasks.data?.items ?? [];
  const pol = new Map((policies.data?.items ?? []).map(p => [p.type, p]));
  const overdue = all.filter(t => t.sla_state === 'OVERDUE');
  const soon = all.filter(t => t.sla_state === 'DUE_SOON');
  const idle = all.filter(t => t.status === 'TODO' && t.assigned_at && Date.now() - Date.parse(t.assigned_at) > (pol.get(t.type)?.idle_alert_minutes ?? 240) * 60000);
  const raised = all.filter(t => t.priority_raised_from);
  const byDue = (a: WorkTask, b: WorkTask) => String(dueOf(a)).localeCompare(String(dueOf(b)));

  return (
    <div className="view-enter">
      <PageHead
        eyebrow="운영"
        title="SLA · 마감"
        sub="업무 유형마다 처리 제한 시간을 두고, 기한이 가까워지면 우선순위를 올리고 책임자에게 자동으로 알려요."
        actions={<button type="button" className="adm-btn soft small" onClick={() => { tasks.reload(); policies.reload(); }}>새로고침</button>}
      />
      <Filters label="SLA 화면" value={tab} onChange={v => setParams({ tab: v }, { replace: true })} options={[{ value: 'board', label: '기한 현황' }, { value: 'policy', label: '유형별 SLA' }]} />
      {tab === 'board' ? (
        <>
          <MiniStats items={[
            { label: 'SLA 초과', value: overdue.length, tone: overdue.length ? 'alert' : 'muted', hint: '책임자에게 escalation' },
            { label: '기한 임박', value: soon.length, tone: soon.length ? 'warn' : 'muted', hint: '우선순위 자동 상승' },
            { label: '미처리', value: idle.length, tone: idle.length ? 'warn' : 'muted', hint: '맡았지만 시작 전' },
            { label: '자동 상승', value: raised.length, hint: '기한 때문에 올라간 우선순위' },
          ]} />
          {tasks.error && <ErrorBox message={tasks.error} onRetry={tasks.reload} />}
          {tasks.loading && !tasks.data ? <Skeleton rows={4} /> : overdue.length + soon.length + idle.length === 0 ? (
            <Empty icon={<Glyph name="clock" size={22} />} title="기한을 넘기거나 임박한 업무가 없어요">모든 업무가 SLA 안에서 처리되고 있어요.</Empty>
          ) : (
            <>
              {overdue.length > 0 && <Section title="SLA 초과" meta={`${overdue.length}건 · 지난 순`}><div className="adm-list">{[...overdue].sort(byDue).map(t => <DueRow key={t.id} t={t} people={people} />)}</div></Section>}
              {soon.length > 0 && <Section title="기한 임박" meta={`${soon.length}건 · 남은 시간 순`}><div className="adm-list">{[...soon].sort(byDue).map(t => <DueRow key={t.id} t={t} people={people} />)}</div></Section>}
              {idle.length > 0 && <Section title="담당자 미처리" meta="맡은 뒤 미처리 알림 시간이 지난 업무"><div className="adm-list">{idle.map(t => <DueRow key={t.id} t={t} people={people} idle />)}</div></Section>}
            </>
          )}
        </>
      ) : (
        <>
          <div className="adm-alert">예: <b>긴급 송출 2시간</b>, <b>권리 검토 1영업일</b>. 바꾸면 진행 중인 업무의 SLA 기한도 다시 계산해요.</div>
          {policies.error && <ErrorBox message={policies.error} onRetry={policies.reload} />}
          {policies.loading && !policies.data ? <Skeleton rows={3} /> : (
            <div className="adm-w-policy-grid">
              {(policies.data?.items ?? []).map(p => <PolicyCard key={`${p.type}-${p.target_minutes}-${p.enabled}-${p.warn_ratio}`} policy={p} onSaved={() => { policies.reload(); tasks.reload(); }} />)}
            </div>
          )}
        </>
      )}
    </div>
  );
}
