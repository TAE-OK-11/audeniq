// SLA · 마감 — 업무 유형과 기존 대기열(발매 심사·2차 승인·서류 검토·고객 문의·예외 처리)의 처리 기한.
// 기존 대기열의 기한은 원천의 시작 시각에서 계산하고(저장 안 함), 업무는 만들 때 정책으로 기한을 정한다.
// 정책·업무 캘린더·휴일은 관리자만 바꾼다(감사 기록). 기한이 지나면 우선순위를 올리고 지정 역할에 알린다.
import { useState } from 'react';
import { Link } from '../../../lib/router';
import { Modal, useModalClose } from '../../../components/Modal';
import { useToast } from '../../../components/Toast';
import { useConfirm } from '../../../components/Confirm';
import { errorMessage } from '../../../api/errors';
import { useAsync } from '../../../hooks/useAsync';
import { Glyph } from '../../../components/Glyph';
import { Badge, EmptyState, ErrorBox, PageHeader, Section, SkeletonList, StatusBadge, useStaff, WorkTabs } from '../../ui';
import { DSP_NAME } from '../../dspReqs';
import { workApi } from '../api';
import {
  QUEUE_TYPE, QUEUE_TYPES, ROLE_NAME, SLA_STATE, WEEKDAY, WORK_TYPE, WORK_TYPES, minutesLabel, remaining, slaTypeLabel,
} from '../labels';
import { MiniStats, Person, PriorityChip, SwitchRow, usePeople } from '../ui';
import type { SlaOverview, SlaPolicy, SlaPolicyInput, StaffRoleName, WorkCalendar } from '../types';

const ROLES: StaffRoleName[] = ['ADMIN', 'REVIEWER', 'OPERATOR', 'SUPPORT'];
const dayMinutes = (c?: WorkCalendar) => {
  if (!c) return undefined;
  const [a, b] = [c.work_start, c.work_end].map(t => { const [h, m] = t.split(':').map(Number); return h * 60 + m; });
  return b - a;
};

function PolicyForm({ init, data, onSaved }: { init: SlaPolicy | null; data: SlaOverview; onSaved: (d: SlaOverview) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [v, setV] = useState<SlaPolicyInput>(init ?? {
    type: 'GENERAL', scope_kind: null, scope_id: null, target_minutes: 540, calendar_id: data.calendars[0]?.id ?? null,
    warn_ratio: 0.25, idle_alert_minutes: 240, escalate_to: 'ADMIN', auto_raise_priority: true, enabled: true,
  });
  const [hours, setHours] = useState(String(Math.round((v.target_minutes / 60) * 100) / 100));
  const [idle, setIdle] = useState(String(Math.round((v.idle_alert_minutes / 60) * 100) / 100));
  const [busy, setBusy] = useState(false);
  const set = <K extends keyof SlaPolicyInput>(k: K, val: SlaPolicyInput[K]) => setV(x => ({ ...x, [k]: val }));
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    const target = Math.round(Number(hours) * 60);
    const idleMin = Math.round(Number(idle) * 60);
    if (!(target >= 15) || !(idleMin >= 10)) { toast('기한은 15분, 멈춤 알림은 10분 이상으로 정해 주세요.', 'error'); return; }
    setBusy(true);
    try {
      const d = await workApi.savePolicy({ ...v, id: init?.id, target_minutes: target, idle_alert_minutes: idleMin });
      toast('SLA 정책을 저장했어요.', 'success');
      onSaved(d);
      close();
    } catch (err) {
      toast(errorMessage(err, '저장하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <form onSubmit={submit} className="adm-w-form">
      <div className="adm-w-grid2">
        <div className="ui-field">
          <label htmlFor="spType">대상</label>
          <select id="spType" className="ui-select" value={v.type} onChange={e => set('type', e.target.value as SlaPolicyInput['type'])}>
            <optgroup label="기존 대기열">{QUEUE_TYPES.map(t => <option key={t} value={t}>{QUEUE_TYPE[t]}</option>)}</optgroup>
            <optgroup label="업무 유형">{WORK_TYPES.map(t => <option key={t} value={t}>{WORK_TYPE[t]}</option>)}</optgroup>
          </select>
        </div>
        <div className="ui-field">
          <label htmlFor="spScope">적용 범위</label>
          <select id="spScope" className="ui-select" value={v.scope_id ?? ''}
            onChange={e => setV(x => ({ ...x, scope_kind: e.target.value ? 'dsp' : null, scope_id: e.target.value || null }))}>
            <option value="">전체</option>
            {Object.entries(DSP_NAME).map(([code, name]) => <option key={code} value={code}>{name} 전용</option>)}
          </select>
        </div>
        <div className="ui-field">
          <label htmlFor="spHours">처리 기한 (시간)</label>
          <input id="spHours" className="ui-input" inputMode="decimal" value={hours} onChange={e => setHours(e.target.value)} />
        </div>
        <div className="ui-field">
          <label htmlFor="spCal">시간 계산</label>
          <select id="spCal" className="ui-select" value={v.calendar_id ?? ''} onChange={e => set('calendar_id', e.target.value || null)}>
            <option value="">24시간 (휴일 없이)</option>
            {data.calendars.map(c => <option key={c.id} value={c.id}>{c.name}</option>)}
          </select>
        </div>
        <div className="ui-field">
          <label htmlFor="spWarn">임박 경고</label>
          <select id="spWarn" className="ui-select" value={String(v.warn_ratio)} onChange={e => set('warn_ratio', Number(e.target.value))}>
            {[0.1, 0.2, 0.25, 0.33, 0.5].map(r => <option key={r} value={r}>남은 시간 {Math.round(r * 100)}% 이하</option>)}
          </select>
        </div>
        <div className="ui-field">
          <label htmlFor="spIdle">진행 없음 알림 (시간)</label>
          <input id="spIdle" className="ui-input" inputMode="decimal" value={idle} onChange={e => setIdle(e.target.value)} />
        </div>
        <div className="ui-field">
          <label htmlFor="spEsc">기한 초과 시 알릴 역할</label>
          <select id="spEsc" className="ui-select" value={v.escalate_to} onChange={e => set('escalate_to', e.target.value as StaffRoleName)}>
            {ROLES.map(r => <option key={r} value={r}>{ROLE_NAME[r]}</option>)}
          </select>
        </div>
      </div>
      <div className="adm-w-box">
        <SwitchRow title="기한을 넘기면 우선순위 한 단계 올리기" hint="한 번만 올리고, 처리 이력에 남아요." checked={v.auto_raise_priority} onChange={x => set('auto_raise_priority', x)} />
        <SwitchRow title="정책 사용" hint="끄면 이 대상에는 기한을 계산하지 않아요." checked={v.enabled} onChange={x => set('enabled', x)} />
      </div>
      <div className="adm-form-actions">
        <button type="button" className="ui-btn is-soft" onClick={close}>취소</button>
        <button type="submit" className="ui-btn is-primary" disabled={busy}>{busy ? '저장 중…' : '저장'}</button>
      </div>
    </form>
  );
}

function CalendarForm({ cal, onSaved }: { cal: WorkCalendar; onSaved: (d: SlaOverview) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [name, setName] = useState(cal.name);
  const [start, setStart] = useState(cal.work_start);
  const [end, setEnd] = useState(cal.work_end);
  const [days, setDays] = useState<number[]>(cal.workdays);
  const [busy, setBusy] = useState(false);
  const toggle = (d: number) => setDays(xs => (xs.includes(d) ? xs.filter(x => x !== d) : [...xs, d].sort()));
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      onSaved(await workApi.saveCalendar(cal.id, { name: name.trim(), work_start: start, work_end: end, workdays: days }));
      toast('업무 캘린더를 저장했어요.', 'success');
      close();
    } catch (err) {
      toast(errorMessage(err, '저장하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <form onSubmit={submit} className="adm-w-form">
      <div className="ui-field">
        <label htmlFor="cName">이름</label>
        <input id="cName" className="ui-input" maxLength={80} value={name} onChange={e => setName(e.target.value)} />
      </div>
      <div className="adm-w-grid2">
        <div className="ui-field"><label htmlFor="cStart">근무 시작</label><input id="cStart" type="time" className="ui-input" value={start} onChange={e => setStart(e.target.value)} /></div>
        <div className="ui-field"><label htmlFor="cEnd">근무 종료</label><input id="cEnd" type="time" className="ui-input" value={end} onChange={e => setEnd(e.target.value)} /></div>
      </div>
      <div className="ui-field">
        <span className="ui-field-label">근무일</span>
        <div className="ui-chips" role="group" aria-label="근무일">
          {[1, 2, 3, 4, 5, 6, 7].map(d => <button key={d} type="button" className="ui-chip" aria-pressed={days.includes(d)} onClick={() => toggle(d)}>{WEEKDAY[d]}</button>)}
        </div>
      </div>
      <div className="adm-form-actions">
        <button type="button" className="ui-btn is-soft" onClick={close}>취소</button>
        <button type="submit" className="ui-btn is-primary" disabled={busy || !days.length || !name.trim()}>{busy ? '저장 중…' : '저장'}</button>
      </div>
    </form>
  );
}

function Holidays({ cal, names, admin, onSaved }: { cal: WorkCalendar; names: Record<string, string>; admin: boolean; onSaved: (d: SlaOverview) => void }) {
  const toast = useToast();
  const [day, setDay] = useState('');
  const [name, setName] = useState('');
  const add = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!day || !name.trim()) return;
    try {
      onSaved(await workApi.setHoliday(cal.id, day, name.trim()));
      setDay(''); setName('');
      toast('휴일을 추가했어요.', 'success');
    } catch (err) { toast(errorMessage(err, '추가하지 못했어요.'), 'error'); }
  };
  const remove = async (d: string) => {
    try { onSaved(await workApi.setHoliday(cal.id, d)); toast('휴일을 뺐어요.', 'info'); } catch (err) { toast(errorMessage(err, '빼지 못했어요.'), 'error'); }
  };
  return (
    <div className="adm-w-holidays">
      {cal.holidays.length === 0 ? <p className="small muted">등록된 휴일이 없어요.</p> : (
        <div className="adm-w-related">
          {cal.holidays.map(d => (
            <span key={d} className="adm-w-ref"><span><b>{d.slice(5).replace('-', '.')}</b>&nbsp;{names[d] ?? ''}</span>
              {admin && <button type="button" aria-label={`${d} 휴일 빼기`} onClick={() => remove(d)}><Glyph name="close" size={12} /></button>}</span>
          ))}
        </div>
      )}
      {admin && (
        <form className="adm-w-inline" onSubmit={add}>
          <input type="date" className="ui-input" aria-label="휴일 날짜" value={day} onChange={e => setDay(e.target.value)} />
          <input className="ui-input" aria-label="휴일 이름" placeholder="예: 추석" maxLength={80} value={name} onChange={e => setName(e.target.value)} />
          <button type="submit" className="ui-btn is-soft is-sm" disabled={!day || !name.trim()}>추가</button>
        </form>
      )}
    </div>
  );
}

export function Sla() {
  const { me } = useStaff();
  const admin = me.role === 'ADMIN';
  const confirm = useConfirm();
  const toast = useToast();
  const people = usePeople();
  const { data: loaded, loading, error, reload } = useAsync(() => workApi.sla(), []);
  const [override, setOverride] = useState<SlaOverview | null>(null);
  const data = override ?? loaded;
  const [editing, setEditing] = useState<SlaPolicy | null | 'new'>(null);
  const [calEdit, setCalEdit] = useState<WorkCalendar | null>(null);
  const saved = (d: SlaOverview) => setOverride(d);

  if (error && !data) return <ErrorBox message={error} onRetry={reload} />;
  if (loading && !data) return <SkeletonList rows={6} />;
  if (!data) return null;
  const calOf = (id: string | null) => data.calendars.find(c => c.id === id);
  const breached = data.board.reduce((n, b) => n + b.breached, 0) + data.tasks.filter(t => t.state === 'BREACHED').length;
  const warning = data.board.reduce((n, b) => n + b.warning, 0) + data.tasks.filter(t => t.state === 'WARNING').length;
  const archive = async (p: SlaPolicy) => {
    if (!await confirm({ title: `${slaTypeLabel(p.type)} 정책을 보관할까요?`, message: '보관하면 이 대상에는 기한을 계산하지 않아요. 기록은 남아요.', confirmLabel: '보관', danger: true })) return;
    try { saved(await workApi.archivePolicy(p.id)); toast('정책을 보관했어요.', 'info'); } catch (e) { toast(errorMessage(e, '보관하지 못했어요.'), 'error'); }
  };

  return (
    <div className="view-enter">
      <PageHeader
        eyebrow="업무"
        title="SLA · 마감"
        sub="업무 유형과 기존 대기열마다 처리 기한을 정하고, 임박·초과 건을 한눈에 봐요. 영업일 기준이면 근무 시간과 휴일을 빼고 계산해요."
        actions={<>
          <button type="button" className="ui-btn is-soft is-sm" onClick={() => { setOverride(null); reload(); }}>새로고침</button>
          {admin && <button type="button" className="ui-btn is-primary is-sm" onClick={() => setEditing('new')}><Glyph name="plus" size={14} />정책 추가</button>}
        </>}
      />
      <WorkTabs />
      <MiniStats items={[
        { label: '기한 초과', value: breached, tone: breached ? 'alert' : 'muted' },
        { label: '임박', value: warning, tone: warning ? 'warn' : 'muted' },
        { label: '기한 있는 업무', value: data.tasks.length },
        { label: '정책', value: data.policies.filter(p => p.enabled).length, hint: `전체 ${data.policies.length}개` },
      ]} />

      <Section title="대기열 기한 현황" meta="원천의 시작 시각 기준">
        <div className="adm-w-sla-board">
          {data.board.map(b => (
            <div key={b.queue} className={`adm-w-stat${b.breached ? ' is-alert' : b.warning ? '' : ' is-muted'}`}>
              <strong>{b.open}</strong>
              <span>{QUEUE_TYPE[b.queue]}</span>
              <small>{b.breached ? `초과 ${b.breached}` : ''}{b.breached && b.warning ? ' · ' : ''}{b.warning ? `임박 ${b.warning}` : ''}{!b.breached && !b.warning ? '모두 여유' : ''}</small>
            </div>
          ))}
        </div>
        {data.queue.length > 0 && (
          <div className="ui-list" style={{ marginTop: 12 }}>
            {data.queue.slice(0, 30).map(q => (
              <Link key={`${q.queue}:${q.id}`} to={q.link} className="ui-row adm-w-task-row">
                <StatusBadge value={SLA_STATE[q.state]} />
                <span className="adm-min">
                  <span className="ui-row-title">{q.title}</span>
                  <span className="ui-row-meta"><span>{QUEUE_TYPE[q.queue]}</span>{q.org_name && <span>{q.org_name}</span>}<span>알림 {ROLE_NAME[q.escalate_to]}</span></span>
                </span>
                <span className="ui-row-end"><Badge tone={SLA_STATE[q.state][1]}><Glyph name="clock" size={12} />{remaining(q.due_at)}</Badge></span>
              </Link>
            ))}
          </div>
        )}
      </Section>

      <Section title="업무 기한" meta="마감일과 SLA 중 이른 쪽">
        {data.tasks.length === 0 ? <EmptyState title="기한이 있는 업무가 없어요" /> : (
          <div className="ui-list">
            {data.tasks.slice(0, 40).map(t => (
              <Link key={t.id} to={`/admin/tasks/${t.id}`} className="ui-row adm-w-task-row">
                <Person id={t.assignee_id} people={people} compact />
                <span className="adm-min">
                  <span className="ui-row-title"><span className="ui-tag is-mono">{t.key}</span>{t.title}</span>
                  <span className="ui-row-meta"><span>{WORK_TYPE[t.type]}</span>{t.escalation_level > 0 && <span className="is-late-text">상향 {t.escalation_level}단계</span>}</span>
                </span>
                <span className="ui-row-end">
                  <span className="adm-codes"><PriorityChip value={t.priority} />{t.state && <StatusBadge value={SLA_STATE[t.state]} />}</span>
                  {t.deadline && <span className="small muted">{remaining(t.deadline)}</span>}
                </span>
              </Link>
            ))}
          </div>
        )}
      </Section>

      <Section title="SLA 정책" meta={admin ? '관리자만 바꿀 수 있어요' : '보기 전용'}>
        <div className="ui-list">
          {data.policies.map(p => {
            const cal = calOf(p.calendar_id);
            return (
              <div key={p.id} className={`ui-row adm-w-policy${p.enabled ? '' : ' is-closed'}`}>
                <span className="adm-min">
                  <span className="ui-row-title">{slaTypeLabel(p.type)}{p.scope_id && <Badge tone="violet">{DSP_NAME[p.scope_id] ?? p.scope_id} 전용</Badge>}</span>
                  <span className="ui-row-meta">
                    <span>{minutesLabel(p.target_minutes, dayMinutes(cal))} 안에</span>
                    <span>{cal ? cal.name : '24시간 기준'}</span>
                    <span>멈춤 알림 {minutesLabel(p.idle_alert_minutes)}</span>
                    <span>초과 시 {ROLE_NAME[p.escalate_to]}{p.auto_raise_priority ? ' · 우선순위 올림' : ''}</span>
                  </span>
                </span>
                <span className="ui-row-end">
                  {!p.enabled && <Badge tone="gray">꺼짐</Badge>}
                  {admin && <span className="adm-w-btns">
                    <button type="button" className="ui-btn is-soft is-sm" onClick={() => setEditing(p)}>수정</button>
                    <button type="button" className="ui-btn is-soft is-sm" onClick={() => archive(p)}>보관</button>
                  </span>}
                </span>
              </div>
            );
          })}
        </div>
      </Section>

      {data.calendars.map(c => (
        <Section key={c.id} title={`업무 캘린더 · ${c.name}`} meta={`${c.workdays.map(d => WEEKDAY[d]).join('·')} ${c.work_start}–${c.work_end} (UTC${c.utc_offset_minutes >= 0 ? '+' : ''}${c.utc_offset_minutes / 60})`}
          action={admin ? <button type="button" className="ui-btn is-soft is-sm" onClick={() => setCalEdit(c)}>근무 시간 수정</button> : undefined}>
          <Holidays cal={c} names={data.holiday_names} admin={admin} onSaved={saved} />
        </Section>
      ))}

      {editing && (
        <Modal title={editing === 'new' ? 'SLA 정책 추가' : `${slaTypeLabel(editing.type)} 정책 수정`} onClose={() => setEditing(null)} dismissible={false} modalClass="adm-w-modal">
          <PolicyForm init={editing === 'new' ? null : editing} data={data} onSaved={saved} />
        </Modal>
      )}
      {calEdit && (
        <Modal title="업무 캘린더" onClose={() => setCalEdit(null)}>
          <CalendarForm cal={calEdit} onSaved={saved} />
        </Modal>
      )}
    </div>
  );
}
