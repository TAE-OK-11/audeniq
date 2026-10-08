// 문서 대화 상자 — 공유(권한), 옮기기, 새 문서(템플릿), 반려 사유, 템플릿으로 저장.
// 권한은 서버(work.doc_level)가 판단한다. 여기서는 바꿀 값만 고른다.
import { useEffect, useState } from 'react';
import { Modal } from '../../../components/Modal';
import { useToast } from '../../../components/Toast';
import { errorMessage } from '../../../api/errors';
import { useAsync } from '../../../hooks/useAsync';
import { useStaff } from '../../ui';
import { workApi } from '../api';
import { ROLE_NAME } from '../labels';
import { EntityPicker, PersonSelect, type People } from '../ui';
import type { DocAccess, DocGrant, DocTemplate, EntityRef, StaffRoleName } from '../types';

const LEVELS: [DocGrant['level'], string][] = [['VIEWER', '보기'], ['EDITOR', '편집'], ['MANAGER', '관리(공유·승인)']];
const ROLES = Object.keys(ROLE_NAME) as StaffRoleName[];

export function ShareDialog({ docId, people, onClose, onSaved }: { docId: string; people: People; onClose: () => void; onSaved: () => void }) {
  const toast = useToast();
  const { data, loading, error } = useAsync(() => workApi.docPermissions(docId), [docId]);
  const [access, setAccess] = useState<DocAccess | 'NONE'>('EDITOR');
  const [grants, setGrants] = useState<DocGrant[]>([]);
  const [children, setChildren] = useState(true);
  const [who, setWho] = useState<string | null>(null);
  const [role, setRole] = useState<StaffRoleName | ''>('');
  const [level, setLevel] = useState<DocGrant['level']>('EDITOR');
  const [busy, setBusy] = useState(false);
  useEffect(() => { if (data) { setAccess(data.staff_access ?? 'NONE'); setGrants(data.grants.map(g => ({ principal_kind: g.principal_kind, principal_id: g.principal_id, level: g.level, email: g.email }))); } }, [data]);

  const add = () => {
    const g: DocGrant | null = who ? { principal_kind: 'USER', principal_id: who, level } : role ? { principal_kind: 'ROLE', principal_id: role, level } : null;
    if (!g) return;
    setGrants(gs => [...gs.filter(x => !(x.principal_kind === g.principal_kind && x.principal_id === g.principal_id)), g]);
    setWho(null); setRole('');
  };
  const save = async () => {
    setBusy(true);
    try {
      await workApi.setDocPermissions(docId, {
        staff_access: access === 'NONE' ? null : access,
        grants: grants.map(({ principal_kind, principal_id, level: l }) => ({ principal_kind, principal_id, level: l })),
        apply_to_children: children,
      });
      toast('공유 설정을 바꿨어요.', 'success');
      onSaved(); onClose();
    } catch (e) { toast(errorMessage(e, '공유 설정을 바꾸지 못했어요.'), 'error'); } finally { setBusy(false); }
  };
  const name = (g: DocGrant) => (g.principal_kind === 'ROLE' ? `${ROLE_NAME[g.principal_id as StaffRoleName] ?? g.principal_id} (역할)` : people.get(g.principal_id)?.email ?? g.email ?? '직원');

  return (
    <Modal title="공유" onClose={onClose}>
      {error ? <div className="ui-callout is-danger">{error}</div> : loading || !data ? <div className="ui-skel-rows"><span /></div> : (
        <form className="adm-form" onSubmit={e => { e.preventDefault(); void save(); }}>
          <fieldset className="ui-field">
            <legend>모든 직원</legend>
            {([['EDITOR', '편집할 수 있어요'], ['VIEWER', '볼 수만 있어요'], ['NONE', '제한됨 — 아래 사람·역할과 관리자만']] as const).map(([v, l]) => (
              <label key={v} className="adm-radio"><input type="radio" name="access" checked={access === v} onChange={() => setAccess(v)} />{l}</label>
            ))}
          </fieldset>
          <div className="ui-field">
            <span>사람·역할에게 권한 주기</span>
            <div className="adm-doc-grant-add">
              <PersonSelect id="grant-user" value={who} onChange={v => { setWho(v); if (v) setRole(''); }} people={people} empty="사람 고르기" label="사람" />
              <select className="ui-select is-auto" aria-label="역할" value={role} onChange={e => { setRole(e.target.value as StaffRoleName | ''); if (e.target.value) setWho(null); }}>
                <option value="">또는 역할</option>{ROLES.map(r => <option key={r} value={r}>{ROLE_NAME[r]}</option>)}
              </select>
              <select className="ui-select is-auto" aria-label="권한" value={level} onChange={e => setLevel(e.target.value as DocGrant['level'])}>
                {LEVELS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
              </select>
              <button type="button" className="ui-btn is-soft is-sm" disabled={!who && !role} onClick={add}>추가</button>
            </div>
          </div>
          {grants.length > 0 && (
            <ul className="adm-doc-grants">
              {grants.map(g => (
                <li key={`${g.principal_kind}:${g.principal_id}`}>
                  <span>{name(g)}</span>
                  <select className="ui-select is-auto" aria-label={`${name(g)} 권한`} value={g.level}
                    onChange={e => setGrants(gs => gs.map(x => (x === g ? { ...x, level: e.target.value as DocGrant['level'] } : x)))}>
                    {LEVELS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
                  </select>
                  <button type="button" className="ui-btn is-text is-muted" onClick={() => setGrants(gs => gs.filter(x => x !== g))} aria-label={`${name(g)} 빼기`}>빼기</button>
                </li>
              ))}
            </ul>
          )}
          <label className="adm-check ui-on-soft"><input type="checkbox" checked={children} onChange={e => setChildren(e.target.checked)} />하위 문서에도 같게</label>
          <p className="small muted">만든 사람과 관리자는 항상 볼 수 있어요. 볼 수 없는 직원에게는 문서가 목록·검색·링크 어디에도 나타나지 않아요.</p>
          <div className="adm-form-actions">
            <button type="button" className="ui-btn is-soft" onClick={onClose}>취소</button>
            <button type="submit" className="ui-btn is-primary" disabled={busy}>{busy ? '저장 중…' : '저장'}</button>
          </div>
        </form>
      )}
    </Modal>
  );
}

export function MoveDialog({ docId, onClose, onMove }: { docId: string; onClose: () => void; onMove: (parent: string | null) => void }) {
  return (
    <Modal title="문서 옮기기" onClose={onClose}>
      <p className="small muted">새 상위 문서를 고르세요. 문서는 5단계까지 둘 수 있어요.</p>
      <button type="button" className="ui-btn is-soft" onClick={() => onMove(null)}>맨 위로 옮기기</button>
      <EntityPicker kinds={['doc']} exclude={[`doc:${docId}`]} onPick={(r: EntityRef) => onMove(r.id)} />
    </Modal>
  );
}

export function NewDocDialog({ parentId, onClose, onCreate }: { parentId: string | null; onClose: () => void; onCreate: (template: DocTemplate | null, title: string) => void }) {
  const { data, loading, error } = useAsync(() => workApi.docTemplates(), []);
  const [title, setTitle] = useState('');
  return (
    <Modal title={parentId ? '하위 문서 만들기' : '새 문서'} onClose={onClose}>
      <label className="ui-field"><span>제목</span>
        <input className="ui-input" autoFocus maxLength={200} value={title} onChange={e => setTitle(e.target.value)} placeholder="비워 두면 템플릿 이름" />
      </label>
      <div className="adm-doc-templates" role="list" aria-label="시작할 모양">
        <button type="button" role="listitem" className="adm-doc-template" onClick={() => onCreate(null, title)}>
          <span aria-hidden="true">📄</span><b>빈 문서</b><small>처음부터 쓰기</small>
        </button>
        {error ? <div className="ui-callout is-danger">{error}</div> : loading && !data ? <div className="ui-skel-rows"><span /></div> : (data?.items ?? []).map(t => (
          <button key={t.id} type="button" role="listitem" className="adm-doc-template" onClick={() => onCreate(t, title)}>
            <span aria-hidden="true">{t.icon || '📋'}</span><b>{t.name}</b><small>{t.description}</small>
          </button>
        ))}
      </div>
    </Modal>
  );
}

export function ReasonDialog({ title, label, onClose, onSubmit }: { title: string; label: string; onClose: () => void; onSubmit: (reason: string) => void }) {
  const [v, setV] = useState('');
  return (
    <Modal title={title} onClose={onClose}>
      <form className="adm-form" onSubmit={e => { e.preventDefault(); if (v.trim()) onSubmit(v.trim()); }}>
        <label className="ui-field"><span>{label}</span><textarea className="ui-textarea" autoFocus rows={3} maxLength={1000} value={v} onChange={e => setV(e.target.value)} /></label>
        <div className="adm-form-actions">
          <button type="button" className="ui-btn is-soft" onClick={onClose}>취소</button>
          <button type="submit" className="ui-btn is-primary" disabled={!v.trim()}>보내기</button>
        </div>
      </form>
    </Modal>
  );
}

export function TemplateDialog({ docId, defaultName, onClose }: { docId: string; defaultName: string; onClose: () => void }) {
  const toast = useToast();
  const { me } = useStaff();
  const [name, setName] = useState(defaultName);
  const [description, setDescription] = useState('');
  const [busy, setBusy] = useState(false);
  if (me.role !== 'ADMIN') return null;
  return (
    <Modal title="템플릿으로 저장" onClose={onClose}>
      <form className="adm-form" onSubmit={async e => {
        e.preventDefault();
        setBusy(true);
        try { await workApi.saveDocTemplate({ from_doc: docId, name: name.trim(), description: description.trim() }); toast('템플릿을 만들었어요.', 'success'); onClose(); }
        catch (err) { toast(errorMessage(err, '템플릿을 만들지 못했어요.'), 'error'); } finally { setBusy(false); }
      }}>
        <label className="ui-field"><span>이름</span><input className="ui-input" autoFocus maxLength={100} value={name} onChange={e => setName(e.target.value)} /></label>
        <label className="ui-field"><span>설명</span><input className="ui-input" maxLength={300} value={description} onChange={e => setDescription(e.target.value)} /></label>
        <p className="small muted">파일·이미지가 들어간 문서는 템플릿이 될 수 없어요.</p>
        <div className="adm-form-actions">
          <button type="button" className="ui-btn is-soft" onClick={onClose}>취소</button>
          <button type="submit" className="ui-btn is-primary" disabled={busy || !name.trim()}>저장</button>
        </div>
      </form>
    </Modal>
  );
}
