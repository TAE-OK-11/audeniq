// 거래처 (CRM Lite) — 아티스트 · 레이블 · DSP · 파트너. 담당자, 마지막 연락, 다음 follow-up, 계약 진행 상태, 연락 기록.
import { useEffect, useRef, useState } from 'react';
import { Link, useNavigate, useParams, useSearchParams } from '../../../lib/router';
import { Modal, useModalClose } from '../../../components/Modal';
import { useToast } from '../../../components/Toast';
import { errorMessage } from '../../../api/errors';
import { useAsync } from '../../../hooks/useAsync';
import { Glyph } from '../../../components/Glyph';
import { Chip, Empty, ErrorBox, Filters, Initial, PageHead, Seg, Skeleton, StatusChip, useStaff } from '../../ui';
import { ago, day, when } from '../../labels';
import { workApi } from '../api';
import { ACTIVITY_KIND, CONTACT_KIND, CONTACT_KINDS, CONTRACT_STAGE, CONTRACT_STAGES } from '../labels';
import { TaskForm, fromLocalInput, toLocalInput } from '../TaskForm';
import { MiniStats, NotesPanel, Person, PersonSelect, PriorityChip, TaskStatusChip, usePeople } from '../ui';
import type { ActivityKind, Contact, ContactInput, ContactKind, ContractStage } from '../types';

const isDue = (c: Contact) => !!c.next_follow_up_at && Date.parse(c.next_follow_up_at) <= Date.now() + 86_400_000;
const isLate = (c: Contact) => !!c.next_follow_up_at && Date.parse(c.next_follow_up_at) < Date.now();

function ContactForm({ onSaved }: { onSaved: (c: Contact) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const { me } = useStaff();
  const people = usePeople();
  const [v, setV] = useState<ContactInput>({ kind: 'ARTIST', name: '', company: '', person: '', email: '', phone: '', owner_id: me.user_id, next_follow_up_at: null, contract_stage: 'NONE', tags: [], memo: '' });
  const [busy, setBusy] = useState(false);
  const set = <K extends keyof ContactInput>(k: K, val: ContactInput[K]) => setV(o => ({ ...o, [k]: val }));
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy || !v.name.trim()) return;
    setBusy(true);
    try {
      const c = await workApi.createContact({ ...v, name: v.name.trim() });
      toast(`${c.name}을(를) 거래처로 등록했어요.`, 'success');
      onSaved(c);
      close();
    } catch (err) {
      toast(errorMessage(err, '등록하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <form onSubmit={submit}>
      <div className="adm-field">
        <span className="adm-field-label">구분</span>
        <Seg label="구분" value={v.kind} onChange={k => set('kind', k)} options={CONTACT_KINDS.map(k => [k, CONTACT_KIND[k]] as const)} />
      </div>
      <div className="adm-w-grid2">
        <div className="adm-field"><label htmlFor="cName">이름 <span className="required">*</span></label><input id="cName" data-autofocus className="adm-input" maxLength={120} value={v.name} onChange={e => set('name', e.target.value)} placeholder="예: Lumi Records" /></div>
        <div className="adm-field"><label htmlFor="cCompany">회사</label><input id="cCompany" className="adm-input" maxLength={120} value={v.company} onChange={e => set('company', e.target.value)} /></div>
        <div className="adm-field"><label htmlFor="cPerson">상대 담당자</label><input id="cPerson" className="adm-input" maxLength={120} value={v.person} onChange={e => set('person', e.target.value)} placeholder="예: 정하늘 대표" /></div>
        <div className="adm-field"><label htmlFor="cOwner">우리 담당자</label><PersonSelect id="cOwner" value={v.owner_id} onChange={o => set('owner_id', o)} people={people} empty="담당자 없음" /></div>
        <div className="adm-field"><label htmlFor="cEmail">이메일</label><input id="cEmail" type="email" className="adm-input" maxLength={200} value={v.email} onChange={e => set('email', e.target.value)} /></div>
        <div className="adm-field"><label htmlFor="cPhone">전화</label><input id="cPhone" type="tel" className="adm-input" maxLength={40} value={v.phone} onChange={e => set('phone', e.target.value)} /></div>
        <div className="adm-field"><label htmlFor="cStage">계약 상태</label>
          <select id="cStage" className="adm-select adm-w-select" value={v.contract_stage} onChange={e => set('contract_stage', e.target.value as ContractStage)}>{CONTRACT_STAGES.map(s => <option key={s} value={s}>{CONTRACT_STAGE[s][0]}</option>)}</select>
        </div>
        <div className="adm-field"><label htmlFor="cFollow">다음 follow-up</label><input id="cFollow" type="datetime-local" className="adm-input" value={toLocalInput(v.next_follow_up_at)} onChange={e => set('next_follow_up_at', fromLocalInput(e.target.value))} /></div>
      </div>
      <div className="adm-field"><label htmlFor="cMemo">메모</label><textarea id="cMemo" className="adm-textarea" rows={3} maxLength={2000} value={v.memo} onChange={e => set('memo', e.target.value)} /></div>
      <div className="adm-form-actions">
        <button type="button" className="adm-btn soft" onClick={close}>취소</button>
        <button type="submit" className="adm-btn primary" disabled={busy || !v.name.trim()}>{busy ? '등록 중…' : '거래처 등록'}</button>
      </div>
    </form>
  );
}

function ContactPane({ id, onChanged }: { id: string; onChanged: () => void }) {
  const toast = useToast();
  const people = usePeople();
  const { refreshWork } = useStaff();
  const { data, loading, error, reload } = useAsync(() => workApi.contact(id), [id]);
  const [kind, setKind] = useState<ActivityKind>('CALL');
  const [summary, setSummary] = useState('');
  const [follow, setFollow] = useState('');
  const [busy, setBusy] = useState(false);
  const [newTask, setNewTask] = useState(false);
  const paneRef = useRef<HTMLElement>(null);
  useEffect(() => { setSummary(''); setFollow(''); }, [id]);
  const loaded = !!data;
  useEffect(() => {
    if (!loaded || !window.matchMedia?.('(max-width: 960px)').matches) return;
    paneRef.current?.scrollIntoView({ block: 'start', behavior: 'smooth' });
  }, [id, loaded]);

  if (loading && !data) return <div className="adm-thread-pane"><Skeleton rows={3} /></div>;
  if (error || !data) return <div className="adm-thread-pane"><ErrorBox message={error || '거래처를 불러오지 못했어요.'} onRetry={reload} /></div>;
  const c = data.contact;

  const update = async (body: Partial<ContactInput>, msg: string) => {
    try { await workApi.updateContact(c.id, body); toast(msg, 'success'); reload(); onChanged(); refreshWork(); } catch (err) { toast(errorMessage(err, '바꾸지 못했어요.'), 'error'); }
  };
  const log = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy || !summary.trim()) return;
    setBusy(true);
    try {
      await workApi.addActivity(c.id, { kind, summary: summary.trim(), next_follow_up_at: follow ? fromLocalInput(follow) : undefined });
      toast('연락 기록을 남겼어요.', 'success');
      setSummary(''); setFollow('');
      reload(); onChanged(); refreshWork();
    } catch (err) {
      toast(errorMessage(err, '기록하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="adm-thread-pane adm-w-contact" aria-label="거래처 상세" ref={paneRef}>
      <div className="adm-w-contact-head">
        <Initial text={c.name} plain />
        <span className="adm-min">
          <span className="adm-codes"><Chip tone="gray">{CONTACT_KIND[c.kind]}</Chip><StatusChip value={CONTRACT_STAGE[c.contract_stage]} />{isLate(c) && <Chip tone="red">follow-up 지남</Chip>}</span>
          <h2>{c.name}</h2>
          <span className="adm-row-meta">{c.company && c.company !== c.name && <span>{c.company}</span>}{c.person && <span>{c.person}</span>}{c.dsp_code && <span><Link to={`/admin/dsp-ops?dsp=${c.dsp_code}`}>DSP 운영 보기</Link></span>}</span>
        </span>
      </div>

      <dl className="adm-kv adm-w-contact-kv">
        <div><dt>이메일</dt><dd>{c.email ? <a href={`mailto:${c.email}`}>{c.email}</a> : '—'}</dd></div>
        <div><dt>전화</dt><dd>{c.phone ? <a href={`tel:${c.phone}`}>{c.phone}</a> : '—'}</dd></div>
        <div><dt>마지막 연락</dt><dd>{c.last_contact_at ? ago(c.last_contact_at) : '기록 없음'}</dd></div>
        <div><dt><label htmlFor="cpOwner">우리 담당자</label></dt><dd><PersonSelect id="cpOwner" value={c.owner_id} onChange={v => update({ owner_id: v }, '담당자를 바꿨어요.')} people={people} empty="담당자 없음" /></dd></div>
        <div><dt><label htmlFor="cpStage">계약 상태</label></dt><dd>
          <select id="cpStage" className="adm-select adm-w-select" value={c.contract_stage} onChange={e => update({ contract_stage: e.target.value as ContractStage }, '계약 상태를 바꿨어요.')}>{CONTRACT_STAGES.map(s => <option key={s} value={s}>{CONTRACT_STAGE[s][0]}</option>)}</select>
        </dd></div>
        <div><dt><label htmlFor="cpFollow">다음 follow-up</label></dt><dd>
          <input id="cpFollow" type="datetime-local" className="adm-input" key={c.next_follow_up_at ?? 'none'} defaultValue={toLocalInput(c.next_follow_up_at)} onBlur={e => { const v = fromLocalInput(e.target.value); if (v !== c.next_follow_up_at) void update({ next_follow_up_at: v }, 'follow-up 일정을 바꿨어요.'); }} />
        </dd></div>
      </dl>
      {c.memo && <p className="adm-w-desc adm-w-contact-memo">{c.memo}</p>}

      <div className="adm-section-top" style={{ marginTop: 22 }}><h2>연락 기록<small>{data.activities.length}건</small></h2></div>
      <form className="adm-w-activity-form" onSubmit={log}>
        <Seg label="연락 방식" value={kind} onChange={setKind} options={(['CALL', 'EMAIL', 'MEETING', 'NOTE'] as const).map(k => [k, ACTIVITY_KIND[k]] as const)} />
        <textarea className="adm-textarea" aria-label="내용" rows={2} maxLength={2000} value={summary} onChange={e => setSummary(e.target.value)} placeholder="무슨 얘기를 했는지 짧게 남겨요." />
        <div className="adm-reply-foot">
          <label className="adm-w-inline"><span>다음 follow-up</span><input type="datetime-local" className="adm-input" value={follow} onChange={e => setFollow(e.target.value)} /></label>
          <button type="submit" className="adm-btn primary" disabled={busy || !summary.trim()}>{busy ? '기록 중…' : '기록 남기기'}</button>
        </div>
      </form>
      {data.activities.length > 0 && (
        <ol className="adm-timeline" style={{ marginTop: 14 }}>
          {data.activities.map(a => (
            <li key={a.id} className="is-staff"><i aria-hidden="true" /><div><div className="adm-tl-top"><b>{ACTIVITY_KIND[a.kind]}</b></div><p className="adm-tl-summary">{a.summary}</p><small>{people.get(a.by)?.name ?? '담당자'} · {when(a.at)}</small></div></li>
          ))}
        </ol>
      )}

      <div className="adm-section-top" style={{ marginTop: 22 }}>
        <h2>관련 업무<small>{data.tasks.length}건</small></h2>
        <button type="button" className="adm-btn soft small" onClick={() => setNewTask(true)}><Glyph name="plus" size={14} />업무 만들기</button>
      </div>
      {data.tasks.length === 0 ? <p className="small muted">연결된 업무가 없어요.</p> : (
        <div className="adm-list">
          {data.tasks.map(t => (
            <Link key={t.id} to={`/admin/tasks/${t.id}`} className="adm-row adm-w-task-row is-compact">
              <Person id={t.assignee_id} people={people} compact />
              <span className="adm-min"><span className="adm-row-title"><span className="adm-w-key">{t.key}</span>{t.title}</span></span>
              <span className="adm-codes"><PriorityChip value={t.priority} /><TaskStatusChip value={t.status} /></span>
            </Link>
          ))}
        </div>
      )}

      <div style={{ marginTop: 22 }}><NotesPanel kind="contact" targetId={c.id} /></div>

      {newTask && (
        <Modal title="새 업무" onClose={() => setNewTask(false)} dismissible={false} modalClass="adm-w-modal">
          <TaskForm initial={{ type: c.kind === 'DSP' ? 'DSP_INQUIRY' : 'CONTRACT', title: `${c.name} `, related: [{ kind: 'contact', id: c.id, label: c.name }] }} onSaved={() => { reload(); refreshWork(); }} />
        </Modal>
      )}
    </section>
  );
}

export function Crm() {
  const { id } = useParams<{ id: string }>();
  const nav = useNavigate();
  const [params, setParams] = useSearchParams();
  const kind = (params.get('kind') as ContactKind | null) ?? '';
  const [q, setQ] = useState('');
  const [onlyDue, setOnlyDue] = useState<'all' | 'due'>('all');
  const [creating, setCreating] = useState(false);
  const people = usePeople();
  const { data, loading, error, reload } = useAsync(() => workApi.contacts(kind, '', onlyDue === 'due'), [kind, onlyDue]);
  const term = q.trim().toLowerCase();
  const items = (data?.items ?? []).filter(c => !term || [c.name, c.company, c.person, c.email, ...c.tags].some(v => v.toLowerCase().includes(term)));
  const qsKind = kind ? `?kind=${kind}` : '';

  return (
    <div className="view-enter">
      <PageHead
        eyebrow="운영"
        title="거래처"
        sub="아티스트·레이블·DSP·파트너와의 마지막 연락, 다음 follow-up, 계약 진행 상태를 가볍게 관리해요."
        actions={<>
          <button type="button" className="adm-btn soft small" onClick={reload}>새로고침</button>
          <button type="button" className="adm-btn primary small" onClick={() => setCreating(true)}><Glyph name="plus" size={14} />거래처 등록</button>
        </>}
      />
      <MiniStats items={[
        { label: 'follow-up 지남', value: items.filter(isLate).length, tone: items.some(isLate) ? 'alert' : 'muted', onClick: () => setOnlyDue('due'), active: onlyDue === 'due' },
        { label: '계약 진행 중', value: items.filter(c => ['NEGOTIATING', 'SENT', 'RENEWAL'].includes(c.contract_stage)).length, hint: '협의·발송·갱신' },
        { label: '30일 넘게 연락 없음', value: items.filter(c => !c.last_contact_at || Date.now() - Date.parse(c.last_contact_at) > 30 * 86_400_000).length, tone: 'warn' },
        { label: '전체 거래처', value: items.length, onClick: () => setOnlyDue('all'), active: onlyDue === 'all' },
      ]} />
      <Filters label="구분" value={kind} onChange={v => setParams(v ? { kind: v } : {}, { replace: true })} options={[{ value: '' as ContactKind | '', label: '전체' }, ...CONTACT_KINDS.map(k => ({ value: k as ContactKind | '', label: CONTACT_KIND[k] }))]} />
      <div className="adm-queue-tools">
        <label htmlFor="cSearch" className="sr-only">검색</label>
        <input id="cSearch" type="search" className="adm-input" placeholder="이름·회사·담당자·태그로 찾기" value={q} onChange={e => setQ(e.target.value)} />
        <Seg label="보기" value={onlyDue} onChange={setOnlyDue} options={[['all', '전체'], ['due', 'follow-up 할 곳']] as const} />
      </div>
      {error && <ErrorBox message={error} onRetry={reload} />}
      <div className="adm-split">
        <div className="adm-list">
          {loading && !data ? <Skeleton rows={4} /> : items.length === 0 ? (
            <Empty icon={<Glyph name="users" size={22} />} title={onlyDue === 'due' ? '오늘 follow-up 할 곳이 없어요' : '거래처가 없어요'} />
          ) : items.map(c => (
            <button key={c.id} type="button" className={`adm-row${c.id === id ? ' is-active' : ''}`} onClick={() => nav(`/admin/crm/${c.id}${qsKind}`, { replace: true })} style={{ gridTemplateColumns: 'minmax(0,1fr) auto' }}>
              <span className="adm-min">
                <span className="adm-row-title">{c.name}</span>
                <span className="adm-row-meta"><span>{CONTACT_KIND[c.kind]}</span><span>{people.get(c.owner_id)?.name ?? '담당자 없음'}</span><span>연락 {c.last_contact_at ? ago(c.last_contact_at) : '없음'}</span></span>
              </span>
              <span className="adm-row-end">
                <StatusChip value={CONTRACT_STAGE[c.contract_stage]} />
                {c.next_follow_up_at && <small className={isDue(c) ? 'is-late' : undefined}>다음 {isLate(c) ? `${ago(c.next_follow_up_at)} 지남`.replace(' 전 지남', ' 지남') : day(c.next_follow_up_at)}</small>}
              </span>
            </button>
          ))}
        </div>
        {id ? <ContactPane id={id} onChanged={reload} /> : (
          <div className="adm-thread-pane adm-thread-empty" style={{ justifyContent: 'center' }}>
            <Empty icon={<Glyph name="users" size={22} />} title="거래처를 선택해 주세요">목록에서 거래처를 누르면 연락 기록과 관련 업무가 열려요.</Empty>
          </div>
        )}
      </div>
      {creating && (
        <Modal title="거래처 등록" onClose={() => setCreating(false)} dismissible={false} modalClass="adm-w-modal">
          <ContactForm onSaved={c => { reload(); nav(`/admin/crm/${c.id}`); }} />
        </Modal>
      )}
    </div>
  );
}
