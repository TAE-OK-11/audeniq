// 권리자에게 보낸 서명 요청 — 상태 확인, 새 링크 만들기, 요청 취소, 서명된 문서 열기
import { useEffect, useState } from 'react';
import { SIGNING_STATUS_LABEL, signingApi, signingUrl, useMockSigning, type IssuedLink, type SigningRequest } from '../api/signing';
import { RIGHTS_DOCUMENTS, type SigningChannel } from '../lib/rightsDocument';
import { MOCK } from '../lib/mode';
import { errorMessage } from '../api/errors';
import { useToast } from './Toast';
import { useConfirm } from './Confirm';
import { Modal } from './Modal';
import { Segmented } from './Segmented';
import { currentPath, useNavigate } from '../lib/router';
import { refreshDocs } from '../store/portalSync';

const TONE: Record<string, string> = { SIGNED: 'is-ok', DECLINED: 'is-bad', CANCELLED: 'is-off', EXPIRED: 'is-off', PENDING: '', VERIFIED: '' };

function day(iso: string | null): string {
  if (!iso) return '';
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? '' : d.toLocaleString('ko-KR', { month: 'long', day: 'numeric', hour: 'numeric', minute: '2-digit' });
}

export function SigningRequests({ releases, tick, onOpenDoc }: {
  releases: { id: string; title: string }[];
  tick: number;
  onOpenDoc: (id: string) => void;
}) {
  const toast = useToast();
  const confirm = useConfirm();
  const nav = useNavigate();
  const mockRows = useMockSigning();
  const [items, setItems] = useState<SigningRequest[]>([]);
  const [relink, setRelink] = useState<SigningRequest | null>(null);
  const [channel, setChannel] = useState<SigningChannel>('LINK');
  const [issued, setIssued] = useState<IssuedLink | null>(null);
  const [busy, setBusy] = useState(false);

  const load = async () => {
    try {
      const list = await signingApi.list();
      setItems(list);
      // 그사이 서명된 요청이 있으면 서류 목록도 새로 받는다
      if (!MOCK && list.some(r => r.status === 'SIGNED')) void refreshDocs();
    } catch { /* 목록은 다음에 다시 */ }
  };
  useEffect(() => { void load(); }, [tick, MOCK ? mockRows : null]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!items.length) return null;
  const releaseTitle = (id: string) => releases.find(r => r.id === id)?.title ?? '';

  const cancel = async (r: SigningRequest) => {
    if (!(await confirm({ title: '서명 요청을 취소할까요?', message: `${r.signer_name}님께 보낸 링크로는 더 이상 서명할 수 없어요.`, confirmLabel: '요청 취소', danger: true }))) return;
    try { await signingApi.cancel(r.id); toast('서명 요청을 취소했어요.'); void load(); }
    catch (e) { toast(errorMessage(e)); }
  };
  const reissue = async () => {
    if (!relink || busy) return;
    setBusy(true);
    try {
      const link = await signingApi.reissue(relink.id, channel);
      if (channel === 'IN_PERSON') { nav(`/sign/${link.token}?from=${encodeURIComponent(currentPath())}`); return; }
      setIssued(link);
      void load();
    } catch (e) { toast(errorMessage(e)); }
    finally { setBusy(false); }
  };

  return (
    <section className="aq-signreq" aria-labelledby="aqSignReqHead">
      <h2 id="aqSignReqHead" className="subhead">권리자 서명 요청</h2>
      <ul className="aq-signreq-list">
        {items.map(r => (
          <li key={r.id}>
            <div className="min-0">
              <strong>{RIGHTS_DOCUMENTS[r.document_kind]?.title ?? r.title}</strong>
              <span>{[releaseTitle(r.release_id), `${r.signer_name} · ${r.signer_role}`].filter(Boolean).join(' · ')}</span>
              <span className="aq-signreq-when">
                {r.status === 'SIGNED' ? `본인확인 후 서명 · ${day(r.signed_at)}`
                  : r.status === 'DECLINED' ? `거절${r.decline_reason ? ` · ${r.decline_reason}` : ''}`
                    : r.status === 'PENDING' || r.status === 'VERIFIED' ? `${day(r.expires_at)}까지 · ${r.channel === 'LINK' ? '링크' : '이 기기'}` : ''}
              </span>
            </div>
            <em className={TONE[r.status]}>{SIGNING_STATUS_LABEL[r.status]}</em>
            <div className="aq-signreq-actions">
              {r.status === 'SIGNED' && r.document_id && <button type="button" className="link-btn" onClick={() => onOpenDoc(r.document_id!)}>문서 보기</button>}
              {(r.status === 'PENDING' || r.status === 'EXPIRED') && <button type="button" className="link-btn" onClick={() => { setRelink(r); setIssued(null); setChannel('LINK'); }}>새 링크</button>}
              {(r.status === 'PENDING' || r.status === 'VERIFIED') && <button type="button" className="link-btn" onClick={() => void cancel(r)}>취소</button>}
            </div>
          </li>
        ))}
      </ul>

      {relink && (
        <Modal title="새 서명 링크" onClose={() => setRelink(null)}>
          {issued ? <>
            <p>{relink.signer_name}님께 새 링크를 보내 주세요. 이전 링크는 더 이상 쓸 수 없어요.</p>
            <div className="aq-sign-link"><code>{signingUrl(issued.token)}</code></div>
            <div className="doc-actions">
              <button type="button" className="button secondary" onClick={() => { void navigator.clipboard?.writeText(signingUrl(issued.token)).then(() => toast('링크를 복사했어요.', 'success')); }}>링크 복사</button>
              <button type="button" className="button" onClick={() => setRelink(null)}>완료</button>
            </div>
          </> : <>
            <p className="small muted">새 링크를 만들면 이전 링크는 바로 막혀요.</p>
            <Segmented className="aq-method" label="서명 받는 방법" value={channel} onChange={setChannel}
              options={[{ value: 'LINK', label: '서명 링크 보내기' }, { value: 'IN_PERSON', label: '이 기기에서 바로' }] as const} />
            <button type="button" className="button studio-submit-wide" disabled={busy} onClick={() => void reissue()}>{busy ? '만드는 중…' : '새 링크 만들기'}</button>
          </>}
        </Modal>
      )}
    </section>
  );
}
