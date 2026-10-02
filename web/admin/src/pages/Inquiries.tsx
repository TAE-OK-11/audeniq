// 문의 답변 — 작업 공간 전체의 문의를 보고 담당자 답변을 남긴다 (답변하면 ANSWERED + 작성자 알림).
import { useEffect, useRef, useState } from 'react';
import { Link, useNavigate, useParams, useSearchParams } from '../lib/router';
import { useToast } from '../components/Toast';
import { errorMessage } from '../api/errors';
import { useAsync } from '../hooks/useAsync';
import { staffApi } from '../api/staff';
import { INQUIRY_STATUS, ago, pick, when } from '../labels';
import { Empty, ErrorBox, Filters, NoDuty, PageHead, Skeleton, StatusChip, useStaff } from '../ui';
import { Glyph } from '../components/Glyph';

const STATUSES = ['OPEN', 'ANSWERED', 'CLOSED'] as const;
const TEMPLATES: { label: string; text: string }[] = [
  { label: '인사', text: '안녕하세요, AUDENIQ입니다. 문의 주셔서 감사합니다.\n\n' },
  { label: '확인 중', text: '말씀하신 내용은 담당 부서에서 확인하고 있어요. 확인되는 대로 이 문의로 다시 안내드릴게요.' },
  { label: '심사 일정', text: '현재 담당자 심사 단계에 있어요. 보통 영업일 기준 1~2일 안에 결과를 알려 드려요. 발매 예정일이 가까우면 이 문의에 남겨 주세요.' },
  { label: '마무리', text: '\n\n더 궁금한 점이 있으면 이 문의에 이어서 남겨 주세요. 감사합니다.' },
];

function Thread({ id, onReplied }: { id: string; onReplied: () => void }) {
  const toast = useToast();
  const { can } = useStaff();
  const { data, loading, error, reload } = useAsync(() => staffApi.inquiry(id), [id]);
  const [body, setBody] = useState('');
  const [busy, setBusy] = useState(false);
  const listRef = useRef<HTMLOListElement>(null);

  const paneRef = useRef<HTMLElement>(null);
  useEffect(() => { setBody(''); }, [id]);
  // 목록 아래에 대화가 쌓이는 좁은 화면에서는 문의를 누르면 대화로 내려간다
  const loaded = !!data;
  useEffect(() => {
    if (!loaded || !window.matchMedia?.('(max-width: 960px)').matches) return;
    paneRef.current?.scrollIntoView({ block: 'start', behavior: 'smooth' });
  }, [id, loaded]);
  useEffect(() => {
    const el = listRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [data]);

  const send = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!body.trim() || busy) return;
    setBusy(true);
    try {
      await staffApi.reply(id, body.trim());
      setBody('');
      toast('답변을 보냈어요. 작성자에게 알림이 갔어요.', 'success');
      reload();
      onReplied();
    } catch (err) {
      toast(errorMessage(err, '답변을 보내지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };

  if (loading && !data) return <div className="adm-thread-pane"><Skeleton rows={3} /></div>;
  if (error || !data) return <div className="adm-thread-pane"><ErrorBox message={error || '문의를 불러오지 못했어요.'} onRetry={reload} /></div>;
  const q = data.inquiry;
  const closed = q.status === 'CLOSED';

  return (
    <section className="adm-thread-pane" aria-label="문의 대화" ref={paneRef}>
      <div className="adm-thread-head">
        <div className="adm-check-top">
          <StatusChip value={pick(INQUIRY_STATUS, q.status)} />
          <small className="muted">{when(q.created_at)}</small>
        </div>
        <h2 style={{ marginTop: 8 }}>{q.subject}</h2>
        <span className="adm-row-meta">
          <span>{q.org_name}</span><span>{q.category}</span>
          {q.release_id && <span><Link to={`/reviews/${q.release_id}`}>관련 발매 <Glyph name="arrow-right" size={13} className="aq-inline-glyph" /></Link></span>}
        </span>
      </div>
      <ol className="adm-thread" ref={listRef}>
        {data.messages.map(m => (
          <li key={m.id} className={`adm-msg${m.author_kind === 'STAFF' ? ' is-staff' : ''}`}>
            <span className="adm-msg-who">{m.author_kind === 'STAFF' ? 'AUDENIQ 담당자' : q.org_name} · {when(m.created_at)}</span>
            <p>{m.body}</p>
          </li>
        ))}
      </ol>
      {closed ? (
        <div className="adm-alert">작성자가 종료한 문의예요. 답변을 남길 수 없어요.</div>
      ) : !can('INQUIRIES') ? (
        <NoDuty duty="문의 답변" />
      ) : (
        <form className="adm-reply" onSubmit={send}>
          <div className="adm-templates" aria-label="자주 쓰는 문구">
            {TEMPLATES.map(t => <button key={t.label} type="button" onClick={() => setBody(b => (t.label === '인사' ? t.text + b : b + t.text))}>{t.label}</button>)}
          </div>
          <label htmlFor="replyBody" className="sr-only">답변</label>
          <textarea
            id="replyBody" className="adm-textarea" rows={5} maxLength={4000} value={body} placeholder="답변을 입력하세요. 아티스트의 문의 화면과 알림으로 전달돼요."
            onChange={e => setBody(e.target.value)}
            onKeyDown={e => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) void send(e); }}
          />
          <div className="adm-reply-foot">
            <small className="muted">{body.length} / 4000<span className="adm-kbd-hint"> · ⌘/Ctrl + Enter로 보내기</span></small>
            <button type="submit" className="adm-btn primary" disabled={busy || !body.trim()}>{busy ? '보내는 중…' : q.status === 'ANSWERED' ? '추가 답변' : '답변 보내기'}</button>
          </div>
        </form>
      )}
    </section>
  );
}

export function Inquiries() {
  const { id } = useParams<{ id: string }>();
  const nav = useNavigate();
  const [params] = useSearchParams();
  const status = (params.get('status') as typeof STATUSES[number]) || 'OPEN';
  const { refreshCounts } = useStaff();
  const { data, loading, error, reload } = useAsync(() => staffApi.inquiries(status), [status]);
  const items = data?.items ?? [];
  const go = (to: string) => nav(`${to}?status=${status}`, { replace: true });

  return (
    <div className="view-enter">
      <PageHead
        eyebrow="INQUIRIES"
        title="문의 답변"
        sub="모든 작업 공간의 문의예요. 답변을 보내면 ‘답변 완료’로 바뀌고 작성자에게 알림이 가요. 오래 기다린 문의부터 보여요."
        actions={<button type="button" className="adm-btn soft small" onClick={reload}>새로고침</button>}
      />
      <Filters label="문의 상태" value={status} onChange={v => nav(`/inquiries?status=${v}`, { replace: true })} options={STATUSES.map(s => ({ value: s, label: INQUIRY_STATUS[s][0] }))} />
      {error && <ErrorBox message={error} onRetry={reload} />}
      <div className="adm-split">
        <div className="adm-list">
          {loading && !data ? <Skeleton rows={4} /> : items.length === 0 ? (
            <Empty icon={<Glyph name="mail" size={22} />} title={status === 'OPEN' ? '답변 대기 중인 문의가 없어요' : '해당 상태의 문의가 없어요'} />
          ) : items.map(q => (
            <button key={q.id} type="button" className={`adm-row${q.id === id ? ' is-active' : ''}`} onClick={() => go(`/inquiries/${q.id}`)} style={{ gridTemplateColumns: 'minmax(0,1fr) auto' }}>
              <span className="adm-min">
                <span className="adm-row-title">{q.subject}</span>
                <span className="adm-row-meta"><span>{q.org_name}</span><span>{q.category}</span></span>
              </span>
              <span className="adm-row-end">
                <StatusChip value={pick(INQUIRY_STATUS, q.status)} />
                <small>{ago(q.updated_at ?? q.created_at)}</small>
              </span>
            </button>
          ))}
        </div>
        {id ? (
          <Thread id={id} onReplied={() => { reload(); refreshCounts(); }} />
        ) : (
          <div className="adm-thread-pane adm-thread-empty" style={{ justifyContent: 'center' }}>
            <Empty icon={<Glyph name="mail" size={22} />} title="문의를 선택해 주세요">왼쪽 목록에서 문의를 누르면 대화와 답변 입력창이 열려요.</Empty>
          </div>
        )}
      </div>
    </div>
  );
}
