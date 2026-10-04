// 업무 모듈 공통 조각 — 담당자 표시·선택, 우선순위·SLA 칩, 스위치, 연결 항목, 내부 메모(@멘션·첨부·해결).
// 버튼·입력칸·칩·카드는 관리자 화면(adm-*)과 스튜디오 공통 요소(aq-switch, Glyph, CheckIcon)를 그대로 쓴다.
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { Link } from '../../lib/router';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { Glyph } from '../../components/Glyph';
import { CheckIcon } from '../../components/Check';
import { useAsync } from '../../hooks/useAsync';
import { Chip, useStaff } from '../ui';
import { ago, when } from '../labels';
import { workApi } from './api';
import { ENTITY_KIND, PRIORITY, SLA_STATE, TASK_STATUS, entityLink } from './labels';
import { effectiveDue, remaining } from './sla';
import type { EntityKind, EntityRef, Priority, SlaState, StaffPerson, TaskStatus, WorkNote } from './types';

// ---------- 담당자 ----------
let peopleCache: Promise<StaffPerson[]> | null = null;
export function usePeople() {
  const [list, setList] = useState<StaffPerson[]>([]);
  useEffect(() => {
    let alive = true;
    (peopleCache ??= workApi.people().then(r => r.items).catch(e => { peopleCache = null; throw e; }))
      .then(v => { if (alive) setList(v); })
      .catch(() => { /* 이름 대신 이메일 앞부분을 보여 준다 */ });
    return () => { alive = false; };
  }, []);
  return useMemo(() => {
    const by = new Map(list.map(p => [p.user_id, p]));
    return { list, get: (id: string | null | undefined) => (id ? by.get(id) : undefined) };
  }, [list]);
}

const initialOf = (p?: StaffPerson) => (p?.name || p?.email || '?').replace(/^나 \(|\)$/g, '').trim().slice(0, 1).toUpperCase();

export function Person({ id, people, empty = '담당자 없음', compact }: { id: string | null | undefined; people: ReturnType<typeof usePeople>; empty?: string; compact?: boolean }) {
  const { me } = useStaff();
  if (!id) return <span className="adm-w-person is-empty"><span className="adm-w-avatar" aria-hidden="true">–</span>{!compact && empty}</span>;
  const p = people.get(id);
  const name = id === me.user_id ? '나' : p?.name ?? p?.email.split('@')[0] ?? '담당자';
  return (
    <span className="adm-w-person" title={p?.email}>
      <span className={`adm-w-avatar${id === me.user_id ? ' is-me' : ''}`} aria-hidden="true">{id === me.user_id ? '나' : initialOf(p)}</span>
      {!compact && <span className="adm-w-person-name">{name}</span>}
    </span>
  );
}

export function PersonSelect({ id, value, onChange, people, empty = '지정 안 함', label }: {
  id: string; value: string | null; onChange: (v: string | null) => void; people: ReturnType<typeof usePeople>; empty?: string; label?: string;
}) {
  const { me } = useStaff();
  return (
    <select id={id} className="adm-select adm-w-select" aria-label={label} value={value ?? ''} onChange={e => onChange(e.target.value || null)}>
      <option value="">{empty}</option>
      {people.list.map(p => <option key={p.user_id} value={p.user_id}>{p.user_id === me.user_id ? `나 (${p.email})` : `${p.name} · ${p.email}`}</option>)}
    </select>
  );
}

// ---------- 칩 ----------
export function PriorityChip({ value, raisedFrom }: { value: Priority; raisedFrom?: Priority | null }) {
  const [label, tone] = PRIORITY[value];
  return (
    <Chip tone={tone}>
      <span className={`adm-w-pri is-${value.toLowerCase()}`} aria-hidden="true" />{label}
      {raisedFrom && <span className="sr-only"> (기한 때문에 {PRIORITY[raisedFrom][0]}에서 올라감)</span>}
      {raisedFrom && <Glyph name="arrow-up-right" size={12} />}
    </Chip>
  );
}

export function TaskStatusChip({ value }: { value: TaskStatus }) {
  const [label, tone] = TASK_STATUS[value];
  return <Chip tone={tone}>{label}</Chip>;
}

/** SLA 칩 — 남은 시간까지 (기한 임박 2시간 남음 / 기한 초과 35분 지남) */
export function SlaChip({ state, due, short }: { state: SlaState; due: string | null; short?: boolean }) {
  if (state === 'NONE') return null;
  const [label, tone] = SLA_STATE[state];
  const live = state === 'OK' || state === 'DUE_SOON' || state === 'OVERDUE';
  return (
    <Chip tone={tone}>
      <Glyph name="clock" size={12} />
      {short && live ? remaining(due) : <>{label}{live && due ? ` · ${remaining(due)}` : ''}</>}
    </Chip>
  );
}

export const dueOf = effectiveDue;

// ---------- 스위치 (스튜디오 aq-switch) ----------
export function Switch({ checked, onChange, label, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <label className="aq-switch">
      <input type="checkbox" role="switch" aria-label={label} checked={checked} disabled={disabled} onChange={e => onChange(e.target.checked)} />
      <span aria-hidden="true" />
    </label>
  );
}

/** 제목·설명 + 오른쪽 스위치 한 줄 (설정 화면) */
export function SwitchRow({ title, hint, checked, onChange, disabled }: { title: string; hint?: ReactNode; checked: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  return (
    <div className="adm-w-switch-row">
      <span className="adm-min"><b>{title}</b>{hint && <small>{hint}</small>}</span>
      <Switch checked={checked} onChange={onChange} label={title} disabled={disabled} />
    </div>
  );
}

// ---------- 연결 항목 ----------
/** plain: 이미 링크인 줄(목록 행) 안에서는 글자로만 */
export function RelatedLinks({ items, onRemove, plain }: { items: EntityRef[]; onRemove?: (i: number) => void; plain?: boolean }) {
  if (items.length === 0) return null;
  return (
    <span className="adm-w-related">
      {items.map((r, i) => {
        const href = entityLink(r.kind, r.id);
        const body = <><Glyph name={ENTITY_KIND[r.kind].icon} size={13} /><span className="adm-w-related-kind">{ENTITY_KIND[r.kind].label}</span>{r.label || r.id}</>;
        return (
          <span key={`${r.kind}:${r.id}`} className="adm-w-ref">
            {href && !onRemove && !plain ? <Link to={href}>{body}</Link> : <span>{body}</span>}
            {onRemove && <button type="button" aria-label={`${r.label} 연결 해제`} onClick={() => onRemove(i)}><Glyph name="close" size={12} /></button>}
          </span>
        );
      })}
    </span>
  );
}

// ---------- 내부 메모 ----------
const MAX_FILE = 2 * 1024 * 1024;
const MAX_FILES = 3;

const sizeLabel = (n: number) => (n < 1024 ? `${n}B` : n < 1024 * 1024 ? `${Math.round(n / 1024)}KB` : `${(n / 1024 / 1024).toFixed(1)}MB`);

/** 본문 속 @이름을 강조 */
function NoteBody({ text, people }: { text: string; people: ReturnType<typeof usePeople> }) {
  const names = people.list.map(p => p.name).filter(Boolean).sort((a, b) => b.length - a.length);
  if (names.length === 0) return <p>{text}</p>;
  const esc = names.map(n => n.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'));
  const parts = text.split(new RegExp(`(@(?:${esc.join('|')}))`, 'g'));
  return <p>{parts.map((s, i) => (s.startsWith('@') && names.includes(s.slice(1)) ? <b key={i} className="adm-w-mention">{s}</b> : s))}</p>;
}

/** 고객에게 보이지 않는 내부 메모 — 업무·발매·거래처·DSP 어디에나 붙는다 */
export function NotesPanel({ kind, targetId, title = '내부 메모', onChange }: { kind: EntityKind; targetId: string; title?: string; onChange?: () => void }) {
  const toast = useToast();
  const { me } = useStaff();
  const people = usePeople();
  const { data, loading, error, reload } = useAsync(() => workApi.notes(kind, targetId), [kind, targetId]);
  const [body, setBody] = useState('');
  const [files, setFiles] = useState<File[]>([]);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<number | null>(null);
  const [showResolved, setShowResolved] = useState(false);
  const [mentionAt, setMentionAt] = useState<number | null>(null);
  const area = useRef<HTMLTextAreaElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  const query = mentionAt != null ? body.slice(mentionAt + 1, area.current?.selectionStart ?? body.length).toLowerCase() : '';
  const suggestions = mentionAt == null ? [] : people.list.filter(p => p.user_id !== me.user_id && (p.name.toLowerCase().includes(query) || p.email.toLowerCase().startsWith(query))).slice(0, 5);

  const onType = (v: string, caret: number) => {
    setBody(v);
    const before = v.slice(0, caret);
    const m = /(^|\s)@([^\s@]*)$/.exec(before);
    setMentionAt(m ? caret - m[2].length - 1 : null);
  };
  const pickMention = (p: StaffPerson) => {
    if (mentionAt == null) return;
    const caret = area.current?.selectionStart ?? body.length;
    const next = `${body.slice(0, mentionAt)}@${p.name} ${body.slice(caret)}`;
    setBody(next);
    setMentionAt(null);
    requestAnimationFrame(() => {
      const pos = mentionAt + p.name.length + 2;
      area.current?.focus();
      area.current?.setSelectionRange(pos, pos);
    });
  };

  const addFiles = (list: FileList | null) => {
    if (!list) return;
    const picked = [...files];
    for (const f of Array.from(list)) {
      if (f.size > MAX_FILE) { toast(`${f.name}: 파일은 2MB까지 첨부할 수 있어요.`, 'error'); continue; }
      if (picked.length >= MAX_FILES) { toast('메모 하나에 파일은 3개까지 첨부할 수 있어요.', 'error'); break; }
      picked.push(f);
    }
    setFiles(picked);
    if (fileRef.current) fileRef.current.value = '';
  };

  const send = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy || (!body.trim() && files.length === 0)) return;
    setBusy(true);
    try {
      const mentions = people.list.filter(p => body.includes(`@${p.name}`)).map(p => p.user_id);
      const attachments: string[] = [];
      for (const [i, f] of files.entries()) {
        setProgress(i / files.length);
        attachments.push(await workApi.upload(f, d => setProgress((i + d) / files.length)));
      }
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

  const toggle = async (n: WorkNote) => {
    try {
      await workApi.resolveNote(n.id, !n.resolved);
      reload();
    } catch (err) {
      toast(errorMessage(err, '바꾸지 못했어요.'), 'error');
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
        <ol className="adm-w-note-list">
          {shown.map(n => (
            <li key={n.id} className={`adm-w-note${n.resolved ? ' is-resolved' : ''}`}>
              <div className="adm-w-note-top">
                <Person id={n.author_id} people={people} />
                <small className="muted" title={when(n.created_at)}>{ago(n.created_at)}</small>
                <button type="button" className={`adm-w-resolve${n.resolved ? ' is-on' : ''}`} onClick={() => toggle(n)} aria-pressed={n.resolved}>
                  <CheckIcon size={12} />{n.resolved ? '해결됨' : '해결 표시'}
                </button>
              </div>
              {n.body && <NoteBody text={n.body} people={people} />}
              {n.attachments.length > 0 && (
                <ul className="adm-w-files">
                  {n.attachments.map(a => {
                    const href = workApi.attachmentUrl(a.id);
                    const inner = <><Glyph name="clip" size={13} />{a.name}<small>{sizeLabel(a.size)}</small></>;
                    return <li key={a.id}>{href ? <a href={href} target="_blank" rel="noopener noreferrer">{inner}</a> : <span>{inner}</span>}</li>;
                  })}
                </ul>
              )}
            </li>
          ))}
        </ol>
      )}
      <form className="adm-w-note-form" onSubmit={send}>
        <label htmlFor={`note-${targetId}`} className="sr-only">메모</label>
        <textarea
          id={`note-${targetId}`} ref={area} className="adm-textarea" rows={3} maxLength={4000} value={body}
          placeholder="메모를 남겨요. @이름으로 동료에게 알릴 수 있어요."
          onChange={e => onType(e.target.value, e.target.selectionStart)}
          onKeyDown={e => {
            if (suggestions.length && (e.key === 'Enter' || e.key === 'Tab')) { e.preventDefault(); pickMention(suggestions[0]); return; }
            if (e.key === 'Escape') setMentionAt(null);
            if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) void send(e);
          }}
        />
        {suggestions.length > 0 && (
          <div className="adm-w-suggest" role="listbox" aria-label="언급할 동료">
            {suggestions.map(p => (
              <button key={p.user_id} type="button" role="option" aria-selected="false" onMouseDown={e => { e.preventDefault(); pickMention(p); }}>
                <Person id={p.user_id} people={people} /><small>{p.email}</small>
              </button>
            ))}
          </div>
        )}
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
            <input ref={fileRef} type="file" multiple hidden onChange={e => addFiles(e.target.files)} />
            <button type="button" className="adm-btn soft small" onClick={() => fileRef.current?.click()}><Glyph name="clip" size={14} />파일 첨부</button>
            <button type="button" className="adm-btn soft small" onClick={() => { const v = `${body}${body && !body.endsWith(' ') ? ' ' : ''}@`; onType(v, v.length); area.current?.focus(); }}><Glyph name="at" size={14} />멘션</button>
          </span>
          <button type="submit" className="adm-btn primary" disabled={busy || (!body.trim() && files.length === 0)}>{busy ? (progress != null && files.length ? `첨부 올리는 중 ${Math.round(progress * 100)}%` : '남기는 중…') : '메모 남기기'}</button>
        </div>
      </form>
    </section>
  );
}

/** 작은 숫자 카드 줄 — 개요의 adm-stat과 같은 모양, 크기만 작게 */
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
