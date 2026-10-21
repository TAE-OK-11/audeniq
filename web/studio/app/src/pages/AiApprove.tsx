// AI 앱의 권한 추가 요청 승인 (/ai/approve/:id) — AI 앱이 request_permission으로 받은 링크.
// 로그인한 본인 요청만 보이고, 승인하면 그 연결에 권한이 더해져 연결을 끊을 때까지 유지된다
// (crates/core/src/mcp/approvals.rs). 60분 안에 답하지 않으면 요청은 사라진다.
import { useEffect, useState } from 'react';
import { Link, useParams } from '../lib/router';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { ApiError, errorMessage } from '../api/errors';
import { aiPermissionRequest, decideAiPermission, SCOPE_HELP, SCOPE_LABEL, type AiPermissionRequest } from '../api/aiConnections';
import { MOCK } from '../lib/mode';
import { PageHeader } from '../ui';
import { AiRequestMissing } from '../components/AiRequestMissing';

const workspaceName = (name: string) => (name === 'Personal workspace' ? '내 작업 공간' : name);
/** 돈이 움직이거나 내 이름으로 접수되는 권한 — 한 번 더 확인한다 */
const STRONG = new Set(['releases:submit', 'payouts:request']);

const STATUS_TEXT: Record<AiPermissionRequest['status'], string> = {
  PENDING: '',
  APPROVED: '승인했어요. 이 연결에 권한이 더해졌어요.',
  REJECTED: '거절했어요. 연결의 권한은 그대로예요.',
  EXPIRED: '요청 시간(60분)이 지났어요. 필요하면 AI 앱에서 다시 요청해 주세요.',
  REVOKED: '이 연결은 이미 끊겼어요.',
  WITHDRAWN: 'AI 앱에서 요청을 거둬들였어요. 연결의 권한은 그대로예요.',
};

const demo = (id: string): AiPermissionRequest => ({
  id, status: 'PENDING', scopes: ['releases:submit'], reason: '발매 신청서를 검토한 대로 접수하려고 해요.',
  client_name: 'Claude', org_id: 'o1', org_name: 'Personal workspace',
  created_at: new Date().toISOString(), expires_at: new Date(Date.now() + 3_600_000).toISOString(), decided_at: null,
  connection_scopes: ['releases:read', 'releases:write'], connection_label: null, connection_revoked: false,
});

export function AiApprove() {
  const { id = '' } = useParams<{ id: string }>();
  const toast = useToast();
  const confirm = useConfirm();
  const [r, setR] = useState<AiPermissionRequest | null>(null);
  const [error, setError] = useState('');
  const [missing, setMissing] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (MOCK) { setR(demo(id)); return; }
    aiPermissionRequest(id).then(setR, err => (err instanceof ApiError && err.status === 404 ? setMissing(true) : setError(errorMessage(err, '요청을 불러오지 못했어요. 잠시 후 다시 열어 주세요.'))));
  }, [id]);

  async function decide(approve: boolean) {
    if (!r || busy) return;
    const strong = r.scopes.filter(s => STRONG.has(s));
    if (approve && strong.length) {
      const ok = await confirm({
        title: '이 권한을 줄까요?',
        message: `${strong.map(s => SCOPE_LABEL[s] ?? s).join('·')} 권한은 내 이름으로 접수하거나 지급을 신청할 수 있어요. 연결을 끊기 전까지 유지돼요.`,
        confirmLabel: '권한 주기',
      });
      if (!ok) return;
    }
    setBusy(true);
    try {
      const res = MOCK ? { status: approve ? 'APPROVED' : 'REJECTED', connection_scopes: [...r.connection_scopes, ...(approve ? r.scopes : [])] } : await decideAiPermission(r.id, approve);
      setR({ ...r, status: res.status as AiPermissionRequest['status'], connection_scopes: res.connection_scopes, decided_at: new Date().toISOString() });
      toast(approve ? '권한을 더했어요. AI 앱에서 계속 진행해 주세요.' : '요청을 거절했어요.', approve ? 'success' : 'info');
    } catch (err) {
      toast(errorMessage(err, '처리하지 못했어요.'), 'error');
      // 이미 처리됐거나 시간이 지난 요청은 최신 상태로 다시 보여 준다
      aiPermissionRequest(r.id).then(setR, () => {});
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="view-enter aq-ai-approve">
      <PageHeader eyebrow="AI 앱 연결" title="권한 추가 요청" sub="AI 앱이 이 연결에 권한을 더 달라고 요청했어요. 내가 요청한 게 아니라면 거절하고 연결을 끊어 주세요." />
      {error && <div className="ui-callout is-danger" role="alert">{error}</div>}
      {missing && <AiRequestMissing hours="60분" />}
      {!r && !error && !missing && <p role="status">불러오는 중이에요.</p>}
      {r && (
        <section className="ui-card aq-ai-approve-card">
          <dl className="aq-ai-approve-meta">
            <dt>AI 앱</dt><dd><b>{r.connection_label ?? r.client_name}</b></dd>
            <dt>작업 공간</dt><dd>{workspaceName(r.org_name)}</dd>
            <dt>지금 권한</dt><dd>{r.connection_scopes.map(s => SCOPE_LABEL[s] ?? s).join(', ')}</dd>
            {r.reason && <><dt>요청 이유</dt><dd className="aq-ai-approve-reason">{r.reason}</dd></>}
          </dl>
          <h2 className="ui-field-label">더 달라는 권한</h2>
          <ul className="aq-ai-approve-scopes">
            {r.scopes.map(s => (
              <li key={s} className={STRONG.has(s) ? 'is-strong' : undefined}>
                <strong>{SCOPE_LABEL[s] ?? s}</strong>
                <span>{SCOPE_HELP[s]}</span>
              </li>
            ))}
          </ul>
          {r.status === 'PENDING' && !r.connection_revoked ? (
            <>
              <p className="ui-field-help">승인하면 연결을 끊을 때까지 유지돼요. {new Date(r.expires_at).toLocaleTimeString('ko-KR', { hour: '2-digit', minute: '2-digit' })}까지 답해 주세요.</p>
              <div className="aq-ai-approve-actions">
                <button type="button" className="ui-btn is-soft" disabled={busy} onClick={() => void decide(false)}>거절</button>
                <button type="button" className={`ui-btn is-primary${busy ? ' is-busy' : ''}`} disabled={busy} onClick={() => void decide(true)}>승인</button>
              </div>
            </>
          ) : (
            <div className={`ui-callout${r.status === 'APPROVED' ? ' is-success' : ''}`} role="status">
              {r.connection_revoked && r.status === 'PENDING' ? STATUS_TEXT.REVOKED : STATUS_TEXT[r.status]}
            </div>
          )}
          <p className="ui-field-help"><Link to="/profile?open=ai">연결된 AI 앱 관리</Link></p>
        </section>
      )}
    </div>
  );
}
