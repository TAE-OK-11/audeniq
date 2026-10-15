// 팀 · 작업 공간 — 지금 작업 공간과 역할, 멤버(소유자는 이메일로 초대·역할 변경·해제), 내가 받은 초대.
// 초대는 AUDENIQ에 가입한 계정의 이메일로 보내고, 받은 사람이 자기 계정에서 수락해야 멤버가 된다.
import { useCallback, useEffect, useState } from 'react';
import { api } from '../api/client';
import { useAuth } from '../api/auth';
import { errorMessage } from '../api/errors';
import type { Invitation, OrgMember } from '../api/types';
import { Glyph } from './Glyph';
import { Modal, useModalClose } from './Modal';
import { useConfirm } from './Confirm';
import { useToast } from './Toast';

const ROLE: Record<string, string> = { OWNER: '소유자', EDITOR: '편집', VIEWER: '보기' };
const ROLE_HELP = '편집: 발매를 만들고 고칠 수 있어요 · 보기: 내용만 볼 수 있어요';

function Row({ label, value, onClick }: { label: string; value?: string; onClick?: () => void }) {
  const body = (
    <>
      <span className="aq-set-label">{label}</span>
      {value && <span className="aq-set-value">{value}</span>}
      {onClick && <Glyph name="chevron-right" size={16} className="aq-set-chev" />}
    </>
  );
  // 누를 일이 없는 줄(작업 공간이 하나뿐일 때)은 비활성 버튼이 아니라 보통 정보 줄로 — 회색으로 죽어 보이지 않게
  if (!onClick) return <div className="aq-set-row">{body}</div>;
  return <button type="button" className="aq-set-row" onClick={onClick}>{body}</button>;
}

function SwitchOrg() {
  const { org, orgs, selectOrg } = useAuth();
  return (
    <ul className="aq-team-list">
      {orgs.map(o => (
        <li key={o.id}>
          <button type="button" className={`aq-team-pick${o.id === org?.id ? ' is-on' : ''}`} onClick={() => o.id !== org?.id && selectOrg(o)}>
            <span>{o.name}</span>
            <small>{ROLE[o.role ?? ''] ?? ''}{o.id === org?.id ? ' · 지금 작업 공간' : ''}</small>
          </button>
        </li>
      ))}
    </ul>
  );
}

function Members({ onChanged }: { onChanged: () => void }) {
  const toast = useToast();
  const confirm = useConfirm();
  const [data, setData] = useState<{ role: string; items: OrgMember[] } | null>(null);
  const [email, setEmail] = useState('');
  const [role, setRole] = useState<'EDITOR' | 'VIEWER'>('EDITOR');
  const [busy, setBusy] = useState(false);
  const load = useCallback(() => { api.listMembers().then(setData).catch(() => setData(null)); }, []);
  useEffect(load, [load]);
  const owner = data?.role === 'OWNER';

  const invite = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!email.trim() || busy) return;
    setBusy(true);
    try {
      await api.inviteMember(email.trim(), role);
      toast('초대를 보냈어요. 받은 사람이 자기 계정에서 수락하면 멤버가 돼요.', 'success');
      setEmail('');
      load();
      onChanged();
    } catch (err) {
      const code = (err as { code?: string }).code;
      toast(code === 'MEMBER_ACCOUNT_NOT_FOUND'
        ? '이 이메일로 가입한 AUDENIQ 계정이 없어요. 먼저 가입해 달라고 알려 주세요.'
        : errorMessage(err, '초대하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  const change = async (m: OrgMember, next: 'EDITOR' | 'VIEWER' | 'REVOKED') => {
    if (next === 'REVOKED') {
      const ok = await confirm({ title: m.status === 'INVITED' ? '초대를 취소할까요?' : '멤버에서 내보낼까요?', message: `${m.email ?? ''} 계정은 이 작업 공간의 발매와 정산을 더 볼 수 없어요.`, confirmLabel: m.status === 'INVITED' ? '초대 취소' : '내보내기', danger: true });
      if (!ok) return;
    }
    try {
      await api.setMember(m.user_id, next === 'REVOKED' ? (m.role === 'VIEWER' ? 'VIEWER' : 'EDITOR') : next, next === 'REVOKED' ? 'REVOKED' : 'ACTIVE');
      load();
      onChanged();
    } catch (err) {
      toast(errorMessage(err, '바꾸지 못했어요.'), 'error');
    }
  };

  if (!data) return <p className="muted small">멤버를 불러오는 중이에요…</p>;
  const shown = data.items.filter(m => m.status !== 'REVOKED');
  return (
    <>
      <ul className="aq-team-list">
        {shown.map(m => (
          <li key={m.user_id} className="aq-team-member">
            <span className="min-0">
              <strong>{m.email ?? '멤버'}{m.me ? ' (나)' : ''}</strong>
              <small>{ROLE[m.role]}{m.status === 'INVITED' ? ' · 수락 대기' : ''}</small>
            </span>
            {owner && !m.me && m.role !== 'OWNER' && (
              <span className="aq-team-actions">
                {m.status === 'ACTIVE' && (
                  <select aria-label="역할" value={m.role} onChange={e => void change(m, e.target.value as 'EDITOR' | 'VIEWER')}>
                    <option value="EDITOR">편집</option>
                    <option value="VIEWER">보기</option>
                  </select>
                )}
                <button type="button" className="ui-btn is-text aq-danger-text is-sm" onClick={() => void change(m, 'REVOKED')}>
                  {m.status === 'INVITED' ? '초대 취소' : '내보내기'}
                </button>
              </span>
            )}
          </li>
        ))}
      </ul>
      {owner ? (
        <form className="aq-team-invite" onSubmit={invite}>
          <div className="ui-field">
            <label htmlFor="teamInviteEmail">초대할 계정 이메일</label>
            <input id="teamInviteEmail" type="email" autoComplete="off" maxLength={320} value={email} onChange={e => setEmail(e.target.value)} placeholder="AUDENIQ에 가입한 이메일" />
          </div>
          <div className="ui-field">
            <label htmlFor="teamInviteRole">역할</label>
            <select id="teamInviteRole" value={role} onChange={e => setRole(e.target.value as 'EDITOR' | 'VIEWER')}>
              <option value="EDITOR">편집</option>
              <option value="VIEWER">보기</option>
            </select>
            <p className="ui-field-help">{ROLE_HELP}</p>
          </div>
          <button type="submit" className="ui-btn is-primary" disabled={busy || !email.trim()}>{busy ? '보내는 중' : '초대 보내기'}</button>
        </form>
      ) : (
        <p className="ui-field-help">멤버 초대와 역할 변경은 소유자만 할 수 있어요.</p>
      )}
    </>
  );
}

function Invitations({ items, onDone }: { items: Invitation[]; onDone: () => void }) {
  const toast = useToast();
  const close = useModalClose();
  const [busy, setBusy] = useState('');
  const act = async (i: Invitation, accept: boolean) => {
    setBusy(i.org_id);
    try {
      if (accept) {
        await api.acceptInvitation(i.org_id);
        toast(`‘${i.org_name}’ 작업 공간에 들어갔어요. 계정 정보에서 작업 공간을 바꿀 수 있어요.`, 'success');
        // 작업 공간 목록을 새로 받는다
        window.setTimeout(() => window.location.reload(), 900);
      } else {
        await api.declineInvitation(i.org_id);
        toast('초대를 거절했어요.');
      }
      onDone();
      close();
    } catch (err) {
      toast(errorMessage(err, '처리하지 못했어요.'), 'error');
    } finally {
      setBusy('');
    }
  };
  return (
    <ul className="aq-team-list">
      {items.map(i => (
        <li key={i.org_id} className="aq-team-member">
          <span className="min-0"><strong>{i.org_name}</strong><small>{ROLE[i.role]} 권한으로 초대</small></span>
          <span className="aq-team-actions">
            <button type="button" className="ui-btn is-soft is-sm" disabled={busy === i.org_id} onClick={() => void act(i, false)}>거절</button>
            <button type="button" className="ui-btn is-primary is-sm" disabled={busy === i.org_id} onClick={() => void act(i, true)}>수락</button>
          </span>
        </li>
      ))}
    </ul>
  );
}

export function TeamSettings() {
  const { org, orgs } = useAuth();
  const [open, setOpen] = useState<'switch' | 'members' | 'invites' | null>(null);
  const [invites, setInvites] = useState<Invitation[]>([]);
  const [count, setCount] = useState<number | null>(null);
  const refresh = useCallback(() => {
    api.listInvitations().then(setInvites).catch(() => setInvites([]));
    api.listMembers().then(m => setCount(m.items.filter(x => x.status === 'ACTIVE').length)).catch(() => setCount(null));
  }, []);
  useEffect(refresh, [refresh]);

  return (
    <section className="aq-set-card" aria-labelledby="setTeam">
      <h2 id="setTeam">팀 · 작업 공간</h2>
      <Row
        label="작업 공간"
        value={org ? `${org.name}${org.role ? ` · ${ROLE[org.role]}` : ''}` : undefined}
        onClick={orgs.length > 1 ? () => setOpen('switch') : undefined}
      />
      <Row label="멤버" value={count != null ? `${count}명` : undefined} onClick={() => setOpen('members')} />
      {!!invites.length && <Row label="받은 초대" value={`${invites.length}건`} onClick={() => setOpen('invites')} />}
      {open === 'switch' && <Modal title="작업 공간 바꾸기" modalClass="aq-set-modal" onClose={() => setOpen(null)}><SwitchOrg /></Modal>}
      {open === 'members' && <Modal title="멤버" modalClass="aq-set-modal" onClose={() => setOpen(null)}><Members onChanged={refresh} /></Modal>}
      {open === 'invites' && <Modal title="받은 초대" modalClass="aq-set-modal" onClose={() => setOpen(null)}><Invitations items={invites} onDone={refresh} /></Modal>}
    </section>
  );
}
