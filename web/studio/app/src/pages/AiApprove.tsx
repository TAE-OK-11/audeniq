// AI 앱의 권한 추가 요청 승인 (/ai/approve/:id) — AI 앱이 request_permission으로 받은 링크.
// 로그인한 본인 요청만 보이고, 승인하면 그 연결에 권한이 더해져 연결을 끊을 때까지 유지된다
// (crates/core/src/mcp/approvals.rs). 60분 안에 답하지 않으면 요청은 사라진다.
// AI 앱 연결 화면과 같은 틀(components/AiFlow): 누가 → 왜 → 무엇을 더 → 지금 권한, 아래 고정 버튼.
import { useCallback, useEffect, useState } from 'react';
import { Link, useParams } from '../lib/router';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { ApiError, errorMessage } from '../api/errors';
import { appBrandForName, aiPermissionRequest, decideAiPermission, SCOPE_HELP, SCOPE_LABEL, type AiPermissionRequest } from '../api/aiConnections';
import { MOCK } from '../lib/mode';
import { particle } from '../lib/josa';
import { AiRequestMissing } from '../components/AiRequestMissing';
import { AiFlowFrame, AiFlowHeader, AiFlowHero, AiFlowLoading, AiFlowPage, AiSetCard, AiSetItem, AiSetRow, AiNextRequest, timeLeft, useFreshRequest, useNow, workspaceName } from '../components/AiFlow';
import { Tag, type Tone } from '../ui';

const TITLE = '권한 추가 요청';  // 위 막대는 연결 화면과 같은 'AI 앱 연결', 이 이름은 제목 위 작은 글씨로
/** 돈이 움직이거나 내 이름으로 접수되는 권한 — 한 번 더 확인한다 */
const STRONG: Record<string, string> = { 'releases:submit': '내 이름으로 접수', 'payouts:request': '돈이 움직여요' };
/** 확인 창에서 그 권한으로 앱이 할 수 있게 되는 일 */
const STRONG_DOES: Record<string, string> = {
  'releases:submit': '내 이름으로 발매 신청을 접수할 수 있어요.',
  'payouts:request': '등록한 계좌로 지급을 신청할 수 있어요.',
};

const DONE_TITLE: Record<Exclude<AiPermissionRequest['status'], 'PENDING'>, string> = {
  APPROVED: '권한을 줬어요',
  REJECTED: '거절했어요',
  EXPIRED: '답할 수 있는 시간이 지났어요',
  REVOKED: '이미 끊긴 연결이에요',
  WITHDRAWN: 'AI 앱에서 거둬들인 요청이에요',
};
type Status = AiPermissionRequest['status'];
const DONE_ICON: Record<Status, { kind: 'ok' | 'off' | 'warn'; tone: Tone }> = {
  PENDING: { kind: 'warn', tone: 'amber' }, APPROVED: { kind: 'ok', tone: 'green' }, REJECTED: { kind: 'off', tone: 'gray' },
  EXPIRED: { kind: 'warn', tone: 'gray' }, REVOKED: { kind: 'warn', tone: 'gray' },
  WITHDRAWN: { kind: 'off', tone: 'gray' },
};
const STATUS_TEXT: Record<AiPermissionRequest['status'], string> = {
  PENDING: '',
  APPROVED: 'AI 앱 대화로 돌아가 이어서 진행해 주세요. 바로 이어지지 않으면 "권한 줬어"라고 말해 주세요.',
  REJECTED: '이 연결의 권한은 그대로예요. AI 앱은 지금 있는 권한으로만 도와요.',
  EXPIRED: '요청은 60분 동안만 답할 수 있어요. 필요하면 AI 앱에 다시 요청해 달라고 말해 주세요.',
  REVOKED: '끊긴 연결의 요청은 처리할 수 없어요.',
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
  const [busy, setBusy] = useState<'approve' | 'deny' | null>(null);

  const load = useCallback(() => {
    if (MOCK) { setR(prev => prev ?? demo(id)); return; }
    setError('');
    aiPermissionRequest(id).then(setR, err => (err instanceof ApiError && err.status === 404 ? setMissing(true) : setError(errorMessage(err, '요청을 불러오지 못했어요. 잠시 후 다시 해 주세요.'))));
  }, [id]);
  useEffect(load, [load]);
  const now = useNow();
  // 시간이 지나면 버튼을 누르기 전에 바로 '시간이 지났어요'로
  const status: Status | null = !r ? null
    : r.connection_revoked && r.status === 'PENDING' ? 'REVOKED'
    : r.status === 'PENDING' && new Date(r.expires_at).getTime() <= now ? 'EXPIRED'
    : r.status;
  useFreshRequest(load, { active: status === 'PENDING', poll: false });

  async function decide(approve: boolean) {
    if (!r || busy) return;
    const strong = r.scopes.filter(s => s in STRONG);
    const app = r.connection_label ?? r.client_name;
    if (approve && strong.length) {
      const ok = await confirm({
        title: `${strong.map(s => SCOPE_LABEL[s] ?? s).join('·')} 권한을 줄까요?`,
        message: `${app}${particle(app, '이/가')} ${strong.map(s => STRONG_DOES[s]).join(' ')} 연결을 끊기 전까지 계속 쓸 수 있어요.`,
        confirmLabel: '권한 주기',
      });
      if (!ok) return;
    }
    setBusy(approve ? 'approve' : 'deny');
    try {
      const res = MOCK ? { status: approve ? 'APPROVED' : 'REJECTED', connection_scopes: [...r.connection_scopes, ...(approve ? r.scopes : [])] } : await decideAiPermission(r.id, approve);
      setR({ ...r, status: res.status as AiPermissionRequest['status'], connection_scopes: res.connection_scopes, decided_at: new Date().toISOString() });
      toast(approve ? '권한을 줬어요.' : '요청을 거절했어요.', approve ? 'success' : 'info');
    } catch (err) {
      toast(errorMessage(err, '처리하지 못했어요. 잠시 후 다시 해 주세요.'), 'error');
      // 이미 처리됐거나 시간이 지난 요청은 최신 상태로 다시 보여 준다
      aiPermissionRequest(r.id).then(setR, () => {});
    } finally {
      setBusy(null);
      window.scrollTo({ top: 0, behavior: 'smooth' });
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
            <Link id="wizardBack" className="ui-btn is-soft" to="/">홈으로</Link>
            <button type="button" id="wizardNext" className="ui-btn is-primary" onClick={load}>다시 시도</button>
          </div>
        </AiFlowFrame>
      </AiFlowPage>
    );
  }
  if (!r) return <AiFlowPage><AiFlowFrame><AiFlowLoading text="요청을 불러오고 있어요…" /></AiFlowFrame></AiFlowPage>;

  const app = r.connection_label ?? r.client_name;
  if (!status) return null;
  const pending = status === 'PENDING';
  const soon = pending && new Date(r.expires_at).getTime() - now < 5 * 60_000;

  const until = new Date(r.expires_at).toLocaleTimeString('ko-KR', { hour: '2-digit', minute: '2-digit' });

  return (
    <AiFlowPage>
      <AiFlowFrame>
        {pending ? (
          <AiFlowHero
            app={app} brand={appBrandForName(r.client_name)} kicker={TITLE}
            title={`${app}에게\n권한을 더 줄까요?`}
            sub="주면 연결을 끊기 전까지 계속 쓸 수 있어요. 내가 시작한 요청이 아니라면 거절해 주세요."
          />
        ) : (
          <AiFlowHero icon={DONE_ICON[status]} kicker={TITLE} title={DONE_TITLE[status]} sub={STATUS_TEXT[status]} />
        )}

        <AiSetCard title={status === 'APPROVED' ? '새로 준 권한' : '요청한 권한'}>
          {r.scopes.map(s => (
            <AiSetItem key={s} title={SCOPE_LABEL[s] ?? s} tag={STRONG[s] && <Tag tone="amber">{STRONG[s]}</Tag>}>
              {SCOPE_HELP[s]}
            </AiSetItem>
          ))}
        </AiSetCard>

        <AiSetCard title="요청 내용">
          {!pending && <AiSetRow label="요청한 AI 앱" value={app} />}
          <AiSetRow label="작업 공간" value={workspaceName(r.org_name)} />
          {r.reason && <AiSetRow label="요청 이유" value={r.reason} stack />}
          <AiSetRow label="지금 있는 권한" value={r.connection_scopes.map(s => SCOPE_LABEL[s] ?? s).join('\n')} />
          {pending && <AiSetRow label="답할 수 있는 시간" value={<span className={soon ? 'aq-flow-soon' : undefined}>{timeLeft(r.expires_at, now)}<small>{until}까지</small></span>} />}
        </AiSetCard>

        {pending ? (
          <>
            <div className="step-actions">
              <button type="button" id="wizardBack" className="ui-btn is-soft" disabled={!!busy} onClick={() => void decide(false)}>거절</button>
              <button type="button" id="wizardNext" className={`ui-btn is-primary${busy === 'approve' ? ' is-busy' : ''}`} disabled={!!busy} onClick={() => void decide(true)}>권한 주기</button>
            </div>
            <p className="aq-connect-deny">
              <button type="button" className="ui-btn is-text" disabled={!!busy} onClick={() => void decide(false)}>거절하기</button>
            </p>
          </>
        ) : (
          <>
            <AiNextRequest current={r.id} />
            <div className="step-actions">
              <Link id="wizardBack" className="ui-btn is-soft" to="/profile?open=ai">연결된 AI 앱</Link>
              <Link id="wizardNext" className="ui-btn is-primary" to="/">홈으로</Link>
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
