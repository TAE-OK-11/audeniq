// AI 앱의 권한 추가 요청 승인 (/ai/approve/:id) — AI 앱이 request_permission으로 받은 링크.
// 로그인한 본인 요청만 보이고, 승인하면 그 연결에 권한이 더해져 연결을 끊을 때까지 유지된다
// (crates/core/src/mcp/approvals.rs). 60분 안에 답하지 않으면 요청은 사라진다.
// AI 앱 연결 화면과 같은 틀(components/AiFlow): 누가 → 왜 → 무엇을 더 → 지금 권한, 아래 고정 버튼.
import { useEffect, useState } from 'react';
import { Link, useParams } from '../lib/router';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { ApiError, errorMessage } from '../api/errors';
import { aiPermissionRequest, decideAiPermission, SCOPE_HELP, SCOPE_LABEL, type AiPermissionRequest } from '../api/aiConnections';
import { MOCK } from '../lib/mode';
import { AiRequestMissing } from '../components/AiRequestMissing';
import { AiFlowFrame, AiFlowHeader, AiFlowHero, AiFlowLoading, AiFlowPage, AiSetCard, AiSetItem, AiSetRow, workspaceName } from '../components/AiFlow';
import { Tag, type Tone } from '../ui';

const TITLE = '권한 추가 요청';  // 위 막대는 연결 화면과 같은 'AI 앱 연결', 이 이름은 제목 위 작은 글씨로
/** 돈이 움직이거나 내 이름으로 접수되는 권한 — 한 번 더 확인한다 */
const STRONG: Record<string, string> = { 'releases:submit': '내 이름으로 접수', 'payouts:request': '돈이 움직여요' };

const DONE_TITLE: Record<Exclude<AiPermissionRequest['status'], 'PENDING'>, string> = {
  APPROVED: '권한을 더했어요',
  REJECTED: '거절했어요',
  EXPIRED: '요청 시간이 지났어요',
  REVOKED: '이미 끊긴 연결이에요',
};
type Status = AiPermissionRequest['status'];
const DONE_ICON: Record<Status, { kind: 'ok' | 'off' | 'warn'; tone: Tone }> = {
  PENDING: { kind: 'warn', tone: 'amber' }, APPROVED: { kind: 'ok', tone: 'green' }, REJECTED: { kind: 'off', tone: 'gray' },
  EXPIRED: { kind: 'warn', tone: 'gray' }, REVOKED: { kind: 'warn', tone: 'gray' },
};
const STATUS_TEXT: Record<AiPermissionRequest['status'], string> = {
  PENDING: '',
  APPROVED: '이 연결에 권한이 더해졌어요. AI 앱에서 계속 진행해 주세요.',
  REJECTED: '연결의 권한은 그대로예요.',
  EXPIRED: '요청 시간(60분)이 지났어요. 필요하면 AI 앱에서 다시 요청해 주세요.',
  REVOKED: '이 연결은 이미 끊겼어요.',
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
  const [busy, setBusy] = useState<'approve' | 'deny' | null>(null);

  useEffect(() => {
    if (MOCK) { setR(demo(id)); return; }
    aiPermissionRequest(id).then(setR, err => (err instanceof ApiError && err.status === 404 ? setMissing(true) : setError(errorMessage(err, '요청을 불러오지 못했어요. 잠시 후 다시 열어 주세요.'))));
  }, [id]);

  async function decide(approve: boolean) {
    if (!r || busy) return;
    const strong = r.scopes.filter(s => s in STRONG);
    if (approve && strong.length) {
      const ok = await confirm({
        title: '이 권한을 줄까요?',
        message: `${strong.map(s => SCOPE_LABEL[s] ?? s).join('·')} 권한은 내 이름으로 접수하거나 지급을 신청할 수 있어요. 연결을 끊기 전까지 유지돼요.`,
        confirmLabel: '권한 주기',
      });
      if (!ok) return;
    }
    setBusy(approve ? 'approve' : 'deny');
    try {
      const res = MOCK ? { status: approve ? 'APPROVED' : 'REJECTED', connection_scopes: [...r.connection_scopes, ...(approve ? r.scopes : [])] } : await decideAiPermission(r.id, approve);
      setR({ ...r, status: res.status as AiPermissionRequest['status'], connection_scopes: res.connection_scopes, decided_at: new Date().toISOString() });
      toast(approve ? '권한을 더했어요. AI 앱에서 계속 진행해 주세요.' : '요청을 거절했어요.', approve ? 'success' : 'info');
    } catch (err) {
      toast(errorMessage(err, '처리하지 못했어요.'), 'error');
      // 이미 처리됐거나 시간이 지난 요청은 최신 상태로 다시 보여 준다
      aiPermissionRequest(r.id).then(setR, () => {});
    } finally {
      setBusy(null);
    }
  }

  if (missing) return <AiFlowPage><AiRequestMissing title={TITLE} hours="60분" /></AiFlowPage>;
  if (error) {
    return (
      <AiFlowPage>
        <AiFlowFrame>
          <AiFlowHeader kicker={TITLE} title={'요청을\n불러오지 못했어요'} />
          <div className="ui-callout is-danger" role="alert">{error}</div>
          <div className="step-actions">
            <Link id="wizardNext" className="ui-btn is-primary" to="/">스튜디오 홈</Link>
          </div>
        </AiFlowFrame>
      </AiFlowPage>
    );
  }
  if (!r) return <AiFlowPage><AiFlowFrame><AiFlowLoading text="요청을 불러오고 있어요…" /></AiFlowFrame></AiFlowPage>;

  const app = r.connection_label ?? r.client_name;
  const status = r.connection_revoked && r.status === 'PENDING' ? 'REVOKED' : r.status;
  const pending = status === 'PENDING';

  const until = new Date(r.expires_at).toLocaleTimeString('ko-KR', { hour: '2-digit', minute: '2-digit' });

  return (
    <AiFlowPage>
      <AiFlowFrame>
        {pending ? (
          <AiFlowHero
            app={app} kicker={TITLE}
            title={`${app}에서\n권한을 더 달라고 해요`}
            sub="승인하면 연결을 끊을 때까지 유지돼요. 내가 요청한 게 아니라면 거절해 주세요."
          />
        ) : (
          <AiFlowHero icon={DONE_ICON[status]} kicker={TITLE} title={DONE_TITLE[status]} sub={STATUS_TEXT[status]} />
        )}

        <AiSetCard title={pending ? '더 달라는 권한' : status === 'APPROVED' ? '더한 권한' : '요청한 권한'}>
          {r.scopes.map(s => (
            <AiSetItem key={s} title={SCOPE_LABEL[s] ?? s} tag={STRONG[s] && <Tag tone="amber">{STRONG[s]}</Tag>}>
              {SCOPE_HELP[s]}
            </AiSetItem>
          ))}
        </AiSetCard>

        <AiSetCard title="요청 내용">
          {!pending && <AiSetRow label="AI 앱" value={app} />}
          <AiSetRow label="작업 공간" value={workspaceName(r.org_name)} />
          {r.reason && <AiSetRow label="요청 이유" value={r.reason} stack />}
          <AiSetRow label="지금 권한" value={r.connection_scopes.map(s => SCOPE_LABEL[s] ?? s).join('\n')} />
          {pending && <AiSetRow label="답할 시간" value={`${until}까지`} />}
        </AiSetCard>

        {pending ? (
          <>
            <div className="step-actions">
              <button type="button" id="wizardBack" className="ui-btn is-soft" disabled={!!busy} onClick={() => void decide(false)}>거절</button>
              <button type="button" id="wizardNext" className={`ui-btn is-primary${busy === 'approve' ? ' is-busy' : ''}`} disabled={!!busy} onClick={() => void decide(true)}>승인</button>
            </div>
            <p className="aq-connect-deny">
              <button type="button" className="ui-btn is-text" disabled={!!busy} onClick={() => void decide(false)}>거절할래요</button>
            </p>
          </>
        ) : (
          <>
            <div className="step-actions">
              <Link id="wizardBack" className="ui-btn is-soft" to="/profile?open=ai">연결된 AI 앱</Link>
              <Link id="wizardNext" className="ui-btn is-primary" to="/">스튜디오 홈</Link>
            </div>
            <p className="aq-connect-deny">
              <Link className="ui-btn is-text" to="/profile?open=ai">연결된 AI 앱 관리</Link>
            </p>
          </>
        )}
      </AiFlowFrame>
    </AiFlowPage>
  );
}
