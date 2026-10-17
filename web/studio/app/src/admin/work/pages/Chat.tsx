// 팀 대화 — 직원끼리 채널에서 이야기한다. 메시지는 내부 메모와 같은 저장소(work.notes)라서
// @멘션은 업무함으로, 첨부는 업로드 검사를 거치고, 고친 메시지는 이력이 남는다.
// 새 메시지는 실시간 연결(업무 변경)로 바로 보이고, 채널을 열면 읽음으로 표시된다.
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { Link, useNavigate, useParams } from '../../../lib/router';
import { Modal, useModalClose } from '../../../components/Modal';
import { useToast } from '../../../components/Toast';
import { useConfirm } from '../../../components/Confirm';
import { errorMessage } from '../../../api/errors';
import { useAsync } from '../../../hooks/useAsync';
import { Glyph } from '../../../components/Glyph';
import { EmptyState, ErrorBox, PageHeader, SkeletonList, useStaff } from '../../ui';
import { when } from '../../labels';
import { workApi } from '../api';
import { MAX_FILE, MAX_FILES, MentionArea, NoteBody, Person, mentionsIn, sizeLabel, usePeople, type People } from '../ui';
import { ADMIN_WORK_EVENT } from '../../liveEvents';
import type { Channel, WorkNote } from '../types';

const dayLabel = (iso: string) => {
  const d = new Date(iso);
  const today = new Date();
  const y = new Date(); y.setDate(today.getDate() - 1);
  if (d.toDateString() === today.toDateString()) return '오늘';
  if (d.toDateString() === y.toDateString()) return '어제';
  return d.toLocaleDateString('ko-KR', { month: 'long', day: 'numeric', weekday: 'short' });
};
const timeLabel = (iso: string) => new Date(iso).toLocaleTimeString('ko-KR', { hour: 'numeric', minute: '2-digit' });

function ChannelForm({ init, onSaved }: { init: Channel | null; onSaved: (c: Channel) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [name, setName] = useState(init?.name ?? '');
  const [topic, setTopic] = useState(init?.topic ?? '');
  const [busy, setBusy] = useState(false);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim() || busy) return;
    setBusy(true);
    try {
      const c = init ? await workApi.updateChannel(init.id, { name: name.trim(), topic: topic.trim() }) : await workApi.createChannel({ name: name.trim(), topic: topic.trim() });
      toast(init ? '채널을 고쳤어요.' : '채널을 만들었어요.', 'success');
      onSaved(c);
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
        <label htmlFor="chName">채널 이름 <span className="required">*</span></label>
        <input id="chName" data-autofocus className="ui-input" maxLength={40} value={name} onChange={e => setName(e.target.value)} placeholder="예: 멜론 장애 대응" />
      </div>
      <div className="ui-field">
        <label htmlFor="chTopic">설명</label>
        <input id="chTopic" className="ui-input" maxLength={200} value={topic} onChange={e => setTopic(e.target.value)} placeholder="이 채널에서 무엇을 이야기하나요?" />
      </div>
      <div className="adm-form-actions">
        <button type="button" className="ui-btn is-soft" onClick={close}>닫기</button>
        <button type="submit" className="ui-btn is-primary" disabled={!name.trim() || busy}>{busy ? '저장 중…' : init ? '저장' : '만들기'}</button>
      </div>
    </form>
  );
}

function Message({ m, people, grouped, onChanged }: { m: WorkNote; people: People; grouped: boolean; onChanged: () => void }) {
  const { me } = useStaff();
  const toast = useToast();
  const confirm = useConfirm();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(m.body);
  const mine = m.author_id === me.user_id;
  const save = async () => {
    try { await workApi.editNote(m.id, { body: draft, mentions: mentionsIn(draft, people) }); setEditing(false); onChanged(); } catch (e) { toast(errorMessage(e, '고치지 못했어요.'), 'error'); }
  };
  const archive = async () => {
    if (!await confirm({ title: '메시지를 지울까요?', message: '대화에서 사라지지만 기록은 남아요.', confirmLabel: '지우기', danger: true })) return;
    try { await workApi.archiveNote(m.id); onChanged(); } catch (e) { toast(errorMessage(e, '지우지 못했어요.'), 'error'); }
  };
  return (
    <li className={`adm-chat-msg${grouped ? ' is-grouped' : ''}${mine ? ' is-mine' : ''}${m.mentions.includes(me.user_id) ? ' is-mention' : ''}`}>
      {!grouped && (
        <div className="adm-chat-msg-top">
          <Person id={m.author_id} people={people} />
          <time dateTime={m.created_at} title={when(m.created_at)}>{timeLabel(m.created_at)}</time>
        </div>
      )}
      <div className="adm-chat-msg-body">
        {editing ? (
          <div className="adm-w-note-edit">
            <MentionArea id={`edit-${m.id}`} value={draft} onChange={setDraft} people={people} onSubmit={save} placeholder="메시지" rows={2} enterSends />
            <div className="adm-form-actions">
              <button type="button" className="ui-btn is-soft is-sm" onClick={() => setEditing(false)}>취소</button>
              <button type="button" className="ui-btn is-primary is-sm" disabled={!draft.trim()} onClick={save}>저장</button>
            </div>
          </div>
        ) : m.body && <NoteBody text={m.body} people={people} />}
        {m.attachments.length > 0 && (
          <ul className="adm-w-files">
            {m.attachments.map(a => {
              const href = workApi.attachmentUrl(a.id);
              const inner = <><Glyph name="clip" size={13} />{a.name}<small>{sizeLabel(a.size)}</small></>;
              return <li key={a.id}>{href ? <a href={href} download>{inner}</a> : <span>{inner}</span>}</li>;
            })}
          </ul>
        )}
        {m.edited_at && !editing && <small className="adm-chat-edited">수정됨{m.revisions ? ` · 이력 ${m.revisions}` : ''}</small>}
      </div>
      {!editing && (mine || me.role === 'ADMIN') && (
        <span className="adm-chat-msg-actions">
          {mine && <button type="button" className="ui-btn is-ghost is-sm" aria-label="메시지 고치기" onClick={() => { setDraft(m.body); setEditing(true); }}><Glyph name="pencil" size={13} /></button>}
          <button type="button" className="ui-btn is-ghost is-sm" aria-label="메시지 지우기" onClick={archive}><Glyph name="close" size={13} /></button>
        </span>
      )}
    </li>
  );
}

function Composer({ channel, people, onSent }: { channel: Channel; people: People; onSent: () => void }) {
  const toast = useToast();
  const [body, setBody] = useState('');
  const [files, setFiles] = useState<File[]>([]);
  const [busy, setBusy] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);
  const addFiles = (list: FileList | null) => {
    if (!list) return;
    const picked = [...files];
    for (const f of Array.from(list)) {
      if (f.size > MAX_FILE) { toast(`${f.name}: 파일은 20MB까지 보낼 수 있어요.`, 'error'); continue; }
      if (picked.length >= MAX_FILES) { toast('한 번에 파일은 5개까지 보낼 수 있어요.', 'error'); break; }
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
      for (const f of files) attachments.push(await workApi.upload(f));
      await workApi.addNote({ target_kind: 'channel', target_id: channel.id, body: body.trim(), mentions: mentionsIn(body, people), attachments });
      setBody(''); setFiles([]);
      onSent();
    } catch (err) {
      toast(errorMessage(err, '보내지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  if (channel.archived_at) return <p className="adm-chat-closed">보관된 채널이라 새 메시지를 보낼 수 없어요.</p>;
  return (
    <form className="adm-chat-composer" onSubmit={e => { e.preventDefault(); void send(); }}>
      {files.length > 0 && (
        <ul className="adm-w-files is-pending">
          {files.map((f, i) => (
            <li key={`${f.name}-${i}`}><span><Glyph name="clip" size={13} />{f.name}<small>{sizeLabel(f.size)}</small></span>
              <button type="button" aria-label={`${f.name} 빼기`} onClick={() => setFiles(files.filter((_, j) => j !== i))}><Glyph name="close" size={12} /></button>
            </li>
          ))}
        </ul>
      )}
      <div className="adm-chat-input">
        <input ref={fileRef} type="file" multiple hidden accept=".pdf,.png,.jpg,.jpeg,.webp,.txt,.csv,.zip,.xlsx,.xls,.docx,.doc,.pptx" onChange={e => addFiles(e.target.files)} />
        <button type="button" className="ui-btn is-ghost is-sm" aria-label="파일 첨부" onClick={() => fileRef.current?.click()}><Glyph name="clip" size={17} /></button>
        <label htmlFor={`chat-${channel.id}`} className="sr-only">#{channel.name}에 메시지 보내기</label>
        <MentionArea id={`chat-${channel.id}`} value={body} onChange={setBody} people={people} onSubmit={send} rows={1} enterSends
          placeholder={`#${channel.name}에 메시지 보내기 · @이름으로 알림 · Shift+Enter 줄 바꿈`} />
        <button type="submit" className="ui-btn is-primary is-sm" disabled={busy || (!body.trim() && files.length === 0)}>{busy ? '보내는 중…' : '보내기'}</button>
      </div>
    </form>
  );
}

function Conversation({ channel, people, onRead, onEdit, onArchive }: {
  channel: Channel; people: People; onRead: () => void; onEdit: () => void; onArchive: () => void;
}) {
  const { me } = useStaff();
  const { data, error, reload } = useAsync(() => workApi.messages(channel.id), [channel.id]);
  const [older, setOlder] = useState<WorkNote[]>([]);
  const [hasMore, setHasMore] = useState<boolean | null>(null);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const scroller = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  useEffect(() => { setOlder([]); setHasMore(null); stick.current = true; }, [channel.id]);

  // 새 메시지(실시간) — 최신 페이지를 다시 받고, 화면에 있으면 읽음 표시
  useEffect(() => {
    const on = () => reload();
    window.addEventListener(ADMIN_WORK_EVENT, on);
    return () => window.removeEventListener(ADMIN_WORK_EVENT, on);
  }, [reload]);
  const latest = data?.items ?? [];
  const lastId = latest.at(-1)?.id;
  useEffect(() => {
    if (!data) return;
    if (document.visibilityState === 'visible') void workApi.readChannel(channel.id).then(onRead).catch(() => {});
  }, [lastId, channel.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const items = useMemo(() => {
    const seen = new Set(latest.map(m => m.id));
    return [...older.filter(m => !seen.has(m.id)), ...latest];
  }, [older, latest]);
  const more = hasMore ?? data?.has_more ?? false;

  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [items.length, lastId]);

  const loadOlder = async () => {
    const first = items[0];
    if (!first || loadingOlder) return;
    setLoadingOlder(true);
    const el = scroller.current;
    const before = el ? el.scrollHeight - el.scrollTop : 0;
    try {
      const r = await workApi.messages(channel.id, first.created_at);
      stick.current = false;
      setOlder(o => [...r.items, ...o]);
      setHasMore(r.has_more);
      requestAnimationFrame(() => { if (el) el.scrollTop = el.scrollHeight - before; });
    } finally {
      setLoadingOlder(false);
    }
  };

  const manage = me.role === 'ADMIN' || channel.created_by === me.user_id;
  let lastDay = '';
  let prev: WorkNote | null = null;
  return (
    <section className="adm-chat-main" aria-label={`#${channel.name}`}>
      <header className="adm-chat-head">
        <Link to="/admin/chat" className="ui-btn is-ghost is-sm adm-chat-back" aria-label="채널 목록"><Glyph name="arrow-left" size={15} /></Link>
        <span className="adm-min">
          <b>#{channel.name}</b>
          {channel.topic && <small>{channel.topic}</small>}
        </span>
        {manage && !channel.archived_at && (
          <span className="adm-w-btns">
            <button type="button" className="ui-btn is-soft is-sm" onClick={onEdit}>수정</button>
            <button type="button" className="ui-btn is-soft is-sm" onClick={onArchive}>보관</button>
          </span>
        )}
      </header>
      <div className="adm-chat-scroll" ref={scroller} onScroll={e => { const el = e.currentTarget; stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80; }} aria-live="polite">
        {error && <ErrorBox message={error} onRetry={reload} />}
        {!data && !error ? <SkeletonList rows={4} /> : (
          <>
            {more && <button type="button" className="ui-btn is-soft is-sm adm-chat-more" disabled={loadingOlder} onClick={loadOlder}>{loadingOlder ? '불러오는 중…' : '이전 메시지 더 보기'}</button>}
            {items.length === 0 ? (
              <div className="adm-chat-empty"><Glyph name="inquiry" size={22} /><p>아직 대화가 없어요. 첫 메시지를 남겨 보세요.</p></div>
            ) : (
              <ol className="adm-chat-list">
                {items.map(m => {
                  const day = dayLabel(m.created_at);
                  const divider = day !== lastDay;
                  const grouped = !divider && !!prev && prev.author_id === m.author_id && Date.parse(m.created_at) - Date.parse(prev.created_at) < 5 * 60_000;
                  lastDay = day;
                  prev = m;
                  return (
                    <FragmentRow key={m.id} divider={divider ? day : null}>
                      <Message m={m} people={people} grouped={grouped} onChanged={reload} />
                    </FragmentRow>
                  );
                })}
              </ol>
            )}
          </>
        )}
      </div>
      <Composer channel={channel} people={people} onSent={() => { stick.current = true; reload(); }} />
    </section>
  );
}

function FragmentRow({ divider, children }: { divider: string | null; children: React.ReactNode }) {
  return <>{divider && <li className="adm-chat-day" role="separator"><span>{divider}</span></li>}{children}</>;
}

export function Chat() {
  const { id } = useParams();
  const nav = useNavigate();
  const toast = useToast();
  const confirm = useConfirm();
  const { refreshCounts } = useStaff();
  const people = usePeople();
  const [archived, setArchived] = useState(false);
  const { data, loading, error, reload } = useAsync(() => workApi.channels(archived), [archived]);
  const [form, setForm] = useState<Channel | null | 'new'>(null);
  useEffect(() => {
    const on = () => reload();
    window.addEventListener(ADMIN_WORK_EVENT, on);
    return () => window.removeEventListener(ADMIN_WORK_EVENT, on);
  }, [reload]);
  const channels = data?.items ?? [];
  const current = channels.find(c => c.id === id) ?? null;
  // 채널을 고르지 않았으면 넓은 화면에서는 첫 채널을 연다
  useEffect(() => {
    if (!id && channels.length && window.matchMedia('(min-width: 900px)').matches) nav(`/admin/chat/${channels[0].id}`, { replace: true });
  }, [id, channels.length]); // eslint-disable-line react-hooks/exhaustive-deps
  const onRead = useCallback(() => { reload(); refreshCounts(); }, [reload, refreshCounts]);

  const archive = async (c: Channel) => {
    if (!await confirm({ title: `#${c.name} 채널을 보관할까요?`, message: '대화는 그대로 남고, 새 메시지는 보낼 수 없어요.', confirmLabel: '보관', danger: true })) return;
    try { await workApi.archiveChannel(c.id); toast('채널을 보관했어요.', 'info'); reload(); nav('/admin/chat'); } catch (e) { toast(errorMessage(e, '보관하지 못했어요.'), 'error'); }
  };

  return (
    <div className="view-enter">
      <PageHeader
        eyebrow="업무"
        title="팀 대화"
        sub="직원끼리 채널에서 이야기해요. @이름으로 부르면 업무함에 알림이 가고, 고객에게는 보이지 않아요."
        actions={<button type="button" className="ui-btn is-primary is-sm" onClick={() => setForm('new')}><Glyph name="plus" size={14} />채널 만들기</button>}
      />
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <SkeletonList rows={6} /> : (
        <div className={`adm-chat${id ? ' has-channel' : ''}`}>
          <nav className="adm-chat-side" aria-label="채널">
            <ul>
              {channels.map(c => (
                <li key={c.id}>
                  <Link to={`/admin/chat/${c.id}`} aria-current={c.id === id ? 'page' : undefined} className={c.unread ? 'is-unread' : ''}>
                    <span className="adm-min"><span className="adm-chat-hash">#</span>{c.name}</span>
                    {c.mentions ? <b className="ui-count is-hot">@{c.mentions}</b> : c.unread ? <b className="ui-count">{c.unread > 99 ? '99+' : c.unread}</b> : null}
                  </Link>
                </li>
              ))}
            </ul>
            {channels.length === 0 && <p className="small muted">{archived ? '보관한 채널이 없어요.' : '채널이 없어요.'}</p>}
            <button type="button" className="ui-btn is-text is-muted" onClick={() => { setArchived(a => !a); nav('/admin/chat'); }}>{archived ? '사용 중인 채널 보기' : '보관한 채널 보기'}</button>
          </nav>
          {current ? (
            <Conversation key={current.id} channel={current} people={people} onRead={onRead} onEdit={() => setForm(current)} onArchive={() => archive(current)} />
          ) : id ? <EmptyState title="채널을 찾을 수 없어요"><Link to="/admin/chat" className="ui-btn is-soft is-sm">채널 목록</Link></EmptyState>
            : <div className="adm-chat-main adm-chat-pick"><p className="muted">왼쪽에서 채널을 고르세요.</p></div>}
        </div>
      )}
      {form && (
        <Modal title={form === 'new' ? '채널 만들기' : '채널 수정'} onClose={() => setForm(null)}>
          <ChannelForm init={form === 'new' ? null : form} onSaved={c => { reload(); nav(`/admin/chat/${c.id}`); }} />
        </Modal>
      )}
    </div>
  );
}
