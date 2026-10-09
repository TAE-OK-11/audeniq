// 관계 관리(CRM Lite) — 아티스트·레이블·DSP·작업 공간·파트너. 이름·계약 상태·발매는 원천에서 읽고,
// 여기서는 담당자·상태·다음 연락일·연락처·연락 이력만 기록한다. 관련 기록은 연결, 메모는 내부 메모.
import { useState } from 'react';
import { Link, useNavigate, useParams, useSearchParams } from '../../../lib/router';
import { Modal, useModalClose } from '../../../components/Modal';
import { useToast } from '../../../components/Toast';
import { useConfirm } from '../../../components/Confirm';
import { errorMessage } from '../../../api/errors';
import { useAsync } from '../../../hooks/useAsync';
import { Glyph } from '../../../components/Glyph';
import { Badge, EmptyState, ErrorBox, ChipGroup, PageHeader, Section, SkeletonList, StatusBadge, useStaff, WorkTabs } from '../../ui';
import { DOC_STATUS, RELEASE_STATUS, ago, when } from '../../labels';
import { workApi } from '../api';
import {
  ACTIVITY_KIND, ACTIVITY_KINDS, CONTRACT_STATUS, CONTRACT_STATUSES, ENTITY_KIND, REL_ENTITY, REL_KIND, REL_KINDS, REL_STATUS, REL_STATUSES, entityLink,
} from '../labels';
import { fromLocalInput, toLocalInput } from '../TaskForm';
import { DueChip, EntityChips, EntityPicker, MiniStats, NotesPanel, Person, PersonSelect, usePeople, type People } from '../ui';
import type {
  ActivityKind, ContactInput, ContractStatus, EntityRef, RelContact, RelKind, RelQuery, RelStatus, Relationship, RelationshipDetail,
} from '../types';
import { DateTimeField } from '../../../components/Picker';

const contractChip = (r: Relationship) => <StatusBadge value={CONTRACT_STATUS[r.contract.status]} />;
const name = (r: Relationship) => r.name ?? (r.missing ? '원천 기록이 없어요' : '이름 없음');

function Row({ r, people }: { r: Relationship; people: People }) {
  return (
    <Link to={`/admin/crm/${r.id}`} className="ui-row adm-w-task-row">
      <Person id={r.owner_id} people={people} compact />
      <span className="adm-min">
        <span className="ui-row-title"><Badge tone="gray">{REL_KIND[r.kind].label}</Badge>&nbsp;{name(r)}</span>
        <span className="ui-row-meta">
          {r.primary_contact && <span>{r.primary_contact.name}{r.primary_contact.role ? ` · ${r.primary_contact.role}` : ''}</span>}
          <span>{r.last_contact_at ? `마지막 연락 ${ago(r.last_contact_at)}` : '연락 기록 없음'}</span>
          {r.open_tasks > 0 && <span className="adm-w-meta-icon"><Glyph name="task" size={12} />업무 {r.open_tasks}</span>}
          {r.note_count > 0 && <span className="adm-w-meta-icon"><Glyph name="inquiry" size={12} />메모 {r.note_count}</span>}
          {r.tags.map(t => <span key={t}>#{t}</span>)}
        </span>
      </span>
      <span className="ui-row-end">
        <span className="adm-codes"><StatusBadge value={REL_STATUS[r.status]} />{contractChip(r)}</span>
        {r.next_contact_at && <DueChip due={r.next_contact_at} />}
      </span>
    </Link>
  );
}

function NewRelationship({ onSaved }: { onSaved: (d: RelationshipDetail) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const { me } = useStaff();
  const people = usePeople();
  const [kind, setKind] = useState<RelKind>('ORG');
  const [entity, setEntity] = useState<EntityRef | null>(null);
  const [own, setOwn] = useState('');
  const [owner, setOwner] = useState<string | null>(me.user_id);
  const [status, setStatus] = useState<RelStatus>('ACTIVE');
  const [contract, setContract] = useState<ContractStatus>('NONE');
  const [next, setNext] = useState('');
  const [busy, setBusy] = useState(false);
  const ek = REL_ENTITY[kind];
  const ready = ek ? !!entity : !!own.trim();
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!ready || busy) return;
    setBusy(true);
    try {
      const d = await workApi.createRelationship({
        kind, entity_kind: ek && entity ? entity.kind : null, entity_id: ek && entity ? entity.id : null, name: ek ? null : own.trim(),
        owner_id: owner, status, next_contact_at: fromLocalInput(next), manual_contract_status: ek ? null : contract,
      });
      toast('관계를 등록했어요.', 'success');
      onSaved(d);
      close();
    } catch (err) {
      toast(errorMessage(err, '등록하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <form onSubmit={submit} className="adm-w-form">
      <div className="ui-field">
        <span className="ui-field-label">종류</span>
        <div className="ui-chips" role="group" aria-label="종류">
          {REL_KINDS.map(k => <button key={k} type="button" className="ui-chip" aria-pressed={kind === k} onClick={() => { setKind(k); setEntity(null); }}>{REL_KIND[k].label}</button>)}
        </div>
      </div>
      {ek ? (
        <div className="ui-field">
          <span className="ui-field-label">{ENTITY_KIND[ek].label} <span className="required">*</span></span>
          {entity ? (
            <EntityChips items={[entity]} onRemove={() => setEntity(null)} />
          ) : <EntityPicker key={ek} kinds={[ek]} onPick={setEntity} />}
          <small className="muted">이름·계약 상태·발매는 원천 기록에서 읽어요.</small>
        </div>
      ) : (
        <div className="adm-w-grid2">
          <div className="ui-field">
            <label htmlFor="relName">이름 <span className="required">*</span></label>
            <input id="relName" data-autofocus className="ui-input" maxLength={120} value={own} onChange={e => setOwn(e.target.value)} placeholder="예: 모노 퍼블리싱" />
          </div>
          <div className="ui-field">
            <label htmlFor="relContract">계약 상태</label>
            <select id="relContract" className="ui-select" value={contract} onChange={e => setContract(e.target.value as ContractStatus)}>
              {CONTRACT_STATUSES.map(c => <option key={c} value={c}>{CONTRACT_STATUS[c][0]}</option>)}
            </select>
          </div>
        </div>
      )}
      <div className="adm-w-grid2">
        <div className="ui-field"><label htmlFor="relOwner">담당자</label><PersonSelect id="relOwner" value={owner} onChange={setOwner} people={people} /></div>
        <div className="ui-field">
          <label htmlFor="relStatus">상태</label>
          <select id="relStatus" className="ui-select" value={status} onChange={e => setStatus(e.target.value as RelStatus)}>
            {REL_STATUSES.map(s => <option key={s} value={s}>{REL_STATUS[s][0]}</option>)}
          </select>
        </div>
        <div className="ui-field"><label htmlFor="relNext">다음 연락</label><DateTimeField id="relNext" className="ui-input" value={next} onChange={setNext} /></div>
      </div>
      <div className="adm-form-actions">
        <button type="button" className="ui-btn is-soft" onClick={close}>취소</button>
        <button type="submit" className="ui-btn is-primary" disabled={!ready || busy}>{busy ? '등록 중…' : '등록'}</button>
      </div>
    </form>
  );
}

export function Crm() {
  const nav = useNavigate();
  const people = usePeople();
  const [params, setParams] = useSearchParams();
  const kind = (params.get('kind') as RelKind | null) ?? '';
  const [status, setStatus] = useState<RelStatus | ''>('');
  const [contact, setContact] = useState<NonNullable<RelQuery['contact']>>('');
  const [owner, setOwner] = useState<string | null>(null);
  const [q, setQ] = useState('');
  const [archived, setArchived] = useState(false);
  const [creating, setCreating] = useState(false);
  const { data, loading, error, reload } = useAsync(
    () => workApi.relationships({ kind, status, contact, owner: owner ?? undefined, q: q.trim() || undefined, archived }),
    [kind, status, contact, owner, q, archived],
  );
  const items = data?.items ?? [];
  const counts = data?.counts ?? {};
  const setKind = (v: RelKind | '') => { const next = new URLSearchParams(params); if (v) next.set('kind', v); else next.delete('kind'); setParams(next, { replace: true }); };
  const total = Object.values(counts).reduce((a, b) => a + b, 0);
  const due = items.filter(r => r.contact_due).length;
  const atRisk = items.filter(r => r.status === 'AT_RISK').length;

  return (
    <div className="view-enter">
      <PageHeader
        eyebrow="업무"
        title="관계 관리"
        sub="아티스트·레이블·DSP·파트너마다 담당자, 연락처, 연락 이력, 다음 연락일을 관리해요. 이름과 계약 상태는 원천 기록에서 읽어요."
        actions={<>
          <button type="button" className="ui-btn is-soft is-sm" onClick={reload}>새로고침</button>
          <button type="button" className="ui-btn is-primary is-sm" onClick={() => setCreating(true)}><Glyph name="plus" size={14} />관계 등록</button>
        </>}
      />
      <WorkTabs />
      <MiniStats items={[
        { label: '전체', value: total, onClick: () => setKind(''), active: !kind },
        { label: '연락할 때', value: due, tone: due ? 'warn' : 'muted', onClick: () => setContact(contact === 'due' ? '' : 'due'), active: contact === 'due' },
        { label: '주의', value: atRisk, tone: atRisk ? 'alert' : 'muted', onClick: () => setStatus(status === 'AT_RISK' ? '' : 'AT_RISK'), active: status === 'AT_RISK' },
        { label: 'DSP', value: counts.DSP ?? 0, onClick: () => setKind('DSP'), active: kind === 'DSP' },
      ]} />
      <ChipGroup label="종류" value={kind} onChange={setKind}
        options={[{ value: '' as RelKind | '', label: '전체', count: total }, ...REL_KINDS.map(k => ({ value: k as RelKind | '', label: REL_KIND[k].label, count: counts[k] ?? 0 }))]} />
      <div className="adm-queue-tools adm-w-filter-row">
        <label htmlFor="crmSearch" className="sr-only">검색</label>
        <input id="crmSearch" type="search" className="ui-input" placeholder="이름·태그로 찾기" value={q} onChange={e => setQ(e.target.value)} />
        <select className="ui-select is-auto" aria-label="상태" value={status} onChange={e => setStatus(e.target.value as RelStatus | '')}>
          <option value="">모든 상태</option>
          {REL_STATUSES.map(s => <option key={s} value={s}>{REL_STATUS[s][0]}</option>)}
        </select>
        <select className="ui-select is-auto" aria-label="다음 연락" value={contact} onChange={e => setContact(e.target.value as NonNullable<RelQuery['contact']>)}>
          <option value="">모든 연락 일정</option>
          <option value="due">연락할 때가 됨</option>
          <option value="week">7일 안</option>
        </select>
        <PersonSelect id="crmOwner" label="담당자" value={owner} onChange={setOwner} people={people} empty="모든 담당자" />
        <select className="ui-select is-auto" aria-label="보관" value={archived ? 'y' : ''} onChange={e => setArchived(!!e.target.value)}>
          <option value="">사용 중</option>
          <option value="y">보관함</option>
        </select>
      </div>
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <SkeletonList rows={5} /> : items.length === 0 ? (
        <EmptyState icon={<Glyph name="users" size={22} />} title={archived ? '보관한 관계가 없어요' : '등록된 관계가 없어요'}>
          {!archived && <button type="button" className="ui-btn is-soft is-sm" style={{ marginTop: 14 }} onClick={() => setCreating(true)}>관계 등록</button>}
        </EmptyState>
      ) : (
        <>
          <p className="adm-queue-count">{items.length}건</p>
          <div className="ui-list">{items.map(r => <Row key={r.id} r={r} people={people} />)}</div>
        </>
      )}
      {creating && (
        <Modal title="관계 등록" onClose={() => setCreating(false)} dismissible={false} modalClass="adm-w-modal">
          <NewRelationship onSaved={d => nav(`/admin/crm/${d.relationship.id}`)} />
        </Modal>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// 상세
// ---------------------------------------------------------------------------

function ContactForm({ relId, init, onSaved }: { relId: string; init: RelContact | null; onSaved: (d: RelationshipDetail) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [v, setV] = useState<ContactInput>({ name: init?.name ?? '', role: init?.role ?? '', email: init?.email ?? '', phone: init?.phone ?? '', is_primary: init?.is_primary ?? false });
  const [busy, setBusy] = useState(false);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!v.name.trim() || busy) return;
    setBusy(true);
    try {
      onSaved(init ? await workApi.editContact(init.id, v) : await workApi.addContact(relId, v));
      toast(init ? '연락처를 고쳤어요.' : '연락처를 추가했어요.', 'success');
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
        <div className="ui-field"><label htmlFor="ctName">이름 <span className="required">*</span></label><input id="ctName" data-autofocus className="ui-input" maxLength={80} value={v.name} onChange={e => setV({ ...v, name: e.target.value })} /></div>
        <div className="ui-field"><label htmlFor="ctRole">역할</label><input id="ctRole" className="ui-input" maxLength={80} value={v.role} onChange={e => setV({ ...v, role: e.target.value })} placeholder="예: A&R 팀장" /></div>
        <div className="ui-field"><label htmlFor="ctEmail">이메일</label><input id="ctEmail" type="email" className="ui-input" maxLength={254} value={v.email ?? ''} onChange={e => setV({ ...v, email: e.target.value })} /></div>
        <div className="ui-field"><label htmlFor="ctPhone">전화</label><input id="ctPhone" type="tel" className="ui-input" maxLength={32} value={v.phone ?? ''} onChange={e => setV({ ...v, phone: e.target.value })} /></div>
      </div>
      <label className="adm-w-check"><input type="checkbox" checked={v.is_primary} onChange={e => setV({ ...v, is_primary: e.target.checked })} />주 연락처</label>
      <div className="adm-form-actions">
        <button type="button" className="ui-btn is-soft" onClick={close}>취소</button>
        <button type="submit" className="ui-btn is-primary" disabled={!v.name.trim() || busy}>{busy ? '저장 중…' : '저장'}</button>
      </div>
    </form>
  );
}

function ActivityForm({ d, onSaved }: { d: RelationshipDetail; onSaved: (d: RelationshipDetail) => void }) {
  const toast = useToast();
  const [kind, setKind] = useState<ActivityKind>('CALL');
  const [summary, setSummary] = useState('');
  const [contact, setContact] = useState('');
  const [next, setNext] = useState('');
  const [busy, setBusy] = useState(false);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!summary.trim() || busy) return;
    setBusy(true);
    try {
      onSaved(await workApi.logActivity(d.relationship.id, {
        kind, summary: summary.trim(), contact_id: contact || null, ...(next ? { next_contact_at: fromLocalInput(next) } : {}),
      }));
      setSummary(''); setNext('');
      toast('연락 기록을 남겼어요.', 'success');
    } catch (err) {
      toast(errorMessage(err, '기록하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <form className="adm-w-box adm-w-activity-form" onSubmit={submit}>
      <div className="ui-chips" role="group" aria-label="연락 방법">
        {ACTIVITY_KINDS.map(k => <button key={k} type="button" className="ui-chip" aria-pressed={kind === k} onClick={() => setKind(k)}>{ACTIVITY_KIND[k].label}</button>)}
      </div>
      <label htmlFor="actSummary" className="sr-only">내용</label>
      <textarea id="actSummary" className="ui-textarea" rows={3} maxLength={2000} value={summary} onChange={e => setSummary(e.target.value)} placeholder="무엇을 이야기했나요? 약속한 일도 같이 적어 두세요." />
      <div className="adm-w-inline">
        {d.contacts.length > 0 && (
          <select className="ui-select is-auto" aria-label="상대" value={contact} onChange={e => setContact(e.target.value)}>
            <option value="">상대 선택 안 함</option>
            {d.contacts.map(c => <option key={c.id} value={c.id}>{c.name}</option>)}
          </select>
        )}
        <label className="adm-w-inline-label" htmlFor="actNext">다음 연락</label>
        <DateTimeField id="actNext" className="ui-input" value={next} onChange={setNext} />
        <button type="submit" className="ui-btn is-primary is-sm" disabled={!summary.trim() || busy}>{busy ? '기록 중…' : '기록'}</button>
      </div>
    </form>
  );
}

export function CrmDetail() {
  const { id = '' } = useParams();
  const nav = useNavigate();
  const toast = useToast();
  const confirm = useConfirm();
  const { me } = useStaff();
  const people = usePeople();
  const { data: loaded, loading, error, reload } = useAsync(() => workApi.relationship(id), [id]);
  const [over, setOver] = useState<RelationshipDetail | null>(null);
  const d = over && over.relationship.id === id ? over : loaded;
  const [contactEdit, setContactEdit] = useState<RelContact | null | 'new'>(null);
  const [linking, setLinking] = useState(false);

  if (error && !d) return <ErrorBox message={error} onRetry={reload} />;
  if (loading && !d) return <SkeletonList rows={6} />;
  if (!d) return null;
  const r = d.relationship;
  const archived = !!r.archived_at;
  const patch = async (p: Omit<Parameters<typeof workApi.updateRelationship>[1], 'row_version'>, msg: string) => {
    try { setOver(await workApi.updateRelationship(r.id, { ...p, row_version: r.row_version })); toast(msg, 'success'); } catch (e) { toast(errorMessage(e, '바꾸지 못했어요.'), 'error'); reload(); }
  };
  const archive = async () => {
    if (!await confirm({ title: '이 관계를 보관할까요?', message: '연락처와 연락 이력은 그대로 남고, 목록에서는 보관함으로 옮겨져요.', confirmLabel: '보관', danger: true })) return;
    try { await workApi.archiveRelationship(r.id); toast('보관했어요.', 'info'); nav('/admin/crm'); } catch (e) { toast(errorMessage(e, '보관하지 못했어요.'), 'error'); }
  };
  const removeContact = async (c: RelContact) => {
    if (!await confirm({ title: `${c.name} 연락처를 지울까요?`, message: '보관되어 연락 이력에는 이름이 남아요.', confirmLabel: '지우기' })) return;
    try { setOver(await workApi.editContact(c.id, { name: c.name, role: c.role, is_primary: false, archived: true })); } catch (e) { toast(errorMessage(e, '지우지 못했어요.'), 'error'); }
  };
  const source = r.entity ? entityLink(r.entity.kind, r.entity.id) : null;

  return (
    <div className="view-enter">
      <Link to="/admin/crm" className="adm-back"><Glyph name="arrow-left" size={14} />관계 관리</Link>
      <PageHeader
        eyebrow={REL_KIND[r.kind].label}
        title={name(r)}
        sub={<span className="adm-codes"><StatusBadge value={REL_STATUS[r.status]} />{contractChip(r)}{r.tags.map(t => <Badge key={t} tone="gray">#{t}</Badge>)}{archived && <Badge tone="gray">보관됨</Badge>}</span>}
        actions={<>
          <Link to={`/admin/tasks?entity_kind=relationship&entity_id=${r.id}`} className="ui-btn is-soft is-sm"><Glyph name="task" size={14} />연결된 업무 {r.open_tasks}</Link>
          {me.role === 'ADMIN' && !archived && <button type="button" className="ui-btn is-soft is-sm" onClick={archive}>보관</button>}
        </>}
      />
      <div className="adm-detail">
        <div className="adm-detail-main">
          <Section title="연락 이력" meta={r.last_contact_at ? `마지막 연락 ${ago(r.last_contact_at)}` : '아직 기록이 없어요'}>
            {!archived && <ActivityForm d={d} onSaved={setOver} />}
            {d.activities.length > 0 && (
              <ol className="adm-w-timeline">
                {d.activities.map(a => (
                  <li key={a.id}>
                    <span className="adm-w-inbox-icon adm-t-blue" aria-hidden="true"><Glyph name={ACTIVITY_KIND[a.kind].icon} size={14} /></span>
                    <span className="adm-min">
                      <b>{ACTIVITY_KIND[a.kind].label}{a.contact ? ` · ${a.contact}` : ''}</b>
                      <p>{a.summary}</p>
                      <small>{when(a.at)} · {people.get(a.by_user)?.name ?? '담당자'}</small>
                    </span>
                  </li>
                ))}
              </ol>
            )}
          </Section>
          <div className="adm-w-block"><NotesPanel kind="relationship" targetId={r.id} /></div>
          {(d.releases.length > 0 || d.agreements.length > 0) && (
            <Section title="작업 공간 기록" meta="원천에서 읽어요">
              {d.agreements.length > 0 && <>
                <h3 className="adm-w-subhead">배급 계약서</h3>
                <div className="ui-list">
                  {d.agreements.map(a => (
                    <div key={a.id} className="ui-row adm-w-policy">
                      <span className="adm-min"><span className="ui-row-title">{a.title}</span><span className="ui-row-meta">{a.release_title && <span>{a.release_title}</span>}<span>{ago(a.updated_at)}</span></span></span>
                      <span className="ui-row-end">{DOC_STATUS[a.status] ? <StatusBadge value={DOC_STATUS[a.status]} /> : <Badge tone="gray">{a.status}</Badge>}</span>
                    </div>
                  ))}
                </div>
              </>}
              {d.releases.length > 0 && <>
                <h3 className="adm-w-subhead">최근 발매</h3>
                <div className="ui-list">
                  {d.releases.map(x => (
                    <Link key={x.id} to={`/admin/reviews/${x.id}`} className="ui-row adm-w-policy">
                      <span className="adm-min"><span className="ui-row-title">{x.title}</span><span className="ui-row-meta"><span>{ago(x.created_at)}</span></span></span>
                      <span className="ui-row-end">{RELEASE_STATUS[x.status] ? <StatusBadge value={RELEASE_STATUS[x.status]} /> : <Badge tone="gray">{x.status}</Badge>}</span>
                    </Link>
                  ))}
                </div>
              </>}
            </Section>
          )}
        </div>
        <aside className="adm-detail-side">
          <section className="ui-card ui-section adm-w-side">
            <dl className="adm-w-fields">
              <div><dt><label htmlFor="rOwner">담당자</label></dt><dd>{archived ? <Person id={r.owner_id} people={people} /> : <PersonSelect id="rOwner" value={r.owner_id} people={people} onChange={v => void patch({ owner_id: v }, '담당자를 바꿨어요.')} />}</dd></div>
              <div><dt><label htmlFor="rStatus">상태</label></dt><dd>{archived ? REL_STATUS[r.status][0] : (
                <select id="rStatus" className="ui-select" value={r.status} onChange={e => void patch({ status: e.target.value as RelStatus }, '상태를 바꿨어요.')}>
                  {REL_STATUSES.map(s => <option key={s} value={s}>{REL_STATUS[s][0]}</option>)}
                </select>)}</dd></div>
              <div><dt><label htmlFor="rNext">다음 연락</label></dt><dd>{archived ? (r.next_contact_at ? when(r.next_contact_at) : '—') : (
                <DateTimeField id="rNext" className="ui-input" value={toLocalInput(r.next_contact_at)}
                  onChange={val => { const v = fromLocalInput(val); if (v !== r.next_contact_at) void patch({ next_contact_at: v }, '다음 연락일을 바꿨어요.'); }} />)}
                {r.contact_due && <small className="is-late-text">연락할 때가 됐어요</small>}</dd></div>
              <div><dt>계약</dt><dd>
                {r.contract.source === 'manual' && !archived ? (
                  <select className="ui-select" aria-label="계약 상태" value={r.contract.status} onChange={e => void patch({ manual_contract_status: e.target.value as ContractStatus }, '계약 상태를 바꿨어요.')}>
                    {CONTRACT_STATUSES.map(c => <option key={c} value={c}>{CONTRACT_STATUS[c][0]}</option>)}
                  </select>
                ) : CONTRACT_STATUS[r.contract.status][0]}
                <small>{r.contract.source === 'agreements' ? `배급 계약서 체결 ${r.contract.signed ?? 0} · 진행 ${r.contract.pending ?? 0}` : r.contract.source === 'dsp_contract' ? `계약 경로 ${r.contract.route ?? '미정'}${r.contract.onboarding_stage ? ` · 연동 ${r.contract.onboarding_stage}` : ''}` : '원천 기록이 없어 직접 관리해요'}</small>
              </dd></div>
              {r.entity && <div><dt>원천 기록</dt><dd>{source ? <Link to={source}>{ENTITY_KIND[r.entity.kind].label} 열기</Link> : ENTITY_KIND[r.entity.kind].label}</dd></div>}
            </dl>
          </section>
          <section className="ui-card ui-section">
            <div className="ui-section-head">
              <h2>연락처<small>{d.contacts.length}명</small></h2>
              {!archived && <button type="button" className="ui-btn is-soft is-sm" onClick={() => setContactEdit('new')}><Glyph name="plus" size={14} />추가</button>}
            </div>
            {d.contacts.length === 0 ? <p className="small muted">등록된 연락처가 없어요.</p> : (
              <ul className="adm-w-contacts">
                {d.contacts.map(c => (
                  <li key={c.id}>
                    <span className="adm-min">
                      <b>{c.name}{c.is_primary && <Badge tone="blue">주 연락처</Badge>}</b>
                      {c.role && <small>{c.role}</small>}
                      {c.email && <a href={`mailto:${c.email}`}>{c.email}</a>}
                      {c.phone && <a href={`tel:${c.phone.replace(/[^0-9+]/g, '')}`}>{c.phone}</a>}
                    </span>
                    {!archived && <span className="adm-w-btns">
                      <button type="button" className="ui-btn is-ghost is-sm" aria-label={`${c.name} 수정`} onClick={() => setContactEdit(c)}><Glyph name="pencil" size={13} /></button>
                      <button type="button" className="ui-btn is-ghost is-sm" aria-label={`${c.name} 지우기`} onClick={() => removeContact(c)}><Glyph name="close" size={13} /></button>
                    </span>}
                  </li>
                ))}
              </ul>
            )}
          </section>
          <section className="ui-card ui-section">
            <div className="ui-section-head">
              <h2>관련 기록<small>{d.links.length}</small></h2>
              {!archived && <button type="button" className="ui-btn is-soft is-sm" onClick={() => setLinking(v => !v)}>{linking ? '닫기' : '연결'}</button>}
            </div>
            {d.links.length > 0 ? <EntityChips items={d.links} onRemove={archived ? undefined : l => void workApi.unlinkRelationship(r.id, l.link_id!).then(setOver).catch(e => toast(errorMessage(e, '해제하지 못했어요.'), 'error'))} />
              : <p className="small muted">관련 발매·계약서·정산 리포트를 연결해 두면 한곳에서 찾을 수 있어요.</p>}
            {linking && <EntityPicker kinds={['release', 'document', 'settlement_report', 'payout', 'inquiry', 'org', 'artist', 'label', 'dsp']} exclude={d.links.map(l => `${l.kind}:${l.id}`)}
              onPick={x => void workApi.linkRelationship(r.id, { entity_kind: x.kind, entity_id: x.id }).then(v => { setOver(v); setLinking(false); }).catch(e => toast(errorMessage(e, '연결하지 못했어요.'), 'error'))} />}
          </section>
        </aside>
      </div>
      {contactEdit && (
        <Modal title={contactEdit === 'new' ? '연락처 추가' : '연락처 수정'} onClose={() => setContactEdit(null)}>
          <ContactForm relId={r.id} init={contactEdit === 'new' ? null : contactEdit} onSaved={setOver} />
        </Modal>
      )}
    </div>
  );
}
