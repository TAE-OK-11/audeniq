// AI 앱이 요청한 작업 승인 (/ai/actions/:id) — 발매 신청 취소, 서비스 중단, 수익 분배,
// 서비스·계약 종료 예약, 이의 제기, 멤버 초대, 한도 넘는 지급. 승인하면 내 계정으로 바로
// 실행돼요(스튜디오에서 직접 하는 것과 같아요). 24시간 안에 답하지 않으면 요청은 사라져요.
// crates/core/src/mcp/actions.rs
// AI 앱 연결 화면과 같은 틀(components/AiFlow): 누가 → 무엇을 → 주의할 점, 아래 고정 버튼.
import { useEffect, useState } from 'react';
import { Link, useParams } from '../lib/router';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { ApiError, errorMessage, messageForCode } from '../api/errors';
import { aiActionRequest, decideAiAction, type AiActionRequest } from '../api/aiConnections';
import { MOCK } from '../lib/mode';
import { AiRequestMissing } from '../components/AiRequestMissing';
import { AiFlowFrame, AiFlowHeader, AiFlowHero, AiFlowLoading, AiFlowPage, AiRequestApp, workspaceName } from '../components/AiFlow';
import { Badge, Callout, Card, KeyValue, type Tone } from '../ui';

const TITLE = '작업 승인 요청';

const DONE_TITLE: Record<Exclude<AiActionRequest['status'], 'PENDING'>, string> = {
  RUNNING: '처리하고 있어요',
  DONE: '승인하고 처리했어요',
  FAILED: '작업을 마치지 못했어요',
  REJECTED: '거절했어요',
  EXPIRED: '요청 시간이 지났어요',
  REVOKED: '이미 끊긴 연결이에요',
};
type Status = AiActionRequest['status'];
const BADGE: Record<Status, [Tone, string]> = {
  PENDING: ['amber', '답을 기다려요'], RUNNING: ['blue', '처리 중'], DONE: ['green', '처리함'], FAILED: ['red', '처리 못 함'],
  REJECTED: ['gray', '거절함'], EXPIRED: ['gray', '시간 지남'], REVOKED: ['gray', '연결 끊김'],
};
const DONE_ICON: Record<Status, { kind: 'ok' | 'off' | 'warn'; tone: Tone }> = {
  PENDING: { kind: 'warn', tone: 'amber' }, RUNNING: { kind: 'warn', tone: 'blue' }, DONE: { kind: 'ok', tone: 'green' },
  FAILED: { kind: 'warn', tone: 'red' }, REJECTED: { kind: 'off', tone: 'gray' }, EXPIRED: { kind: 'warn', tone: 'gray' },
  REVOKED: { kind: 'warn', tone: 'gray' },
};
const STATUS_TEXT: Record<AiActionRequest['status'], string> = {
  PENDING: '',
  RUNNING: '잠시 뒤에 다시 확인해 주세요.',
  DONE: '요청한 작업을 마쳤어요. AI 앱에서 계속 진행해 주세요.',
  FAILED: '승인은 했어요.',
  REJECTED: '아무것도 바뀌지 않았어요.',
  EXPIRED: '요청 시간(24시간)이 지났어요. 필요하면 AI 앱에서 다시 요청해 주세요.',
  REVOKED: '이 연결은 이미 끊겼어요.',
};

const demo = (id: string): AiActionRequest => ({
  id, action: 'TAKEDOWN', status: 'PENDING',
  summary: {
    title: '서비스 중단 요청',
    lines: [{ label: '발매', value: '밤의 산책' }, { label: '사유', value: '새 앨범으로 다시 낼 거예요' }],
    warning: '어데닉이 검토한 뒤 모든 플랫폼에서 이 발매를 내려요. 내려간 발매를 다시 올리려면 새로 신청해야 해요.',
  },
  client_name: 'Claude', org_id: 'o1', org_name: 'Personal workspace', release_id: 'r1',
  created_at: new Date().toISOString(), expires_at: new Date(Date.now() + 86_400_000).toISOString(), decided_at: null,
  error_code: null, connection_label: null, connection_revoked: false,
});

export function AiAction() {
  const { id = '' } = useParams<{ id: string }>();
  const toast = useToast();
  const confirm = useConfirm();
  const [r, setR] = useState<AiActionRequest | null>(null);
  const [error, setError] = useState('');
  const [missing, setMissing] = useState(false);
  const [busy, setBusy] = useState<'approve' | 'deny' | null>(null);

  useEffect(() => {
    if (MOCK) { setR(demo(id)); return; }
    aiActionRequest(id).then(setR, err => (err instanceof ApiError && err.status === 404 ? setMissing(true) : setError(errorMessage(err, '요청을 불러오지 못했어요. 잠시 후 다시 열어 주세요.'))));
  }, [id]);

  async function decide(approve: boolean) {
    if (!r || busy) return;
    if (approve) {
      const ok = await confirm({
        title: `${r.summary.title}을(를) 승인할까요?`,
        message: `승인하면 내 계정으로 바로 처리돼요. ${r.summary.warning}`,
        confirmLabel: '승인하고 실행',
      });
      if (!ok) return;
    }
    setBusy(approve ? 'approve' : 'deny');
    try {
      const res = MOCK ? { status: approve ? 'DONE' : 'REJECTED' } : await decideAiAction(r.id, approve);
      setR({ ...r, status: res.status as AiActionRequest['status'], decided_at: new Date().toISOString() });
      toast(approve ? '승인하고 처리했어요. AI 앱에서 계속 진행해 주세요.' : '요청을 거절했어요.', approve ? 'success' : 'info');
    } catch (err) {
      toast(errorMessage(err, '처리하지 못했어요.'), 'error');
      // 실패·만료·이미 처리된 요청은 최신 상태로 다시 보여 준다
      aiActionRequest(r.id).then(setR, () => {});
    } finally {
      setBusy(null);
    }
  }

  if (missing) return <AiFlowPage><AiRequestMissing title={TITLE} hours="24시간" /></AiFlowPage>;
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

  const until = new Date(r.expires_at).toLocaleString('ko-KR', { month: 'long', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  const [badgeTone, badgeText] = BADGE[status];

  return (
    <AiFlowPage>
      <AiFlowFrame>
        {pending ? (
          <AiFlowHero
            kicker={TITLE}
            title={`${app}에서\n작업 승인을 요청했어요`}
            sub="승인하면 내 계정으로 바로 처리돼요. 내가 요청한 게 아니라면 거절해 주세요."
          />
        ) : (
          <AiFlowHero
            icon={DONE_ICON[status]} kicker={TITLE} title={DONE_TITLE[status]}
            sub={<>{STATUS_TEXT[status]}{status === 'FAILED' && r.error_code && <> {messageForCode(r.error_code)}</>}</>}
          />
        )}

        <AiRequestApp
          name={app}
          meta={[workspaceName(r.org_name)]}
          due={pending ? until : undefined}
          badge={<Badge tone={badgeTone} dot>{badgeText}</Badge>}
        />

        <Card className="aq-flow-card">
          <h3>{r.summary.title}</h3>
          {r.summary.lines.length > 0 && <KeyValue cols={2} items={r.summary.lines.map(l => [l.label, l.value] as const)} />}
          {r.summary.warning && <Callout tone="warn">{r.summary.warning}</Callout>}
        </Card>

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
