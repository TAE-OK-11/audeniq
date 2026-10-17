// 예외 처리 — QC 실패, 메타데이터 충돌, 권리 서류 누락, DSP 반려, 배급 실패, 정산 불일치, 자동화 실패.
// 항목은 원천(검사 결과·배급 준비·전송 작업·대조 건·정산 리포트·자동화 작업)에서 읽고, 원천이 정상이 되면 저절로 사라진다.
// 여기서는 담당자·확인·해결/무시·재시도(배급 재검사)·업무로 전환만 기록한다. 권한은 원천의 담당 역할을 따른다.
import { useEffect, useState } from 'react';
import { Link, useSearchParams } from '../../../lib/router';
import { Modal, useModalClose } from '../../../components/Modal';
import { useToast } from '../../../components/Toast';
import { errorMessage } from '../../../api/errors';
import { useAsync } from '../../../hooks/useAsync';
import { Glyph } from '../../../components/Glyph';
import { Badge, EmptyState, ErrorBox, ChipGroup, PageHeader, Seg, SkeletonList, StatusBadge, useStaff, WorkTabs } from '../../ui';
import { ago } from '../../labels';
import { workApi } from '../api';
import {
  ENTITY_KIND, EXCEPTION_KIND, EXCEPTION_KINDS, EXCEPTION_SOURCE, SEVERITIES, SEVERITY, SLA_STATE, TRIAGE_STATUS, entityLink, remaining,
} from '../labels';
import { EntityPicker, MiniStats, Person, PersonSelect, usePeople, type People } from '../ui';
import { ADMIN_WORK_EVENT } from '../../liveEvents';
import type { EntityRef, ExceptionAction, ExceptionItem, ExceptionKind, ExceptionQuery, Severity } from '../types';

const VIEWS = [['open', '처리 대기'], ['mine', '내 담당'], ['unassigned', '담당자 없음'], ['resolved', '처리 완료']] as const;
type View = typeof VIEWS[number][0];

function Resolve({ item, ignore, onDone }: { item: ExceptionItem; ignore: boolean; onDone: (i: ExceptionItem | null) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy || (ignore && !note.trim())) return;
    setBusy(true);
    try {
      const r = await workApi.actException(item.source, item.source_id, { action: ignore ? 'IGNORE' : 'RESOLVE', resolution: note.trim() || undefined });
      toast(ignore ? '무시로 처리했어요. 원천이 다시 실패하면 다시 나타나요.' : '해결로 처리했어요.', 'success');
      onDone(r.item);
      close();
    } catch (err) {
      toast(errorMessage(err, '처리하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <form onSubmit={submit}>
      <div className="ui-field">
        <label htmlFor="exNote">{ignore ? '무시하는 이유' : <>처리 내용<span className="aq-optional">선택</span></>} {ignore && <span className="required">*</span>}</label>
        <textarea id="exNote" data-autofocus className="ui-textarea" rows={4} maxLength={1000} value={note} onChange={e => setNote(e.target.value)}
          placeholder={ignore ? '예: 테스트 패키지라 전송하지 않아요.' : '예: 메타데이터를 고친 뒤 재검사해서 통과했어요.'} />
      </div>
      <div className="adm-form-actions">
        <button type="button" className="ui-btn is-soft" onClick={close}>닫기</button>
        <button type="submit" className="ui-btn is-primary" disabled={busy || (ignore && !note.trim())}>{busy ? '처리 중…' : ignore ? '무시' : '해결'}</button>
      </div>
    </form>
  );
}

function Report({ onDone }: { onDone: () => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [kind, setKind] = useState<ExceptionKind>('SETTLEMENT_MISMATCH');
  const [severity, setSeverity] = useState<Severity>('MEDIUM');
  const [title, setTitle] = useState('');
  const [detail, setDetail] = useState('');
  const [entity, setEntity] = useState<EntityRef | null>(null);
  const [busy, setBusy] = useState(false);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy || !title.trim()) return;
    setBusy(true);
    try {
      await workApi.reportException({ kind, severity, title: title.trim(), detail: detail.trim(), entity_kind: entity?.kind ?? null, entity_id: entity?.id ?? null });
      toast('예외를 등록했어요.', 'success');
      onDone();
      close();
    } catch (err) {
      toast(errorMessage(err, '등록하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <form onSubmit={submit} className="adm-w-form">
      <p className="small muted" style={{ marginTop: 0 }}>자동으로 잡히지 않는 문제(정산 불일치 등)만 직접 등록해요. 검사·배급·자동화 실패는 원천에서 자동으로 올라와요.</p>
      <div className="adm-w-grid2">
        <div className="ui-field">
          <label htmlFor="rpKind">종류</label>
          <select id="rpKind" className="ui-select" value={kind} onChange={e => setKind(e.target.value as ExceptionKind)}>
            {EXCEPTION_KINDS.map(k => <option key={k} value={k}>{EXCEPTION_KIND[k].label}</option>)}
          </select>
        </div>
        <div className="ui-field">
          <label htmlFor="rpSev">심각도</label>
          <select id="rpSev" className="ui-select" value={severity} onChange={e => setSeverity(e.target.value as Severity)}>
            {SEVERITIES.map(s => <option key={s} value={s}>{SEVERITY[s][0]}</option>)}
          </select>
        </div>
      </div>
      <div className="ui-field">
        <label htmlFor="rpTitle">제목 <span className="required">*</span></label>
        <input id="rpTitle" data-autofocus className="ui-input" maxLength={300} value={title} onChange={e => setTitle(e.target.value)} placeholder="예: D-6 9월 리포트 합계가 원장과 12,400원 달라요" />
      </div>
      <div className="ui-field">
        <label htmlFor="rpDetail">내용</label>
        <textarea id="rpDetail" className="ui-textarea" rows={4} maxLength={4000} value={detail} onChange={e => setDetail(e.target.value)} />
      </div>
      <div className="ui-field">
        <span className="ui-field-label">연결 대상<span className="aq-optional">선택</span></span>
        {entity ? (
          <div className="adm-w-related">
            <span className="adm-w-ref"><span><span className="adm-w-related-kind">{ENTITY_KIND[entity.kind].label}</span>{entity.label ?? entity.id}</span>
              <button type="button" aria-label="연결 해제" onClick={() => setEntity(null)}><Glyph name="close" size={12} /></button></span>
          </div>
        ) : <EntityPicker onPick={setEntity} kinds={['release', 'dsp', 'settlement_report', 'payout', 'document', 'org', 'artist', 'label', 'inquiry']} />}
      </div>
      <div className="adm-form-actions">
        <button type="button" className="ui-btn is-soft" onClick={close}>닫기</button>
        <button type="submit" className="ui-btn is-primary" disabled={busy || !title.trim()}>{busy ? '등록 중…' : '등록'}</button>
      </div>
    </form>
  );
}

function Card({ item, people, focus, onChanged }: { item: ExceptionItem; people: People; focus: boolean; onChanged: (i: ExceptionItem | null) => void }) {
  const toast = useToast();
  const [busy, setBusy] = useState<ExceptionAction | null>(null);
  const [resolving, setResolving] = useState<null | 'resolve' | 'ignore'>(null);
  const t = item.triage;
  const kind = item.kind ? EXCEPTION_KIND[item.kind] : null;
  const act = async (action: ExceptionAction, extra: { assignee_id?: string | null } = {}) => {
    setBusy(action);
    try {
      const r = await workApi.actException(item.source, item.source_id, { action, ...extra });
      toast(({
        ACK: '확인했어요. 담당자로 지정됐어요.', ASSIGN: '담당자를 바꿨어요.', REOPEN: '다시 열었어요.',
        RETRY: '배급 재검사를 요청했어요. 결과가 나오면 이 목록이 바뀌어요.', TO_TASK: '업무로 만들었어요.',
      } as Record<string, string>)[action] ?? '처리했어요.', 'success');
      onChanged(r.item);
    } catch (e) {
      toast(errorMessage(e, '처리하지 못했어요.'), 'error');
    } finally {
      setBusy(null);
    }
  };
  const link = item.entity ? entityLink(item.entity.kind, item.entity.id) : null;
  const closed = item.closed || t?.status === 'RESOLVED' || t?.status === 'IGNORED';
  return (
    <article id={`ex-${item.id}`} className={`ui-card adm-w-exc${focus ? ' is-focus' : ''}${closed ? ' is-closed' : ''}`}>
      <div className="adm-w-exc-top">
        <span className={`adm-w-inbox-icon adm-t-${item.severity ? SEVERITY[item.severity][1] : 'gray'}`} aria-hidden="true"><Glyph name={kind?.icon ?? 'alert'} size={16} /></span>
        <span className="adm-min">
          <b className="adm-w-exc-title">{item.title ?? item.source_id}</b>
          <span className="ui-row-meta">
            {kind && <span>{kind.label}</span>}
            <span>{EXCEPTION_SOURCE[item.source]}</span>
            {item.org_name && <span>{item.org_name}</span>}
            {item.dsp && <span>{item.dsp_name ?? item.dsp}</span>}
            {item.detected_at && <span>{ago(item.detected_at)}</span>}
          </span>
        </span>
        <span className="adm-codes">
          {item.severity && <StatusBadge value={SEVERITY[item.severity]} />}
          {t?.status && <StatusBadge value={TRIAGE_STATUS[t.status]} />}
          {item.sla && !closed && <Badge tone={SLA_STATE[item.sla.state][1]}><Glyph name="clock" size={12} />{remaining(item.sla.due_at)}</Badge>}
        </span>
      </div>
      {item.detail && <p className="adm-w-exc-detail">{item.detail}</p>}
      <div className="adm-w-exc-meta">
        {(!item.can_act || closed) && <Person id={t?.assignee_id} people={people} />}
        {t && t.retry_count > 0 && <span className="small muted">재시도 {t.retry_count}회{t.last_retry_at ? ` · ${ago(t.last_retry_at)}` : ''}</span>}
        {t?.resolution && <span className="small muted">“{t.resolution}”</span>}
        {link && <Link to={link} className="ui-btn is-soft is-sm">{ENTITY_KIND[item.entity!.kind].label} 열기</Link>}
        {t?.task_id && <Link to={`/admin/tasks/${t.task_id}`} className="ui-btn is-soft is-sm"><Glyph name="task" size={14} />연결된 업무</Link>}
      </div>
      {item.can_act === false ? (
        <p className="small muted adm-w-exc-note">이 예외는 담당 역할만 처리할 수 있어요.</p>
      ) : item.can_act && (
        <div className="adm-w-exc-actions">
          {closed ? (
            <button type="button" className="ui-btn is-soft is-sm" disabled={!!busy} onClick={() => act('REOPEN')}>다시 열기</button>
          ) : <>
            {t?.status !== 'ACKNOWLEDGED' && <button type="button" className="ui-btn is-primary is-sm" disabled={!!busy} onClick={() => act('ACK')}>{busy === 'ACK' ? '처리 중…' : '확인·맡기'}</button>}
            <PersonSelect id={`exA-${item.id}`} label="담당자" value={t?.assignee_id ?? null} people={people} empty="담당자 없음"
              onChange={v => void act('ASSIGN', { assignee_id: v })} />
            {item.retryable && <button type="button" className="ui-btn is-soft is-sm" disabled={!!busy} onClick={() => act('RETRY')}><Glyph name="repeat" size={14} />{busy === 'RETRY' ? '요청 중…' : '배급 재검사'}</button>}
            {!t?.task_id && <button type="button" className="ui-btn is-soft is-sm" disabled={!!busy} onClick={() => act('TO_TASK')}><Glyph name="task" size={14} />업무로 만들기</button>}
            <button type="button" className="ui-btn is-soft is-sm" disabled={!!busy} onClick={() => setResolving('resolve')}>해결</button>
            <button type="button" className="ui-btn is-soft is-sm" disabled={!!busy} onClick={() => setResolving('ignore')}>무시</button>
          </>}
        </div>
      )}
      {resolving && (
        <Modal title={resolving === 'ignore' ? '예외 무시' : '예외 해결'} onClose={() => setResolving(null)}>
          <Resolve item={item} ignore={resolving === 'ignore'} onDone={onChanged} />
        </Modal>
      )}
    </article>
  );
}

export function Exceptions() {
  const { refreshCounts } = useStaff();
  const people = usePeople();
  const [params, setParams] = useSearchParams();
  const focus = params.get('focus') ?? '';
  const view = (params.get('view') as View) || 'open';
  const [kind, setKind] = useState<ExceptionKind | ''>('');
  const [severity, setSeverity] = useState<Severity | ''>('');
  const [q, setQ] = useState('');
  const [reporting, setReporting] = useState(false);
  const query: ExceptionQuery = { view, kind, severity, q: q.trim() || undefined };
  const { data, loading, error, reload } = useAsync(() => workApi.exceptions(query), [view, kind, severity, q]); // eslint-disable-line react-hooks/exhaustive-deps
  const [items, setItems] = useState<ExceptionItem[]>([]);
  useEffect(() => { setItems(data?.items ?? []); }, [data]);
  useEffect(() => {
    const on = () => reload();
    window.addEventListener(ADMIN_WORK_EVENT, on);
    return () => window.removeEventListener(ADMIN_WORK_EVENT, on);
  }, [reload]);
  useEffect(() => {
    if (!focus || !items.length) return;
    document.getElementById(`ex-${focus}`)?.scrollIntoView({ block: 'center', behavior: 'smooth' });
  }, [focus, items.length]);

  const changed = (id: string) => (next: ExceptionItem | null) => {
    refreshCounts();
    if (!next || ((view === 'open' || view === 'mine' || view === 'unassigned') && next.closed)) {
      setItems(xs => xs.filter(x => x.id !== id));
      return;
    }
    setItems(xs => xs.map(x => (x.id === id ? next : x)));
  };
  const counts = data?.counts ?? {};
  const set = (k: string, v: string) => { const next = new URLSearchParams(params); next.set(k, v); next.delete('focus'); setParams(next, { replace: true }); };
  const critical = items.filter(i => i.severity === 'CRITICAL').length;
  const breached = items.filter(i => i.sla?.state === 'BREACHED' && !i.closed).length;

  return (
    <div className="view-enter">
      <PageHeader
        eyebrow="업무"
        title="예외 처리"
        sub="자동 처리에 실패한 건을 원천에서 모아 보여 줘요. 원천이 정상으로 돌아오면 목록에서 저절로 사라져요."
        actions={<>
          <button type="button" className="ui-btn is-soft is-sm" onClick={reload}>새로고침</button>
          <button type="button" className="ui-btn is-primary is-sm" onClick={() => setReporting(true)}><Glyph name="plus" size={14} />직접 등록</button>
        </>}
      />
      <WorkTabs />
      {view !== 'resolved' && (
        <MiniStats items={[
          { label: '처리 대기', value: data?.total ?? '–' },
          { label: '내 담당', value: data?.mine ?? '–', onClick: () => set('view', 'mine'), active: view === 'mine' },
          { label: '치명', value: critical, tone: critical ? 'alert' : 'muted' },
          { label: 'SLA 초과', value: breached, tone: breached ? 'alert' : 'muted' },
        ]} />
      )}
      <Seg label="보기" value={view} onChange={v => set('view', v)} options={VIEWS} />
      {view !== 'resolved' && (
        <ChipGroup label="종류" value={kind} onChange={setKind}
          options={[{ value: '' as ExceptionKind | '', label: '전체' }, ...EXCEPTION_KINDS.map(k => ({ value: k as ExceptionKind | '', label: EXCEPTION_KIND[k].label, count: counts[k] ?? 0 }))]} />
      )}
      <div className="adm-queue-tools adm-w-filter-row">
        <label htmlFor="exSearch" className="sr-only">검색</label>
        <input id="exSearch" type="search" className="ui-input" placeholder="제목·내용·작업 공간으로 찾기" value={q} onChange={e => setQ(e.target.value)} />
        <select className="ui-select is-auto" aria-label="심각도" value={severity} onChange={e => setSeverity(e.target.value as Severity | '')}>
          <option value="">모든 심각도</option>
          {SEVERITIES.map(s => <option key={s} value={s}>{SEVERITY[s][0]}</option>)}
        </select>
      </div>
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <SkeletonList rows={5} /> : items.length === 0 ? (
        <EmptyState title={view === 'resolved' ? '처리한 예외가 없어요' : '처리할 예외가 없어요'}>
          {view === 'resolved' ? '해결·무시한 예외가 여기에 남아요.' : '검사·배급·정산·자동화가 모두 정상이에요.'}
        </EmptyState>
      ) : (
        <div className="adm-w-exc-list">
          {items.map(i => <Card key={i.id} item={i} people={people} focus={focus === i.id} onChanged={changed(i.id)} />)}
        </div>
      )}
      {reporting && (
        <Modal title="예외 직접 등록" onClose={() => setReporting(false)} dismissible={false} modalClass="adm-w-modal">
          <Report onDone={reload} />
        </Modal>
      )}
    </div>
  );
}
