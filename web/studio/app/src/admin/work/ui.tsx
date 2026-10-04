// 업무 모듈 공통 조각 — 담당자 표시·선택, 우선순위·상태·마감 칩, 스위치, Entity 연결·고르기,
// 내부 메모(@멘션·첨부·수정 이력·해결·보관). 버튼·입력칸·칩·카드는 관리자 화면(adm-*)과
// 스튜디오 공통 요소(aq-switch, Glyph, CheckIcon, Modal, Confirm)를 그대로 쓴다.
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { Link } from '../../lib/router';
import { useToast } from '../../components/Toast';
import { useConfirm } from '../../components/Confirm';
import { Modal } from '../../components/Modal';
import { errorMessage } from '../../api/errors';
import { Glyph } from '../../components/Glyph';
import { CheckIcon } from '../../components/Check';
import { useAsync } from '../../hooks/useAsync';
import { Chip, useStaff } from '../ui';
import { ago, when } from '../labels';
import { workApi } from './api';
import { DOC_KIND_LABEL, ENTITY_KIND, LINK_KINDS, PRIORITY, TASK_STATUS, entityLink, remaining } from './labels';
import type { EntityKind, EntityRef, Priority, StaffPerson, TaskStatus, WorkNote } from './types';

// ---------- 담당자 ----------
let peopleCache: Promise<StaffPerson[]> | null = null;
export function usePeople() {
  const [list, setList] = useState<StaffPerson[]>([]);
  useEffect(() => {
    let alive = true;
    (peopleCache ??= workApi.people().then(r => r.items).catch(e => { peopleCache = null; throw e; }))
      .then(v => { if (alive) setList(v); })
      .catch(() => { /* 이름 대신 '담당자'로 보여 준다 */ });
    return () => { alive = false; };
  }, []);
  return useMemo(() => {
    const by = new Map(list.map(p => [p.user_id, p]));
    return { list, get: (id: string | null | undefined) => (id ? by.get(id) : undefined) };
  }, [list]);
}
export type People = ReturnType<typeof usePeople>;

export function Person({ id, people, empty = '담당자 없음', compact }: { id: string | null | undefined; people: People; empty?: string; compact?: boolean }) {
  const { me } = useStaff();
  if (!id) return <span className="adm-w-person is-empty"><span className="adm-w-avatar" aria-hidden="true">–</span>{!compact && empty}</span>;
  const p = people.get(id);
  const mine = id === me.user_id;
  const name = mine ? '나' : p?.name ?? '담당자';
  return (
    <span className="adm-w-person" title={p?.email}>
      <span className={`adm-w-avatar${mine ? ' is-me' : ''}`} aria-hidden="true">{mine ? '나' : name.slice(0, 1).toUpperCase()}</span>
      {!compact && <span className="adm-w-person-name">{name}</span>}
    </span>
  );
}

export function PersonSelect({ id, value, onChange, people, empty = '지정 안 함', label }: {
  id: string; value: string | null; onChange: (v: string | null) => void; people: People; empty?: string; label?: string;
}) {
  const { me } = useStaff();
  return (
    <select id={id} className="adm-select adm-w-select" aria-label={label} value={value ?? ''} onChange={e => onChange(e.target.value || null)}>
      <option value="">{empty}</option>
      {people.list.map(p => <option key={p.user_id} value={p.user_id}>{p.user_id === me.user_id ? `나 (${p.email})` : p.email}</option>)}
    </select>
  );
}

// ---------- 칩 ----------
export function PriorityChip({ value }: { value: Priority }) {
  const [label, tone] = PRIORITY[value];
  return <Chip tone={tone}><span className={`adm-w-pri is-${value.toLowerCase()}`} aria-hidden="true" />{label}</Chip>;
}

export function TaskStatusChip({ value }: { value: TaskStatus }) {
  const [label, tone] = TASK_STATUS[value];
  return <Chip tone={tone}>{label}</Chip>;
}

/** 마감 칩 — 지났으면 빨강, 하루 안이면 주황 (SLA는 Phase 2) */
export function DueChip({ due, closed }: { due: string | null; closed?: boolean }) {
  if (!due || closed) return null;
  const ms = Date.parse(due) - Date.now();
  const tone = ms < 0 ? 'red' : ms < 24 * 3_600_000 ? 'amber' : 'gray';
  return <Chip tone={tone}><Glyph name="clock" size={12} />{remaining(due)}</Chip>;
}

// ---------- 스위치 (스튜디오 aq-switch) ----------
export function Switch({ checked, onChange, label, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <label className="aq-switch">
      <input type="checkbox" role="switch" aria-label={label} checked={checked} disabled={disabled} onChange={e => onChange(e.target.checked)} />
      <span aria-hidden="true" />
    </label>
  );
}

export function SwitchRow({ title, hint, checked, onChange, disabled }: { title: string; hint?: ReactNode; checked: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  return (
    <div className="adm-w-switch-row">
      <span className="adm-min"><b>{title}</b>{hint && <small>{hint}</small>}</span>
      <Switch checked={checked} onChange={onChange} label={title} disabled={disabled} />
    </div>
  );
}

// ---------- Entity 연결 ----------
const entitySub = (r: EntityRef) => (r.kind === 'document' && r.sub ? r.sub.replace(/^(AGREEMENT|RIGHTS_PROOF)/, m => DOC_KIND_LABEL[m] ?? m) : r.sub);

/** 연결된 Entity 칩 — 이름은 서버가 원천에서 읽은 값. plain: 이미 링크인 줄 안에서는 글자로만 */
export function EntityChips({ items, onRemove, plain }: { items: EntityRef[]; onRemove?: (r: EntityRef) => void; plain?: boolean }) {
  if (items.length === 0) return null;
  return (
    <span className="adm-w-related">
      {items.map(r => {
        const href = entityLink(r.kind, r.id);
        const body = (
          <>
            <Glyph name={ENTITY_KIND[r.kind].icon} size={13} />
            <span className="adm-w-related-kind">{ENTITY_KIND[r.kind].label}</span>
            {r.missing ? <i className="muted">없어진 항목</i> : r.label}
          </>
        );
        return (
          <span key={`${r.kind}:${r.id}`} className="adm-w-ref" title={entitySub(r) ?? undefined}>
            {href && !plain && !r.missing ? <Link to={href}>{body}</Link> : <span>{body}</span>}
            {onRemove && <button type="button" aria-label={`${r.label ?? r.id} 연결 해제`} onClick={() => onRemove(r)}><Glyph name="close" size={12} /></button>}
          </span>
        );
      })}
    </span>
  );
}

/** Entity 고르기 — 종류를 고르고 이름으로 찾는다 (서버가 원천 테이블에서 검색) */
export function EntityPicker({ onPick, kinds = LINK_KINDS, exclude = [] }: { onPick: (r: EntityRef) => void; kinds?: EntityKind[]; exclude?: string[] }) {
  const [kind, setKind] = useState<EntityKind>(kinds[0]);
  const [q, setQ] = useState('');
  const [items, setItems] = useState<EntityRef[] | null>(null);
  useEffect(() => {
    let alive = true;
    setItems(null);
    const t = window.setTimeout(() => {
      workApi.searchEntities(kind, q).then(r => { if (alive) setItems(r.items); }).catch(() => { if (alive) setItems([]); });
    }, q ? 250 : 0);
    return () => { alive = false; window.clearTimeout(t); };
  }, [kind, q]);
  const shown = (items ?? []).filter(r => !exclude.includes(`${r.kind}:${r.id}`));
  return (
    <div className="adm-w-picker">
      <div className="adm-w-picker-top">
        <select className="adm-select" aria-label="연결할 종류" value={kind} onChange={e => setKind(e.target.value as EntityKind)}>
          {kinds.map(k => <option key={k} value={k}>{ENTITY_KIND[k].label}</option>)}
        </select>
        <input className="adm-input" type="search" aria-label="이름으로 찾기" placeholder={`${ENTITY_KIND[kind].label} 이름으로 찾기`} value={q} onChange={e => setQ(e.target.value)} />
      </div>
      <div className="adm-w-picker-list" role="listbox" aria-label="찾은 항목">
        {items == null ? <span className="small muted">찾는 중…</span> : shown.length === 0 ? <span className="small muted">찾은 항목이 없어요.</span> : shown.slice(0, 8).map(r => (
          <button key={`${r.kind}:${r.id}`} type="button" role="option" aria-selected="false" onClick={() => onPick(r)}>
            <Glyph name={ENTITY_KIND[r.kind].icon} size={14} />
            <span className="adm-min"><b>{r.label}</b>{entitySub(r) && <small>{entitySub(r)}</small>}</span>
            <Glyph name="plus" size={14} />
          </button>
        ))}
      </div>
    </div>
  );
}

// ---------- 내부 메모 ----------
export const MAX_FILES = 5;
export const MAX_FILE = 20 * 1024 * 1024;
export const sizeLabel = (n: number) => (n < 1024 ? `${n}B` : n < 1024 * 1024 ? `${Math.round(n / 1024)}KB` : `${(n / 1024 / 1024).toFixed(1)}MB`);

/** 본문 속 @이름 강조 */
export function NoteBody({ text, people }: { text: string; people: People }) {
  const names = people.list.map(p => p.name).filter(Boolean).sort((a, b) => b.length - a.length);
  if (names.length === 0) return <p>{text}</p>;
  const esc = names.map(n => n.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'));
  const parts = text.split(new RegExp(`(@(?:${esc.join('|')}))`, 'g'));
  return <p>{parts.map((s, i) => (s.startsWith('@') && names.includes(s.slice(1)) ? <b key={i} className="adm-w-mention">{s}</b> : s))}</p>;
}

export const mentionsIn = (body: string, people: People) => people.list.filter(p => body.includes(`@${p.name}`)).map(p => p.user_id);

/** @ 입력 시 동료 추천이 뜨는 메모 입력칸 */
export function MentionArea({ id, value, onChange, people, onSubmit, placeholder, rows = 3, enterSends = false }: {
  id: string; value: string; onChange: (v: string) => void; people: People; onSubmit?: () => void; placeholder: string; rows?: number;
  /** 채팅: Enter로 보내고 Shift+Enter로 줄 바꿈 (메모는 Ctrl/⌘+Enter) */
  enterSends?: boolean;
}) {
  const { me } = useStaff();
  const area = useRef<HTMLTextAreaElement>(null);
  const [at, setAt] = useState<number | null>(null);
  const query = at != null ? value.slice(at + 1, area.current?.selectionStart ?? value.length).toLowerCase() : '';
  const suggestions = at == null ? [] : people.list.filter(p => p.user_id !== me.user_id && (p.name.toLowerCase().includes(query) || p.email.toLowerCase().startsWith(query))).slice(0, 5);
  const type = (v: string, caret: number) => {
    onChange(v);
    const m = /(^|\s)@([^\s@]*)$/.exec(v.slice(0, caret));
    setAt(m ? caret - m[2].length - 1 : null);
  };
  const pick = (p: StaffPerson) => {
    if (at == null) return;
    const caret = area.current?.selectionStart ?? value.length;
    onChange(`${value.slice(0, at)}@${p.name} ${value.slice(caret)}`);
    const pos = at + p.name.length + 2;
    setAt(null);
    requestAnimationFrame(() => { area.current?.focus(); area.current?.setSelectionRange(pos, pos); });
  };
  return (
    <div className="adm-w-mention-area">
      <textarea
        id={id} ref={area} className="adm-textarea" rows={rows} maxLength={4000} value={value} placeholder={placeholder}
        onChange={e => type(e.target.value, e.target.selectionStart)}
        onKeyDown={e => {
          if (suggestions.length && (e.key === 'Enter' || e.key === 'Tab')) { e.preventDefault(); pick(suggestions[0]); return; }
          if (e.key === 'Escape') setAt(null);
          if (e.key === 'Enter' && (e.metaKey || e.ctrlKey || (enterSends && !e.shiftKey && !e.nativeEvent.isComposing))) { e.preventDefault(); onSubmit?.(); }
        }}
      />
      {suggestions.length > 0 && (
        <div className="adm-w-suggest" role="listbox" aria-label="언급할 동료">
          {suggestions.map(p => (
            <button key={p.user_id} type="button" role="option" aria-selected="false" onMouseDown={e => { e.preventDefault(); pick(p); }}>
              <Person id={p.user_id} people={people} /><small>{p.email}</small>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function Revisions({ noteId, people, onClose }: { noteId: string; people: People; onClose: () => void }) {
  const { data, loading, error } = useAsync(() => workApi.noteRevisions(noteId), [noteId]);
  return (
    <Modal title="수정 이력" onClose={onClose}>
      {error ? <div className="adm-alert is-error">{error}</div> : loading && !data ? <div className="adm-skel"><span /></div> : (
        <ol className="adm-w-note-list">
          {(data?.items ?? []).map((r, i) => (
            <li key={i} className="adm-w-note">
              <div className="adm-w-note-top"><Person id={r.edited_by} people={people} /><small className="muted">{when(r.edited_at)}에 고치기 전</small></div>
              <NoteBody text={r.body} people={people} />
            </li>
          ))}
        </ol>
      )}
    </Modal>
  );
}

function NoteItem({ n, people, onChanged }: { n: WorkNote; people: People; onChanged: () => void }) {
  const toast = useToast();
  const confirm = useConfirm();
  const { me } = useStaff();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(n.body);
  const [busy, setBusy] = useState(false);
  const [history, setHistory] = useState(false);
  const mine = n.author_id === me.user_id;

  const run = async (fn: () => Promise<unknown>, msg?: string) => {
    if (busy) return;
    setBusy(true);
    try { await fn(); if (msg) toast(msg, 'success'); onChanged(); } catch (err) { toast(errorMessage(err, '처리하지 못했어요.'), 'error'); } finally { setBusy(false); }
  };
  const save = () => run(async () => { await workApi.editNote(n.id, { body: draft, mentions: mentionsIn(draft, people) }); setEditing(false); }, '메모를 고쳤어요. 이전 내용은 수정 이력에 남아요.');
  const archive = async () => {
    if (await confirm({ title: '메모를 보관할까요?', message: '목록에서 사라지지만 기록은 남아요.', confirmLabel: '보관', danger: true })) void run(() => workApi.archiveNote(n.id), '메모를 보관했어요.');
  };

  return (
    <li className={`adm-w-note${n.resolved ? ' is-resolved' : ''}`}>
      <div className="adm-w-note-top">
        <Person id={n.author_id} people={people} />
        <small className="muted" title={when(n.created_at)}>{ago(n.created_at)}{n.edited_at && ' · 수정됨'}</small>
        <span className="adm-w-note-actions">
          {n.revisions > 0 && <button type="button" className="adm-link-btn" onClick={() => setHistory(true)}>이력 {n.revisions}</button>}
          {mine && !editing && <button type="button" className="adm-link-btn" onClick={() => { setDraft(n.body); setEditing(true); }}>고치기</button>}
          {(mine || me.role === 'ADMIN') && <button type="button" className="adm-link-btn" onClick={archive}>보관</button>}
          <button type="button" className={`adm-w-resolve${n.resolved ? ' is-on' : ''}`} disabled={busy} aria-pressed={n.resolved} onClick={() => run(() => workApi.resolveNote(n.id, !n.resolved))}>
            <CheckIcon size={12} />{n.resolved ? '해결됨' : '해결 표시'}
          </button>
        </span>
      </div>
      {editing ? (
        <div className="adm-w-note-edit">
          <MentionArea id={`edit-${n.id}`} value={draft} onChange={setDraft} people={people} onSubmit={save} placeholder="메모" />
          <div className="adm-form-actions">
            <button type="button" className="adm-btn soft small" onClick={() => setEditing(false)}>취소</button>
            <button type="button" className="adm-btn primary small" disabled={busy || (!draft.trim() && n.attachments.length === 0)} onClick={save}>저장</button>
          </div>
        </div>
      ) : n.body && <NoteBody text={n.body} people={people} />}
      {n.attachments.length > 0 && (
        <ul className="adm-w-files">
          {n.attachments.map(a => {
            const href = workApi.attachmentUrl(a.id);
            const inner = <><Glyph name="clip" size={13} />{a.name}<small>{sizeLabel(a.size)}</small></>;
            return <li key={a.id}>{href ? <a href={href} download>{inner}</a> : <span>{inner}</span>}</li>;
          })}
        </ul>
      )}
      {history && <Revisions noteId={n.id} people={people} onClose={() => setHistory(false)} />}
    </li>
  );
}

/** 고객에게 보이지 않는 내부 메모 — 업무·발매·아티스트·계약·DSP 어디에나 붙는다 */
export function NotesPanel({ kind, targetId, title = '내부 메모', onChange }: { kind: EntityKind; targetId: string; title?: string; onChange?: () => void }) {
  const toast = useToast();
  const people = usePeople();
  const { data, loading, error, reload } = useAsync(() => workApi.notes(kind, targetId), [kind, targetId]);
  const [body, setBody] = useState('');
  const [files, setFiles] = useState<File[]>([]);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<number | null>(null);
  const [showResolved, setShowResolved] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);

  const addFiles = (list: FileList | null) => {
    if (!list) return;
    const picked = [...files];
    for (const f of Array.from(list)) {
      if (f.size > MAX_FILE) { toast(`${f.name}: 파일은 20MB까지 첨부할 수 있어요.`, 'error'); continue; }
      if (picked.length >= MAX_FILES) { toast('메모 하나에 파일은 5개까지 첨부할 수 있어요.', 'error'); break; }
      picked.push(f);
    }
    setFiles(picked);
    if (fileRef.current) fileRef.current.value = '';
  };

  const send = async () => {
    if (busy || (!body.trim() && files.length === 0)) return;
    setBusy(true);
    try {
      const attachments: string[] = [];
      for (const [i, f] of files.entries()) {
        setProgress(i / files.length);
        attachments.push(await workApi.upload(f, (r: number) => setProgress((i + r) / files.length)));
      }
      const mentions = mentionsIn(body, people);
      await workApi.addNote({ target_kind: kind, target_id: targetId, body: body.trim(), mentions, attachments });
      setBody(''); setFiles([]);
      toast(mentions.length ? `메모를 남기고 ${mentions.length}명에게 알렸어요.` : '메모를 남겼어요.', 'success');
      reload();
      onChange?.();
    } catch (err) {
      toast(errorMessage(err, '메모를 남기지 못했어요.'), 'error');
    } finally {
      setBusy(false);
      setProgress(null);
    }
  };

  const items = data?.items ?? [];
  const resolvedCount = items.filter(n => n.resolved).length;
  const shown = showResolved ? items : items.filter(n => !n.resolved);

  return (
    <section className="adm-w-notes" aria-label={title}>
      <div className="adm-section-top">
        <h2>{title}<small><Glyph name="lock" size={12} className="aq-inline-glyph" /> 고객에게는 보이지 않아요</small></h2>
        {resolvedCount > 0 && <button type="button" className="adm-link-btn" onClick={() => setShowResolved(v => !v)}>{showResolved ? '해결된 메모 숨기기' : `해결된 메모 ${resolvedCount}개 보기`}</button>}
      </div>
      {error && <div className="adm-alert is-error">{error}</div>}
      {loading && !data ? <div className="adm-skel"><span /></div> : shown.length === 0 ? (
        <p className="adm-w-notes-empty">{items.length ? '열린 메모가 없어요.' : '아직 메모가 없어요. @이름으로 동료를 부를 수 있어요.'}</p>
      ) : (
        <ol className="adm-w-note-list">{shown.map(n => <NoteItem key={n.id} n={n} people={people} onChanged={() => { reload(); onChange?.(); }} />)}</ol>
      )}
      <form className="adm-w-note-form" onSubmit={e => { e.preventDefault(); void send(); }}>
        <label htmlFor={`note-${targetId}`} className="sr-only">메모</label>
        <MentionArea id={`note-${targetId}`} value={body} onChange={setBody} people={people} onSubmit={send} placeholder="메모를 남겨요. @이름으로 동료에게 알릴 수 있어요." />
        {files.length > 0 && (
          <ul className="adm-w-files is-pending">
            {files.map((f, i) => (
              <li key={`${f.name}-${i}`}><span><Glyph name="clip" size={13} />{f.name}<small>{sizeLabel(f.size)}</small></span>
                <button type="button" aria-label={`${f.name} 빼기`} onClick={() => setFiles(files.filter((_, j) => j !== i))}><Glyph name="close" size={12} /></button>
              </li>
            ))}
          </ul>
        )}
        <div className="adm-reply-foot">
          <span className="adm-w-note-tools">
            <input ref={fileRef} type="file" multiple hidden accept=".pdf,.png,.jpg,.jpeg,.webp,.txt,.csv,.zip,.xlsx,.xls,.docx,.doc,.pptx" onChange={e => addFiles(e.target.files)} />
            <button type="button" className="adm-btn soft small" onClick={() => fileRef.current?.click()}><Glyph name="clip" size={14} />파일 첨부</button>
            <button type="button" className="adm-btn soft small" onClick={() => setBody(b => `${b}${b && !b.endsWith(' ') ? ' ' : ''}@`)}><Glyph name="at" size={14} />멘션</button>
          </span>
          <button type="submit" className="adm-btn primary" disabled={busy || (!body.trim() && files.length === 0)}>
            {busy ? (progress != null && files.length ? `첨부 올리는 중 ${Math.round(progress * 100)}%` : '남기는 중…') : '메모 남기기'}
          </button>
        </div>
      </form>
    </section>
  );
}

/** 작은 숫자 카드 줄 — 개요의 adm-stat과 같은 면, 높이만 낮게 */
export function MiniStats({ items }: { items: { label: string; value: ReactNode; hint?: string; tone?: 'alert' | 'warn' | 'muted'; onClick?: () => void; active?: boolean }[] }) {
  return (
    <div className="adm-w-stats">
      {items.map(s => {
        const cls = ['adm-w-stat', s.tone ? `is-${s.tone}` : '', s.active ? 'is-active' : ''].filter(Boolean).join(' ');
        const inner = <><strong>{s.value}</strong><span>{s.label}</span>{s.hint && <small>{s.hint}</small>}</>;
        return s.onClick
          ? <button key={s.label} type="button" className={cls} onClick={s.onClick} aria-pressed={s.active}>{inner}</button>
          : <div key={s.label} className={cls}>{inner}</div>;
      })}
    </div>
  );
}
