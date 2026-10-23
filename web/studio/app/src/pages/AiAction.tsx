// AI 앱이 요청한 작업 승인 (/ai/actions/:id) — 발매 신청 취소, 서비스 중단, 수익 분배,
// 서비스·계약 종료 예약, 이의 제기, 멤버 초대, 한도 넘는 지급. 승인하면 내 계정으로 바로
// 실행돼요(스튜디오에서 직접 하는 것과 같아요). 24시간 안에 답하지 않으면 요청은 사라져요.
// crates/core/src/mcp/actions.rs
// AI 앱 연결 화면과 같은 틀(components/AiFlow): 누가 → 무엇을 → 주의할 점, 아래 고정 버튼.
import { useCallback, useEffect, useState } from 'react';
import { Link, useParams } from '../lib/router';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { ApiError, errorMessage, messageForCode } from '../api/errors';
import { appBrandForName, aiActionRequest, decideAiAction, type AiActionRequest } from '../api/aiConnections';
import { MOCK } from '../lib/mode';
import { josa } from '../lib/josa';
import { AiRequestMissing } from '../components/AiRequestMissing';
import { AiFlowFrame, AiFlowHeader, AiFlowHero, AiFlowLoading, AiFlowPage, AiSetCard, AiSetRow, AiNextRequest, timeLeft, useFreshRequest, useNow, workspaceName } from '../components/AiFlow';
import type { Tone } from '../ui';

const TITLE = '작업 승인 요청';

const DONE_TITLE: Record<Exclude<AiActionRequest['status'], 'PENDING'>, string> = {
  RUNNING: '처리하고 있어요',
  DONE: '처리했어요',
  FAILED: '처리하지 못했어요',
  REJECTED: '거절했어요',
  EXPIRED: '답할 수 있는 시간이 지났어요',
  REVOKED: '이미 끊긴 연결이에요',
};
type Status = AiActionRequest['status'];
const DONE_ICON: Record<Status, { kind: 'ok' | 'off' | 'warn'; tone: Tone }> = {
  PENDING: { kind: 'warn', tone: 'amber' }, RUNNING: { kind: 'warn', tone: 'blue' }, DONE: { kind: 'ok', tone: 'green' },
  FAILED: { kind: 'warn', tone: 'red' }, REJECTED: { kind: 'off', tone: 'gray' }, EXPIRED: { kind: 'warn', tone: 'gray' },
  REVOKED: { kind: 'warn', tone: 'gray' },
};
const STATUS_TEXT: Record<AiActionRequest['status'], string> = {
  PENDING: '',
  RUNNING: '끝나면 이 화면에 바로 결과가 나와요.',
  DONE: 'AI 앱 대화로 돌아가 이어서 진행해 주세요. 바로 이어지지 않으면 "승인했어"라고 말해 주세요.',
  FAILED: '승인은 했지만 처리 중에 문제가 생겼어요.',
  REJECTED: '아무것도 바뀌지 않았어요.',
  EXPIRED: '요청은 24시간 동안만 답할 수 있어요. 필요하면 AI 앱에 다시 요청해 달라고 말해 주세요.',
  REVOKED: '끊긴 연결의 요청은 처리할 수 없어요.',
  WITHDRAWN: 'AI 앱에서 요청을 거둬들였어요. 아무것도 바뀌지 않았어요.',
};

const demo = (id: string): AiActionRequest => ({
  id, action: 'TAKEDOWN', status: 'PENDING',
  summary: {
    title: '서비스 중단 요청',
    lines: [{ label: '발매', value: '밤의 산책' }, { label: '사유', value: '새 앨범으로 다시 낼 거예요' }],
    warning: 'AUDENIQ이 검토한 뒤 모든 플랫폼에서 이 발매를 내려요. 내려간 발매를 다시 올리려면 새로 신청해야 해요.',
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

  const load = useCallback(() => {
    if (MOCK) { setR(prev => prev ?? demo(id)); return; }
    setError('');
    aiActionRequest(id).then(setR, err => (err instanceof ApiError && err.status === 404 ? setMissing(true) : setError(errorMessage(err, '요청을 불러오지 못했어요. 잠시 후 다시 해 주세요.'))));
  }, [id]);
  useEffect(load, [load]);
  const now = useNow();
  // 시간이 지나면 버튼을 누르기 전에 바로 '시간이 지났어요'로
  const status: Status | null = !r ? null
    : r.connection_revoked && r.status === 'PENDING' ? 'REVOKED'
    : r.status === 'PENDING' && new Date(r.expires_at).getTime() <= now ? 'EXPIRED'
    : r.status;
  useFreshRequest(load, { active: status === 'PENDING' || status === 'RUNNING', poll: status === 'RUNNING' });

  async function decide(approve: boolean) {
    if (!r || busy) return;
    if (approve) {
      const ok = await confirm({
        title: '승인하면 바로 처리해요',
        message: r.summary.warning || `${josa(r.summary.title, '을/를')} 내 계정으로 처리해요.`,
        confirmLabel: '승인하기',
      });
      if (!ok) return;
    }
    setBusy(approve ? 'approve' : 'deny');
    try {
      const res = MOCK ? { status: approve ? 'DONE' : 'REJECTED' } : await decideAiAction(r.id, approve);
      setR({ ...r, status: res.status as AiActionRequest['status'], decided_at: new Date().toISOString() });
      if (!approve) toast('요청을 거절했어요.', 'info');
      else if (res.status === 'DONE') toast('승인하고 처리했어요.', 'success');
      else if (res.status === 'RUNNING') toast('승인했어요. 처리를 시작했어요.', 'success');
    } catch (err) {
      toast(errorMessage(err, '처리하지 못했어요. 잠시 후 다시 해 주세요.'), 'error');
      // 실패·만료·이미 처리된 요청은 최신 상태로 다시 보여 준다
      aiActionRequest(r.id).then(setR, () => {});
    } finally {
      setBusy(null);
      window.scrollTo({ top: 0, behavior: 'smooth' });
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

  const until = new Date(r.expires_at).toLocaleString('ko-KR', { month: 'long', day: 'numeric', hour: '2-digit', minute: '2-digit' });

  return (
    <AiFlowPage>
      <AiFlowFrame>
        {pending ? (
          <AiFlowHero
            app={app} brand={appBrandForName(r.client_name)} kicker={TITLE}
            title={`${josa(r.summary.title, '을/를')}\n승인할까요?`}
            sub={`${josa(app, '이/가')} 요청했어요. 승인하면 바로 내 계정으로 처리해요. 내가 시작한 요청이 아니라면 거절해 주세요.`}
          />
        ) : (
          <AiFlowHero
            icon={DONE_ICON[status]} kicker={TITLE} title={DONE_TITLE[status]}
            sub={<>{STATUS_TEXT[status]}{status === 'FAILED' && r.error_code && <> {messageForCode(r.error_code)}</>}</>}
          />
        )}

        <AiSetCard title="요청한 작업">
          {!pending && <AiSetRow label="작업" value={r.summary.title} />}
          {r.summary.lines.map(l => <AiSetRow key={l.label} label={l.label} value={l.value} stack={l.value.length > 18} />)}
          <AiSetRow label="요청한 AI 앱" value={app} />
          <AiSetRow label="작업 공간" value={workspaceName(r.org_name)} />
          {pending && <AiSetRow label="답할 수 있는 시간" value={<span className={soon ? 'aq-flow-soon' : undefined}>{timeLeft(r.expires_at, now)}<small>{until}까지</small></span>} />}
          {r.summary.warning && <p className="aq-flow-note">{r.summary.warning}</p>}
        </AiSetCard>

        {pending ? (
          <>
            <div className="step-actions">
              <button type="button" id="wizardBack" className="ui-btn is-soft" disabled={!!busy} onClick={() => void decide(false)}>거절</button>
              <button type="button" id="wizardNext" className={`ui-btn is-primary${busy === 'approve' ? ' is-busy' : ''}`} disabled={!!busy} onClick={() => void decide(true)}>승인하기</button>
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
              {status === 'DONE' && r.release_id
                ? <Link id="wizardNext" className="ui-btn is-primary" to={`/releases/${r.release_id}`}>발매 보기</Link>
                : <Link id="wizardNext" className="ui-btn is-primary" to="/">홈으로</Link>}
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
