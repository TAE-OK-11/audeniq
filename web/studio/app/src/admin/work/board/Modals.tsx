// 보드 대화상자 — 새 카드, 열 관리(추가·이름·상태·WIP·순서·보관), 보드 만들기, 자동 규칙(ADMIN).
// 권한은 서버가 판단한다. 화면은 access 플래그·역할을 따라 버튼을 보여 주거나 감출 뿐이다.
import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent } from 'react';
import { useModalClose } from '../../../components/Modal';
import { useToast } from '../../../components/Toast';
import { useConfirm } from '../../../components/Confirm';
import { Glyph } from '../../../components/Glyph';
import { ErrorBox, SkeletonList, useStaff } from '../../ui';
import { when } from '../../labels';
import { workApi } from '../api';
import { AUTO_TRIGGER, BOARD_KIND, PRIORITIES, PRIORITY, PRIORITY_CODE, PRIORITY_NAME, ROLE_NAME, TASK_STATUS, WORK_TYPE, WORK_TYPES } from '../labels';
import { PersonSelect, Switch, usePeople } from '../ui';
import { boardError, needsReload } from './logic';
import type {
  AutoTrigger, Board, BoardColumn, BoardKind, BoardRule, BoardView, Priority, StaffRoleKey, TaskStatus, WorkTask, WorkType,
} from '../types';
import { josa } from '../../../lib/josa';
import { isAdmin } from '../../api';

const COLUMN_STATUSES: TaskStatus[] = ['TODO', 'IN_PROGRESS', 'IN_REVIEW', 'BLOCKED', 'DONE', 'CANCELLED'];
const ROLES: StaffRoleKey[] = ['ADMIN', 'REVIEWER', 'OPERATOR', 'SUPPORT'];

// ---------- 새 카드 ----------
export function NewCardForm({ view, onCreated }: { view: BoardView; onCreated: (t: WorkTask) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const { me } = useStaff();
  const people = usePeople();
  const cols = view.columns.filter(c => c.status !== 'IN_REVIEW');
  const [title, setTitle] = useState('');
  const [type, setType] = useState<WorkType>('GENERAL');
  const [priority, setPriority] = useState<Priority>('NORMAL');
  const [column, setColumn] = useState(cols.find(c => c.status === 'TODO')?.id ?? cols[0]?.id ?? '');
  const [assignee, setAssignee] = useState<string | null>(me.user_id);
  const [busy, setBusy] = useState(false);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy || !title.trim()) return;
    setBusy(true);
    try {
      const d = await workApi.createTask({ title: title.trim(), type, priority, assignee_id: assignee, board_id: view.board.id, column_id: column || null });
      toast(`${d.task.key} 카드를 만들었어요.`, 'success');
      onCreated(d.task);
      close();
    } catch (err) {
      toast(boardError(err, '카드를 만들지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <form onSubmit={submit}>
      <div className="ui-field">
        <label htmlFor="kbnTitle">제목 <span className="required">*</span></label>
        <input id="kbnTitle" data-autofocus className="ui-input" maxLength={200} value={title} onChange={e => setTitle(e.target.value)} placeholder="예: Paper Moon 메타데이터 수정 확인" />
      </div>
      <div className="adm-w-grid2">
        <div className="ui-field">
          <label htmlFor="kbnColumn">열</label>
          <select id="kbnColumn" className="ui-select" value={column} onChange={e => setColumn(e.target.value)}>
            {cols.map(c => <option key={c.id} value={c.id}>{c.name} · {TASK_STATUS[c.status][0]}</option>)}
          </select>
        </div>
        <div className="ui-field">
          <label htmlFor="kbnAssignee">담당자</label>
          <PersonSelect id="kbnAssignee" value={assignee} onChange={setAssignee} people={people} empty="담당자 없음" />
        </div>
        <div className="ui-field">
          <label htmlFor="kbnPriority">우선순위</label>
          <select id="kbnPriority" className="ui-select" value={priority} onChange={e => setPriority(e.target.value as Priority)}>
            {PRIORITIES.map(p => <option key={p} value={p}>{PRIORITY_CODE[p]} · {PRIORITY_NAME[p]} ({PRIORITY[p][0]})</option>)}
          </select>
        </div>
        <div className="ui-field">
          <label htmlFor="kbnType">유형</label>
          <select id="kbnType" className="ui-select" value={type} onChange={e => setType(e.target.value as WorkType)}>
            {WORK_TYPES.map(w => <option key={w} value={w}>{WORK_TYPE[w]}</option>)}
          </select>
        </div>
      </div>
      <p className="small muted">검토 열에는 검토자를 정한 뒤 카드를 옮겨 주세요.</p>
      <div className="adm-form-actions">
        <button type="button" className="ui-btn is-soft" onClick={close}>닫기</button>
        <button type="submit" className="ui-btn is-primary" disabled={busy || !title.trim()}>{busy ? '만드는 중…' : '카드 만들기'}</button>
      </div>
    </form>
  );
}

// ---------- 열 관리 ----------
function ColumnRow({ col, index, total, others, busy, run, onMove }: {
  col: BoardColumn; index: number; total: number; others: BoardColumn[]; busy: boolean;
  run: (fn: () => Promise<BoardView>, msg: string) => Promise<boolean>;
  onMove: (from: number, to: number) => void;
}) {
  const confirm = useConfirm();
  const [name, setName] = useState(col.name);
  const [wip, setWip] = useState(col.wip_limit == null ? '' : String(col.wip_limit));
  const [archiving, setArchiving] = useState(false);
  const [moveTo, setMoveTo] = useState(others[0]?.id ?? '');
  useEffect(() => { setName(col.name); setWip(col.wip_limit == null ? '' : String(col.wip_limit)); }, [col.name, col.wip_limit]);

  const saveName = () => {
    const n = name.trim();
    if (!n) { setName(col.name); return; }
    if (n !== col.name) void run(() => workApi.updateColumn(col.id, { name: n }), '열 이름을 바꿨어요.');
  };
  const saveWip = () => {
    const v = wip.trim() === '' ? null : Number(wip);
    if (v != null && (!Number.isInteger(v) || v < 1 || v > 999)) { setWip(col.wip_limit == null ? '' : String(col.wip_limit)); return; }
    if (v !== col.wip_limit) void run(() => workApi.updateColumn(col.id, { wip_limit: v }), v == null ? 'WIP 한도를 없앴어요.' : `WIP 한도를 ${josa(v, '으로/로')} 정했어요.`);
  };
  const changeStatus = async (s: TaskStatus) => {
    if (s === col.status) return;
    if (col.count > 0 && !(await confirm({
      title: '열 상태를 바꿀까요?', confirmLabel: '상태 바꾸기',
      message: `이 열의 카드 ${col.count}개도 모두 ‘${TASK_STATUS[s][0]}’ 상태가 돼요.`,
    }))) return;
    void run(() => workApi.updateColumn(col.id, { status: s }), `열 상태를 ${TASK_STATUS[s][0]}(으)로 바꿨어요.`);
  };
  const archive = async () => {
    if (col.count > 0 && !moveTo) return;
    const ok = await confirm({
      title: `‘${col.name}’ 열을 보관할까요?`, confirmLabel: '열 보관', danger: true,
      message: col.count > 0 ? `카드 ${col.count}개는 ‘${others.find(o => o.id === moveTo)?.name}’ 열로 옮겨지고 그 열의 상태를 따라요.` : '빈 열이라 카드는 옮겨지지 않아요.',
    });
    if (ok && await run(() => workApi.archiveColumn(col.id, col.count > 0 ? moveTo : null), '열을 보관했어요.')) setArchiving(false);
  };

  return (
    <li className="adm-kb-colrow">
      <div className="adm-kb-colrow-main">
        <span className="adm-kb-colrow-order">
          <button type="button" className="adm-kb-icon-btn" aria-label={`${col.name} 열을 앞(왼쪽)으로`} disabled={busy || index === 0} onClick={() => onMove(index, index - 1)}>
            <Glyph name="arrow-left" size={14} className="adm-kb-rot" />
          </button>
          <button type="button" className="adm-kb-icon-btn" aria-label={`${col.name} 열을 뒤(오른쪽)로`} disabled={busy || index === total - 1} onClick={() => onMove(index, index + 1)}>
            <Glyph name="arrow-right" size={14} className="adm-kb-rot" />
          </button>
        </span>
        <label className="sr-only" htmlFor={`kbcName-${col.id}`}>열 이름</label>
        <input id={`kbcName-${col.id}`} className="ui-input" maxLength={40} value={name} onChange={e => setName(e.target.value)} onBlur={saveName}
          onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); saveName(); } }} />
        <label className="sr-only" htmlFor={`kbcStatus-${col.id}`}>{col.name} 상태</label>
        <select id={`kbcStatus-${col.id}`} className="ui-select is-auto" value={col.status} disabled={busy} onChange={e => void changeStatus(e.target.value as TaskStatus)}>
          {COLUMN_STATUSES.map(s => <option key={s} value={s}>{TASK_STATUS[s][0]}</option>)}
        </select>
        <label className="adm-kb-wip-input">
          <span>WIP</span>
          <input className="ui-input" inputMode="numeric" placeholder="없음" aria-label={`${col.name} WIP 한도`} value={wip}
            onChange={e => setWip(e.target.value.replace(/\D/g, '').slice(0, 3))} onBlur={saveWip}
            onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); saveWip(); } }} />
        </label>
        <span className="adm-kb-colrow-count">카드 {col.count}</span>
        <button type="button" className="ui-btn is-text is-muted" disabled={busy || total <= 1} onClick={() => setArchiving(v => !v)} aria-expanded={archiving}>보관</button>
      </div>
      {archiving && (
        <div className="adm-kb-colrow-archive">
          {col.count > 0 ? (
            <label className="adm-w-inline">
              <span>카드 {col.count}개를 옮길 열</span>
              <select className="ui-select is-auto" value={moveTo} onChange={e => setMoveTo(e.target.value)}>
                {others.map(o => <option key={o.id} value={o.id}>{o.name} · {TASK_STATUS[o.status][0]}</option>)}
              </select>
            </label>
          ) : <span className="small muted">빈 열이에요.</span>}
          <button type="button" className="ui-btn is-danger is-sm" disabled={busy || (col.count > 0 && !moveTo)} onClick={() => void archive()}>열 보관</button>
        </div>
      )}
    </li>
  );
}

export function ColumnManager({ view, onView }: { view: BoardView; onView: (v: BoardView) => void }) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const [name, setName] = useState('');
  const [status, setStatus] = useState<TaskStatus>('TODO');
  const [boardName, setBoardName] = useState(view.board.name);
  const [boardDesc, setBoardDesc] = useState(view.board.description);
  const cols = view.columns;

  const run = async (fn: () => Promise<BoardView>, msg: string) => {
    if (busy) return false;
    setBusy(true);
    try {
      onView(await fn());
      toast(msg, 'success');
      return true;
    } catch (err) {
      toast(boardError(err, '바꾸지 못했어요.'), 'error');
      // 낡은 보드 버전·열이면 최신 보드를 받아 와야 다시 눌렀을 때 같은 이유로 거절되지 않는다
      if (needsReload(err)) {
        try { onView(await workApi.board(view.board.id)); } catch { /* 다음 동작에서 다시 알린다 */ }
      }
      return false;
    } finally {
      setBusy(false);
    }
  };
  const move = (from: number, to: number) => {
    const ids = cols.map(c => c.id);
    const [x] = ids.splice(from, 1);
    ids.splice(to, 0, x);
    void run(() => workApi.orderColumns(view.board.id, ids), `‘${cols[from].name}’ 열을 ${to < from ? '앞으로' : '뒤로'} 옮겼어요.`);
  };
  const add = async (e: FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return;
    if (await run(() => workApi.addColumn(view.board.id, { name: name.trim(), status }), `‘${name.trim()}’ 열을 추가했어요.`)) setName('');
  };
  const saveBoard = (e: FormEvent) => {
    e.preventDefault();
    if (!boardName.trim()) return;
    void run(() => workApi.updateBoard(view.board.id, { name: boardName.trim(), description: boardDesc.trim(), row_version: view.board.row_version }), '보드 정보를 저장했어요.');
  };

  return (
    <div className="adm-kb-manager">
      <form className="adm-kb-board-info" onSubmit={saveBoard}>
        <div className="ui-field">
          <label htmlFor="kbmName">보드 이름</label>
          <input id="kbmName" className="ui-input" maxLength={80} value={boardName} onChange={e => setBoardName(e.target.value)} />
        </div>
        <div className="ui-field">
          <label htmlFor="kbmDesc">설명</label>
          <input id="kbmDesc" className="ui-input" maxLength={500} value={boardDesc} onChange={e => setBoardDesc(e.target.value)} />
        </div>
        <button type="submit" className="ui-btn is-soft is-sm" disabled={busy || !boardName.trim() || (boardName.trim() === view.board.name && boardDesc.trim() === view.board.description)}>보드 정보 저장</button>
      </form>
      <h3 className="adm-w-subhead">열 {cols.length}/20</h3>
      <p className="small muted">열의 상태가 카드 상태예요. 상태를 바꾸면 그 열의 카드도 함께 바뀌어요. 이름·WIP는 칸을 벗어나거나 Enter를 누르면 저장돼요.</p>
      <ol className="adm-kb-colrows">
        {cols.map((c, i) => (
          <ColumnRow key={c.id} col={c} index={i} total={cols.length} others={cols.filter(o => o.id !== c.id)} busy={busy} run={run} onMove={move} />
        ))}
      </ol>
      <form className="adm-kb-coladd" onSubmit={add}>
        <label htmlFor="kbmNew" className="sr-only">새 열 이름</label>
        <input id="kbmNew" className="ui-input" maxLength={40} placeholder="새 열 이름" value={name} onChange={e => setName(e.target.value)} />
        <label htmlFor="kbmNewStatus" className="sr-only">새 열 상태</label>
        <select id="kbmNewStatus" className="ui-select is-auto" value={status} onChange={e => setStatus(e.target.value as TaskStatus)}>
          {COLUMN_STATUSES.map(s => <option key={s} value={s}>{TASK_STATUS[s][0]}</option>)}
        </select>
        <button type="submit" className="ui-btn is-primary is-sm" disabled={busy || !name.trim() || cols.length >= 20}><Glyph name="plus" size={13} />열 추가</button>
      </form>
    </div>
  );
}

// ---------- 보드 만들기 ----------
export function BoardCreateForm({ onCreated }: { onCreated: (v: BoardView) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const { me } = useStaff();
  const admin = isAdmin(me);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [kind, setKind] = useState<BoardKind>('PERSONAL');
  const [role, setRole] = useState<StaffRoleKey>('OPERATOR');
  const [busy, setBusy] = useState(false);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy || !name.trim()) return;
    setBusy(true);
    try {
      const v = await workApi.createBoard({ name: name.trim(), description: description.trim(), kind, team_role: kind === 'TEAM' ? role : null });
      toast(`‘${v.board.name}’ 보드를 만들었어요.`, 'success');
      onCreated(v);
      close();
    } catch (err) {
      toast(boardError(err, '보드를 만들지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  const KIND_HINT: Record<BoardKind, string> = {
    PERSONAL: '나만 보고 쓰는 보드예요.',
    TEAM: '모든 직원이 보고, 고른 팀(역할)과 관리자가 카드를 옮겨요.',
    OPERATIONS: '모든 직원이 보고 카드를 옮겨요.',
  };
  return (
    <form onSubmit={submit}>
      <div className="ui-field">
        <label htmlFor="kbbName">이름 <span className="required">*</span></label>
        <input id="kbbName" data-autofocus className="ui-input" maxLength={80} value={name} onChange={e => setName(e.target.value)} placeholder="예: 10월 발매 준비" />
      </div>
      <div className="ui-field">
        <label htmlFor="kbbDesc">설명</label>
        <input id="kbbDesc" className="ui-input" maxLength={500} value={description} onChange={e => setDescription(e.target.value)} placeholder="이 보드에서 다루는 일" />
      </div>
      <fieldset className="adm-kb-kinds">
        <legend className="ui-field-label">종류</legend>
        {(['PERSONAL', 'TEAM', 'OPERATIONS'] as BoardKind[]).map(k => (
          <label key={k} className={`adm-kb-kind${kind === k ? ' is-on' : ''}${k !== 'PERSONAL' && !admin ? ' is-disabled' : ''}`}>
            <input type="radio" name="kbbKind" value={k} checked={kind === k} disabled={k !== 'PERSONAL' && !admin} onChange={() => setKind(k)} />
            <span><b>{BOARD_KIND[k]} 보드</b><small>{KIND_HINT[k]}</small></span>
          </label>
        ))}
        {!admin && <small className="muted">팀·운영 보드는 관리자만 만들 수 있어요.</small>}
      </fieldset>
      {kind === 'TEAM' && (
        <div className="ui-field">
          <label htmlFor="kbbRole">팀(역할)</label>
          <select id="kbbRole" className="ui-select" value={role} onChange={e => setRole(e.target.value as StaffRoleKey)}>
            {ROLES.map(r => <option key={r} value={r}>{ROLE_NAME[r]}</option>)}
          </select>
        </div>
      )}
      <p className="small muted">기본 열(Inbox · To Do · In Progress · Review · Waiting · Blocked · Done)이 함께 만들어져요.</p>
      <div className="adm-form-actions">
        <button type="button" className="ui-btn is-soft" onClick={close}>닫기</button>
        <button type="submit" className="ui-btn is-primary" disabled={busy || !name.trim()}>{busy ? '만드는 중…' : '보드 만들기'}</button>
      </div>
    </form>
  );
}

// ---------- 자동 규칙 (ADMIN) ----------
function RuleRow({ trigger, rule, boards, colsMap, ensure, busy, onSave }: {
  trigger: AutoTrigger; rule: BoardRule | undefined; boards: Board[]; colsMap: Record<string, BoardColumn[]>; ensure: (boardId: string) => void;
  busy: boolean; onSave: (input: { trigger: AutoTrigger; board_id: string; column_id: string; enabled: boolean }) => void;
}) {
  const [boardId, setBoardId] = useState(rule?.board_id ?? '');
  const [columnId, setColumnId] = useState(rule?.column_id ?? '');
  useEffect(() => { setBoardId(rule?.board_id ?? ''); setColumnId(rule?.column_id ?? ''); }, [rule?.board_id, rule?.column_id]);
  const cols = boardId ? colsMap[boardId] : [];
  const enabled = rule?.enabled ?? false;
  const label = AUTO_TRIGGER[trigger] ?? trigger;
  const pickBoard = (id: string) => { setBoardId(id); setColumnId(''); if (id) ensure(id); };
  const pickColumn = (id: string) => {
    setColumnId(id);
    if (id && boardId && rule) onSave({ trigger, board_id: boardId, column_id: id, enabled });
  };
  return (
    <li className="adm-kb-rule">
      <span className="adm-min">
        <b>{label}</b>
        <small>{rule ? `${rule.board_name} · ${rule.column_name}${rule.enabled ? ` · ${when(rule.active_since)}부터` : ''}` : '아직 규칙이 없어요'}</small>
      </span>
      <select className="ui-select is-auto" aria-label={`${label} 카드를 놓을 보드`} value={boardId} disabled={busy} onChange={e => pickBoard(e.target.value)}>
        <option value="">보드 고르기</option>
        {boards.map(b => <option key={b.id} value={b.id}>{b.name}</option>)}
      </select>
      <select className="ui-select is-auto" aria-label={`${label} 카드를 놓을 열`} value={columnId} disabled={busy || !boardId || !cols} onChange={e => pickColumn(e.target.value)}>
        <option value="">{boardId && !cols ? '불러오는 중…' : '열 고르기'}</option>
        {(cols ?? []).map(c => <option key={c.id} value={c.id}>{c.name}</option>)}
      </select>
      <Switch label={`${label} 자동 카드`} checked={enabled} disabled={busy || !boardId || !columnId}
        onChange={v => onSave({ trigger, board_id: boardId, column_id: columnId, enabled: v })} />
    </li>
  );
}

export function RulesPanel({ boards, onRan }: { boards: Board[]; onRan: () => void }) {
  const toast = useToast();
  const [data, setData] = useState<{ items: BoardRule[]; triggers: AutoTrigger[] } | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [cols, setCols] = useState<Record<string, BoardColumn[]>>({});
  const [ran, setRan] = useState<number | null>(null);
  const shared = useMemo(() => boards.filter(b => b.kind !== 'PERSONAL' && !b.archived), [boards]);

  const load = () => {
    setError('');
    workApi.rules().then(setData).catch(e => setError(boardError(e, '자동 규칙을 불러오지 못했어요.')));
  };
  useEffect(load, []);
  // 열 목록은 보드마다 한 번만 불러 둔다 (규칙에 쓰인 보드 + 새로 고른 보드)
  const asked = useRef(new Set<string>());
  const ensure = useCallback((id: string) => {
    if (asked.current.has(id)) return;
    asked.current.add(id);
    workApi.board(id, { done_days: 1 })
      .then(v => setCols(c => ({ ...c, [id]: v.columns })))
      .catch(() => { asked.current.delete(id); });
  }, []);
  useEffect(() => { for (const r of data?.items ?? []) ensure(r.board_id); }, [data, ensure]);

  const save = async (input: { trigger: AutoTrigger; board_id: string; column_id: string; enabled: boolean }) => {
    if (busy) return;
    setBusy(true);
    try {
      setData(await workApi.saveRule(input));
      toast(`${AUTO_TRIGGER[input.trigger] ?? input.trigger} 규칙을 ${input.enabled ? '켰어요' : '저장했어요'}.`, 'success');
    } catch (err) {
      toast(boardError(err, '규칙을 저장하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  const runNow = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const r = await workApi.runAutomation();
      setRan(r.created);
      toast(r.created ? `자동 카드 ${r.created}개를 만들었어요.` : '새로 만들 자동 카드가 없어요.', 'success');
      onRan();
      load();
    } catch (err) {
      toast(boardError(err, '자동화를 실행하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };

  if (error) return <ErrorBox message={error} onRetry={load} />;
  if (!data) return <SkeletonList rows={4} />;
  return (
    <div className="adm-kb-rules">
      <p className="small muted">AUDENIQ에서 일이 생기면(QC 실패, 새 문의 등) 고른 보드·열에 카드가 자동으로 만들어져요. 규칙을 켠 뒤에 생긴 일만 카드가 돼요.</p>
      <ul className="adm-kb-rule-list">
        {data.triggers.map(tr => (
          <RuleRow key={tr} trigger={tr} rule={data.items.find(r => r.trigger === tr)} boards={shared} colsMap={cols} ensure={ensure} busy={busy} onSave={i => void save(i)} />
        ))}
      </ul>
      <div className="adm-form-actions">
        {ran != null && <span className="small muted" role="status">방금 실행: 카드 {ran}개</span>}
        <button type="button" className="ui-btn is-primary is-sm" disabled={busy} onClick={() => void runNow()}>{busy ? '실행 중…' : '지금 실행'}</button>
      </div>
    </div>
  );
}
