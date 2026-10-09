// 문서 오른쪽 패널 — 댓글(스레드·해결·위치), 정보(관련 기록·카드·역링크), 기록(버전 보기·비교·복원).
import { useMemo, useState } from 'react';
import { Link } from '../../../lib/router';
import { useToast } from '../../../components/Toast';
import { useConfirm } from '../../../components/Confirm';
import { errorMessage } from '../../../api/errors';
import { Glyph } from '../../../components/Glyph';
import { DocIcon } from './DocIcon';
import { CheckIcon } from '../../../components/Check';
import { useAsync } from '../../../hooks/useAsync';
import { ago, when } from '../../labels';
import { useStaff } from '../../ui';
import { workApi } from '../api';
import { DOC_LEVEL, DOC_STATUS, PRIORITY_CODE, TASK_STATUS } from '../labels';
import { EntityChips, MentionArea, NoteBody, Person, mentionsIn, type People } from '../ui';
import type { DocComment, DocDetail, DocNode, DocVersionBody } from '../types';
import { diffLines, diffStats } from './diff';
import { plainText } from './model';
import type { CommentAnchor } from './DocEditor';

// ---------- 댓글 ----------
function CommentItem({ c, replies, people, canWrite, onReply, onChanged, onJump }: {
  c: DocComment; replies: DocComment[]; people: People; canWrite: boolean;
  onReply: (parent: DocComment) => void; onChanged: () => void; onJump: (a: NonNullable<DocComment['anchor']>) => void;
}) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const resolve = async () => {
    setBusy(true);
    try { await workApi.resolveNote(c.id, !c.resolved); onChanged(); } catch (e) { toast(errorMessage(e, '바꾸지 못했어요.'), 'error'); } finally { setBusy(false); }
  };
  return (
    <li className={`adm-doc-comment${c.resolved ? ' is-resolved' : ''}`}>
      <div className="adm-w-note-top">
        <Person id={c.author_id} people={people} />
        <small className="muted" title={when(c.created_at)}>{ago(c.created_at)}</small>
      </div>
      {c.anchor?.quote && (
        <button type="button" className="adm-doc-comment-quote" onClick={() => c.anchor && onJump(c.anchor)} title="본문에서 보기">“{c.anchor.quote}”</button>
      )}
      <NoteBody text={c.body} people={people} />
      {replies.length > 0 && (
        <ol className="adm-doc-replies">
          {replies.map(r => (
            <li key={r.id}>
              <div className="adm-w-note-top"><Person id={r.author_id} people={people} /><small className="muted">{ago(r.created_at)}</small></div>
              <NoteBody text={r.body} people={people} />
            </li>
          ))}
        </ol>
      )}
      <div className="adm-doc-comment-actions">
        {canWrite && !c.resolved && <button type="button" className="ui-btn is-text is-muted" onClick={() => onReply(c)}>답글</button>}
        {canWrite && (
          <button type="button" className={`adm-w-resolve${c.resolved ? ' is-on' : ''}`} disabled={busy} aria-pressed={c.resolved} onClick={() => void resolve()}>
            <CheckIcon size={12} />{c.resolved ? '해결됨' : '해결'}
          </button>
        )}
      </div>
    </li>
  );
}

export function CommentsPanel({ docId, people, canWrite, anchor, onClearAnchor, onJump, onChanged }: {
  docId: string; people: People; canWrite: boolean; anchor: CommentAnchor | null; onClearAnchor: () => void;
  onJump: (a: NonNullable<DocComment['anchor']>) => void; onChanged: () => void;
}) {
  const toast = useToast();
  const { data, loading, error, reload } = useAsync(() => workApi.docComments(docId), [docId]);
  const [body, setBody] = useState('');
  const [replyTo, setReplyTo] = useState<DocComment | null>(null);
  const [showResolved, setShowResolved] = useState(false);
  const [busy, setBusy] = useState(false);
  const items = data?.items ?? [];
  const tops = items.filter(c => !c.parent_id);
  const byParent = useMemo(() => {
    const m = new Map<string, DocComment[]>();
    for (const c of items) if (c.parent_id) m.set(c.parent_id, [...(m.get(c.parent_id) ?? []), c]);
    return m;
  }, [items]);
  const shown = showResolved ? tops : tops.filter(c => !c.resolved);

  const send = async () => {
    if (busy || !body.trim()) return;
    setBusy(true);
    try {
      await workApi.addDocComment({
        target_id: docId, body: body.trim(), mentions: mentionsIn(body, people), parent_id: replyTo?.id ?? null,
        anchor: !replyTo && anchor ? { from: anchor.from, to: anchor.to, quote: anchor.quote } : null,
      });
      setBody(''); setReplyTo(null); onClearAnchor();
      reload(); onChanged();
    } catch (e) {
      toast(errorMessage(e, '댓글을 남기지 못했어요.'), 'error');
    } finally { setBusy(false); }
  };

  return (
    <section className="adm-doc-panel" aria-label="댓글">
      <div className="ui-section-head">
        <h3>댓글</h3>
        {tops.some(c => c.resolved) && <button type="button" className="ui-btn is-text is-muted" onClick={() => setShowResolved(v => !v)}>{showResolved ? '해결된 댓글 숨기기' : '해결된 댓글 보기'}</button>}
      </div>
      {error && <div className="ui-callout is-danger">{error} <button type="button" className="ui-btn is-text is-muted" onClick={reload}>다시 시도</button></div>}
      {loading && !data ? <div className="ui-skel-rows"><span /></div> : shown.length === 0 ? <p className="adm-w-notes-empty">열린 댓글이 없어요. 본문을 고른 뒤 댓글 버튼으로 위치를 남길 수 있어요.</p> : (
        <ol className="adm-doc-comments">
          {shown.map(c => <CommentItem key={c.id} c={c} replies={byParent.get(c.id) ?? []} people={people} canWrite={canWrite}
            onReply={setReplyTo} onChanged={() => { reload(); onChanged(); }} onJump={onJump} />)}
        </ol>
      )}
      {canWrite && (
        <form className="adm-w-note-form" onSubmit={e => { e.preventDefault(); void send(); }}>
          {replyTo && <p className="adm-doc-replying">답글: {replyTo.body.slice(0, 40)} <button type="button" className="ui-btn is-text is-muted" onClick={() => setReplyTo(null)}>취소</button></p>}
          {!replyTo && anchor && <p className="adm-doc-replying">위치: “{anchor.quote}” <button type="button" className="ui-btn is-text is-muted" onClick={onClearAnchor}>빼기</button></p>}
          <label htmlFor={`doc-comment-${docId}`} className="sr-only">댓글</label>
          <MentionArea id={`doc-comment-${docId}`} value={body} onChange={setBody} people={people} onSubmit={() => void send()} rows={2}
            placeholder={replyTo ? '답글 (@이름으로 알림)' : '댓글 (@이름으로 알림, Ctrl+Enter로 보내기)'} />
          <div className="adm-form-actions"><button type="submit" className="ui-btn is-primary is-sm" disabled={busy || !body.trim()}>{busy ? '남기는 중…' : '남기기'}</button></div>
        </form>
      )}
    </section>
  );
}

// ---------- 정보 ----------
export function InfoPanel({ detail, people, onNewCard, busy }: { detail: DocDetail; people: People; onNewCard: () => void; busy: boolean }) {
  const d = detail.doc;
  const r = d.review;
  return (
    <section className="adm-doc-panel" aria-label="문서 정보">
      <dl className="adm-doc-props">
        <div><dt>상태</dt><dd>{DOC_STATUS[d.status][0]}</dd></div>
        <div><dt>내 권한</dt><dd>{DOC_LEVEL[d.level]}</dd></div>
        <div><dt>공개 범위</dt><dd>{d.staff_access === 'EDITOR' ? '모든 직원 편집' : d.staff_access === 'VIEWER' ? '모든 직원 보기' : '제한됨'}</dd></div>
        <div><dt>만든 사람</dt><dd><Person id={d.created_by} people={people} /></dd></div>
        <div><dt>만든 때</dt><dd>{when(d.created_at)}</dd></div>
        <div><dt>마지막 수정</dt><dd><Person id={d.updated_by} people={people} /> · {ago(d.updated_at)}</dd></div>
        <div><dt>버전</dt><dd>v{d.version}</dd></div>
        {r.requested_by && <div><dt>검토 요청</dt><dd><Person id={r.requested_by} people={people} /> · {r.requested_at ? when(r.requested_at) : ''}</dd></div>}
        {r.approved_by && <div><dt>승인</dt><dd><Person id={r.approved_by} people={people} /> · {r.approved_at ? when(r.approved_at) : ''}</dd></div>}
        {r.rejected_by && <div><dt>반려</dt><dd><Person id={r.rejected_by} people={people} /> · {r.rejection_reason}</dd></div>}
      </dl>
      <h3>관련 기록</h3>
      {detail.related.length ? <EntityChips items={detail.related} /> : <p className="small muted">본문에 넣은 AUDENIQ 블록이 여기에 모여요.</p>}
      <div className="ui-section-head"><h3>연결된 카드</h3>
        <button type="button" className="ui-btn is-soft is-sm" disabled={busy} onClick={onNewCard}><Glyph name="plus" size={12} />카드 만들기</button>
      </div>
      {detail.cards.length ? (
        <ul className="adm-doc-cards">
          {detail.cards.map(c => (
            <li key={c.id}>
              <Link to={c.board_id ? `/admin/board/${c.board_id}?card=${c.id}` : `/admin/tasks/${c.id}`}>
                <span className="ui-tag is-mono">{c.key}</span><b>{c.title}</b>
                <small>{PRIORITY_CODE[c.priority]} · {c.column_name ?? TASK_STATUS[c.status][0]}</small>
              </Link>
            </li>
          ))}
        </ul>
      ) : <p className="small muted">연결된 카드가 없어요.</p>}
      {detail.backlinks.length > 0 && (
        <>
          <h3>이 문서를 언급한 문서</h3>
          <ul className="adm-doc-cards">{detail.backlinks.map(b => <li key={b.id}><Link to={`/admin/docs/${b.id}`}><DocIcon icon={b.icon} size={14} /> {b.title || '제목 없음'}</Link></li>)}</ul>
        </>
      )}
    </section>
  );
}

// ---------- 기록 (버전) ----------
const REASON: Record<string, string> = { AUTOSAVE: '편집', MANUAL: '저장한 버전', RESTORE: '복원 전', STATUS: '상태 변경', CREATE: '처음' };

export function VersionsPanel({ docId, current, people, canRestore, onRestore }: {
  docId: string; current: () => DocNode; people: People; canRestore: boolean; onRestore: (v: number) => void;
}) {
  const { data, loading, error, reload } = useAsync(() => workApi.docVersions(docId), [docId]);
  const [open, setOpen] = useState<DocVersionBody | null>(null);
  const [loadingV, setLoadingV] = useState<number | null>(null);
  const [compare, setCompare] = useState(true);
  const toast = useToast();
  const confirm = useConfirm();
  const { me } = useStaff();

  const view = async (v: number) => {
    setLoadingV(v);
    try { setOpen(await workApi.docVersion(docId, v)); } catch (e) { toast(errorMessage(e, '버전을 불러오지 못했어요.'), 'error'); } finally { setLoadingV(null); }
  };
  const diff = useMemo(() => (open ? diffLines(open.content_text || plainText(open.content), plainText(current())) : []), [open, current]);
  const stats = diffStats(diff);

  return (
    <section className="adm-doc-panel" aria-label="버전 기록">
      <div className="ui-section-head"><h3>버전 기록</h3><button type="button" className="ui-btn is-text is-muted" onClick={reload}>새로고침</button></div>
      <p className="small muted">편집 세션(다른 사람이 이어 쓰거나 10분 넘게 쉰 뒤), 상태 변경, 복원 때 저장돼요.</p>
      {error && <div className="ui-callout is-danger">{error}</div>}
      {loading && !data ? <div className="ui-skel-rows"><span /></div> : (
        <ol className="adm-doc-versions">
          {(data?.items ?? []).map(v => (
            <li key={v.version} className={open?.version === v.version ? 'is-open' : ''}>
              <button type="button" onClick={() => void view(v.version)} aria-expanded={open?.version === v.version}>
                <b>v{v.version}</b> <span>{v.label || REASON[v.reason] || v.reason}</span>
                <small>{v.created_by === me.user_id ? '나' : people.get(v.created_by)?.name ?? '직원'} · {when(v.created_at)}</small>
                {loadingV === v.version && <small>불러오는 중…</small>}
              </button>
            </li>
          ))}
        </ol>
      )}
      {open && (
        <div className="adm-doc-version-view">
          <div className="ui-section-head">
            <h4>v{open.version} · {open.title}</h4>
            <label className="adm-check ui-on-soft"><input type="checkbox" checked={compare} onChange={e => setCompare(e.target.checked)} />지금 내용과 비교</label>
          </div>
          {compare ? (
            <>
              <p className="small muted">+{stats.added}줄 / -{stats.removed}줄 (지금 기준)</p>
              <pre className="adm-doc-diff" aria-label="차이">
                {diff.map((l, i) => <span key={i} className={`is-${l.kind}`}>{l.kind === 'add' ? '+ ' : l.kind === 'del' ? '- ' : '  '}{l.text}{'\n'}</span>)}
              </pre>
            </>
          ) : <pre className="adm-doc-diff">{open.content_text || plainText(open.content)}</pre>}
          {canRestore && !open.current && (
            <button type="button" className="ui-btn is-soft is-sm" onClick={async () => {
              if (await confirm({ title: `v${open.version}으로 되돌릴까요?`, message: '지금 내용은 새 버전으로 남아서 언제든 다시 돌아올 수 있어요.', confirmLabel: '되돌리기' })) {
                onRestore(open.version);
                setOpen(null);
                reload();
              }
            }}>이 버전으로 되돌리기</button>
          )}
        </div>
      )}
    </section>
  );
}
