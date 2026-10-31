// 공동 작가에게 지분 확인 링크 보내기 — 링크를 복사해 카카오톡·문자로 보내거나, 메일 주소를 적어 메일로 보낸다.
// 받는 사람은 AUDENIQ 계정 없이 링크(/split-confirm/…)에서 '맞아요' 또는 '달라요'(의견)로 한 번 답한다.
// 여럿이면 한 창에서 모두 — 링크를 한꺼번에 만들어 이름과 함께 복사한다
import { useState } from 'react';
import { pubApi, type Work, type WorkContributor } from '../../api/publishing';
import { errorMessage } from '../../api/errors';
import { pct } from '../../lib/publishing';
import { Modal } from '../Modal';
import { Glyph } from '../Glyph';
import { useToast } from '../Toast';
import { Badge } from '../../ui';
import '../../styles/split/ReleaseDetail.css';
import '../../styles/split/Publishing.css';

const linkOf = (token: string) => `${window.location.origin}${import.meta.env.BASE_URL.replace(/\/$/, '')}/split-confirm/${token}`;

async function copy(text: string) {
  try { await navigator.clipboard.writeText(text); return true; } catch { return false; }
}

export function InviteModal({ work, people, onClose, onDone }: { work: Work; people: WorkContributor[]; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const [links, setLinks] = useState<Record<string, string>>({});
  const [emails, setEmails] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<string | null>(null);

  const make = async (c: WorkContributor, email?: string) => {
    setBusy(c.id);
    try {
      const r = await pubApi.inviteContributor(work.id, c.id, email);
      setLinks(l => ({ ...l, [c.id]: linkOf(r.token) }));
      if (r.emailed) toast(`${c.name} 님에게 메일로 보냈어요.`, 'success');
      onDone();
      return linkOf(r.token);
    } catch (err) { toast(errorMessage(err, '링크를 만들지 못했어요.'), 'error'); return null; }
    finally { setBusy(null); }
  };
  const makeAll = async () => {
    const lines: string[] = [];
    for (const c of people) {
      const url = links[c.id] ?? await make(c);
      if (url) lines.push(`${c.name} (${pct(c.share)}): ${url}`);
    }
    if (lines.length && await copy(`‘${work.title}’ 지분 확인 부탁드려요.\n${lines.join('\n')}`)) toast('이름과 링크를 모두 복사했어요.', 'success');
  };

  return (
    <Modal title="지분 확인 링크 보내기" onClose={onClose}>
      <div className="aq-track-sheet aq-pbl-invite">
        <p className="ui-field-help">링크를 받은 사람은 계정 없이 ‘{work.title}’의 지분을 보고 맞는지 답해요. 다시 만들면 전에 보낸 링크는 쓸 수 없어요.</p>
        {people.length > 1 && (
          <button type="button" className="ui-btn is-primary is-block" disabled={!!busy} onClick={() => void makeAll()}>
            <Glyph name="link" size={15} className="aq-glyph-lead" />{people.length}명 링크 모두 만들고 복사
          </button>
        )}
        <ul className="aq-pbl-invite-list">
          {people.map(c => {
            const url = links[c.id];
            const email = emails[c.id] ?? '';
            return (
              <li key={c.id}>
                <div className="aq-pbl-invite-head">
                  <strong>{c.name}</strong><span>{pct(c.share)}</span>
                  {c.inviteMail === 'SENT' ? <Badge tone="blue">메일 보냄</Badge> : c.invited && <Badge tone="gray">링크 보냄</Badge>}
                </div>
                {url ? (
                  <div className="aq-pbl-invite-link">
                    <code>{url}</code>
                    <button type="button" className="ui-btn is-soft is-sm" onClick={() => void copy(url).then(ok => toast(ok ? '링크를 복사했어요.' : '복사하지 못했어요. 링크를 길게 눌러 복사해 주세요.', ok ? 'success' : 'error'))}>복사</button>
                  </div>
                ) : (
                  <button type="button" className={`ui-btn is-soft is-sm${busy === c.id ? ' is-busy' : ''}`} disabled={!!busy} onClick={() => void make(c)}>링크 만들기</button>
                )}
                <div className="aq-pbl-invite-mail">
                  <input className="ui-input" type="email" inputMode="email" placeholder="메일로도 보내기 (선택)" aria-label={`${c.name} 메일 주소`} value={email}
                    onChange={e => setEmails(m => ({ ...m, [c.id]: e.target.value }))} />
                  <button type="button" className="ui-btn is-soft is-sm" disabled={!!busy || !/\S+@\S+\.\S+/.test(email)} onClick={() => void make(c, email.trim())}>보내기</button>
                </div>
              </li>
            );
          })}
        </ul>
      </div>
    </Modal>
  );
}
