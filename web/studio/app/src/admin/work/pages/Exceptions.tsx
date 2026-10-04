// 예외 처리 — 자동화에서 정상건은 사람이 보지 않고, 문제가 생긴 것만 여기로 모인다.
// QC 이상 · 권리 서류 부족 · DSP 반려 · 메타데이터 충돌 · 정산 불일치 · 전송 실패.
import { useEffect, useRef, useState } from 'react';
import { Link, useSearchParams } from '../../../lib/router';
import { Modal, useModalClose } from '../../../components/Modal';
import { useToast } from '../../../components/Toast';
import { errorMessage } from '../../../api/errors';
import { useAsync } from '../../../hooks/useAsync';
import { Glyph } from '../../../components/Glyph';
import { Chip, Empty, ErrorBox, Filters, PageHead, Seg, Skeleton, StatusChip, useStaff } from '../../ui';
import { ago, when } from '../../labels';
import { workApi } from '../api';
import { EXCEPTION_KIND, EXCEPTION_KINDS, EXCEPTION_STATUS, SEVERITY } from '../labels';
import { MiniStats, Person, PersonSelect, RelatedLinks, usePeople } from '../ui';
import type { ExceptionKind, WorkException } from '../types';

function CloseForm({ ex, ignore, onDone }: { ex: WorkException; ignore: boolean; onDone: () => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy || !text.trim()) return;
    setBusy(true);
    try {
      await workApi.actException(ex.id, { action: ignore ? 'IGNORE' : 'RESOLVE', resolution: text.trim() });
      toast(ignore ? '예외를 무시했어요. 같은 문제가 다시 생기면 다시 열려요.' : '해결로 표시했어요.', 'success');
      onDone();
      close();
    } catch (err) {
      toast(errorMessage(err, '처리하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <form onSubmit={submit}>
      <p className="small muted">{ex.title}</p>
      <div className="adm-field" style={{ marginTop: 14 }}>
        <label htmlFor="exRes">{ignore ? '무시하는 이유' : '어떻게 해결했나요'} <span className="required">*</span></label>
        <textarea id="exRes" data-autofocus className="adm-textarea" rows={3} maxLength={1000} value={text} onChange={e => setText(e.target.value)} placeholder={ignore ? '예: 의도된 무음 — 아티스트 확인' : '예: 메타데이터 수정 후 재전송 완료'} />
      </div>
      <div className="adm-form-actions">
        <button type="button" className="adm-btn soft" onClick={close}>취소</button>
        <button type="submit" className={`adm-btn ${ignore ? 'warn' : 'primary'}`} disabled={busy || !text.trim()}>{busy ? '처리 중…' : ignore ? '무시하기' : '해결로 표시'}</button>
      </div>
    </form>
  );
}

function ExceptionCard({ x, focus, onChanged }: { x: WorkException; focus: boolean; onChanged: () => void }) {
  const toast = useToast();
  const people = usePeople();
  const [closing, setClosing] = useState<null | 'resolve' | 'ignore'>(null);
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLElement>(null);
  useEffect(() => { if (focus) ref.current?.scrollIntoView({ block: 'center', behavior: 'smooth' }); }, [focus]);
  const k = EXCEPTION_KIND[x.kind];
  const active = x.status === 'OPEN' || x.status === 'ASSIGNED';

  const act = async (fn: () => Promise<unknown>, msg: string) => {
    if (busy) return;
    setBusy(true);
    try { await fn(); toast(msg, 'success'); onChanged(); } catch (err) { toast(errorMessage(err, '처리하지 못했어요.'), 'error'); } finally { setBusy(false); }
  };

  return (
    <article ref={ref} className={`adm-card adm-w-ex is-${x.severity.toLowerCase()}${focus ? ' is-focus' : ''}${active ? '' : ' is-closed'}`}>
      <div className="adm-check-top">
        <span className="adm-w-ex-title">
          <span className="adm-row-icon plain" aria-hidden="true"><Glyph name={k.icon} size={20} /></span>
          <span className="adm-min">
            <span className="adm-row-title">{x.title}</span>
            <span className="adm-row-meta"><span>{k.label}</span><span>{x.source}</span>{x.occurrences > 1 && <span>{x.occurrences}회 반복</span>}<span title={when(x.first_seen)}>처음 {ago(x.first_seen)}</span>{x.occurrences > 1 && <span>마지막 {ago(x.last_seen)}</span>}</span>
          </span>
        </span>
        <span className="adm-codes"><StatusChip value={SEVERITY[x.severity]} /><StatusChip value={EXCEPTION_STATUS[x.status]} /></span>
      </div>
      <p className="adm-w-desc" style={{ marginTop: 12 }}>{x.detail}</p>
      {x.related.length > 0 && <div style={{ marginTop: 10 }}><RelatedLinks items={x.related} /></div>}
      {x.resolution && <div className="adm-alert" style={{ margin: '12px 0 0' }}><b>{x.status === 'IGNORED' ? '무시' : '해결'}</b> · {x.resolution} <small className="muted">({ago(x.resolved_at)})</small></div>}
      <div className="adm-w-ex-foot">
        <span className="adm-w-ex-owner">
          {active ? (
            <PersonSelect id={`exa-${x.id}`} label="담당자" value={x.assignee_id} people={people} empty="담당자 지정" onChange={v => v && act(() => workApi.actException(x.id, { action: 'ASSIGN', assignee_id: v }), '담당자를 지정했어요.')} />
          ) : <Person id={x.assignee_id} people={people} />}
          {x.task_id && <Link to={`/admin/tasks/${x.task_id}`} className="adm-next-btn">연결된 업무<Glyph name="chevron-right" size={14} /></Link>}
        </span>
        <span className="adm-codes">
          {active ? <>
            {!x.task_id && <button type="button" className="adm-btn soft small" disabled={busy} onClick={() => act(() => workApi.actException(x.id, { action: 'TO_TASK' }), '업무로 만들었어요. 내 업무함에 들어가요.')}><Glyph name="task" size={14} />업무로 만들기</button>}
            <button type="button" className="adm-btn soft small" disabled={busy} onClick={() => setClosing('ignore')}>무시</button>
            <button type="button" className="adm-btn primary small" disabled={busy} onClick={() => setClosing('resolve')}>해결</button>
          </> : <button type="button" className="adm-btn soft small" disabled={busy} onClick={() => act(() => workApi.actException(x.id, { action: 'REOPEN' }), '다시 열었어요.')}>다시 열기</button>}
        </span>
      </div>
      {closing && (
        <Modal title={closing === 'ignore' ? '예외 무시' : '예외 해결'} onClose={() => setClosing(null)} dismissible={false}>
          <CloseForm ex={x} ignore={closing === 'ignore'} onDone={onChanged} />
        </Modal>
      )}
    </article>
  );
}

export function Exceptions() {
  const [params, setParams] = useSearchParams();
  const focusId = params.get('id');
  const kind = (params.get('kind') as ExceptionKind | null) ?? '';
  const [scope, setScope] = useState<'active' | 'RESOLVED' | 'IGNORED'>('active');
  const { refreshWork } = useStaff();
  const { data, loading, error, reload } = useAsync(() => workApi.exceptions('', scope), [scope]);
  const all = data?.items ?? [];
  const items = all.filter(x => !kind || x.kind === kind);
  const count = (k: ExceptionKind) => all.filter(x => x.kind === k).length;
  const changed = () => { reload(); refreshWork(); };

  return (
    <div className="view-enter">
      <PageHead
        eyebrow="업무"
        title="예외 처리"
        sub="자동화가 정상 처리한 건은 숨기고, 사람이 봐야 하는 문제만 모았어요. 해결하거나 업무로 넘겨 주세요."
        actions={<button type="button" className="adm-btn soft small" onClick={reload}>새로고침</button>}
      />
      {scope === 'active' && (
        <MiniStats items={[
          { label: '치명', value: all.filter(x => x.severity === 'CRITICAL').length, tone: all.some(x => x.severity === 'CRITICAL') ? 'alert' : 'muted', hint: '바로 처리' },
          { label: '높음', value: all.filter(x => x.severity === 'HIGH').length, tone: all.some(x => x.severity === 'HIGH') ? 'warn' : 'muted' },
          { label: '담당자 없음', value: all.filter(x => !x.assignee_id).length, hint: '담당자를 지정해 주세요' },
          { label: '반복 발생', value: all.filter(x => x.occurrences > 1).length, hint: '같은 문제가 여러 번' },
        ]} />
      )}
      <Filters
        label="예외 종류"
        value={kind}
        onChange={v => setParams(v ? { kind: v } : {}, { replace: true })}
        options={[{ value: '' as ExceptionKind | '', label: '전체', count: all.length }, ...EXCEPTION_KINDS.map(k => ({ value: k as ExceptionKind | '', label: EXCEPTION_KIND[k].label, count: count(k) }))]}
      />
      <div className="adm-queue-tools">
        <Seg label="상태" value={scope} onChange={setScope} options={[['active', '처리할 것'], ['RESOLVED', '해결'], ['IGNORED', '무시']] as const} />
        {kind && <Chip tone="gray">{EXCEPTION_KIND[kind].hint}</Chip>}
      </div>
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <Skeleton rows={4} /> : items.length === 0 ? (
        <Empty icon={<Glyph name="bolt" size={22} />} title={scope === 'active' ? '처리할 예외가 없어요' : '해당하는 예외가 없어요'}>
          {scope === 'active' ? '자동화가 모두 정상으로 처리하고 있어요.' : undefined}
        </Empty>
      ) : (
        <div className="adm-list">{items.map(x => <ExceptionCard key={x.id} x={x} focus={x.id === focusId} onChanged={changed} />)}</div>
      )}
    </div>
  );
}
