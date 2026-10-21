// AI 앱이 요청한 작업 승인 (/ai/actions/:id) — 발매 신청 취소, 서비스 중단, 수익 분배,
// 서비스·계약 종료 예약, 이의 제기, 멤버 초대, 한도 넘는 지급. 승인하면 내 계정으로 바로
// 실행돼요(스튜디오에서 직접 하는 것과 같아요). 24시간 안에 답하지 않으면 요청은 사라져요.
// crates/core/src/mcp/actions.rs
import { useEffect, useState } from 'react';
import { Link, useParams } from '../lib/router';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { errorMessage, messageForCode } from '../api/errors';
import { aiActionRequest, decideAiAction, type AiActionRequest } from '../api/aiConnections';
import { MOCK } from '../lib/mode';
import { PageHeader } from '../ui';

const workspaceName = (name: string) => (name === 'Personal workspace' ? '내 작업 공간' : name);

const STATUS_TEXT: Record<AiActionRequest['status'], string> = {
  PENDING: '',
  RUNNING: '처리하고 있어요. 잠시 뒤에 다시 확인해 주세요.',
  DONE: '승인했고 요청한 작업을 마쳤어요.',
  FAILED: '승인했지만 작업을 마치지 못했어요.',
  REJECTED: '거절했어요. 아무것도 바뀌지 않았어요.',
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
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (MOCK) { setR(demo(id)); return; }
    aiActionRequest(id).then(setR, err => setError(errorMessage(err, '요청을 찾지 못했어요. 링크가 맞는지, 요청한 계정으로 로그인했는지 확인해 주세요.')));
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
    setBusy(true);
    try {
      const res = MOCK ? { status: approve ? 'DONE' : 'REJECTED' } : await decideAiAction(r.id, approve);
      setR({ ...r, status: res.status as AiActionRequest['status'], decided_at: new Date().toISOString() });
      toast(approve ? '승인하고 처리했어요. AI 앱에서 계속 진행해 주세요.' : '요청을 거절했어요.', approve ? 'success' : 'info');
    } catch (err) {
      toast(errorMessage(err, '처리하지 못했어요.'), 'error');
      // 실패·만료·이미 처리된 요청은 최신 상태로 다시 보여 준다
      aiActionRequest(r.id).then(setR, () => {});
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="view-enter aq-ai-approve">
      <PageHeader eyebrow="AI 앱 연결" title="작업 승인 요청" sub="AI 앱이 아래 작업을 해도 되는지 물어봤어요. 내가 요청한 게 아니라면 거절하고 연결을 끊어 주세요." />
      {error && <div className="ui-callout is-danger" role="alert">{error}</div>}
      {!r && !error && <p role="status">불러오는 중이에요.</p>}
      {r && (
        <section className="ui-card aq-ai-approve-card">
          <dl className="aq-ai-approve-meta">
            <dt>AI 앱</dt><dd><b>{r.connection_label ?? r.client_name}</b></dd>
            <dt>작업 공간</dt><dd>{workspaceName(r.org_name)}</dd>
            <dt>요청한 작업</dt><dd><b>{r.summary.title}</b></dd>
            {r.summary.lines.map(l => (
              <div key={l.label} className="aq-ai-action-line"><dt>{l.label}</dt><dd className="aq-ai-approve-reason">{l.value}</dd></div>
            ))}
          </dl>
          {r.summary.warning && <div className="ui-callout is-warn">{r.summary.warning}</div>}
          {r.status === 'PENDING' && !r.connection_revoked ? (
            <>
              <p className="ui-field-help">
                {new Date(r.expires_at).toLocaleString('ko-KR', { month: 'long', day: 'numeric', hour: '2-digit', minute: '2-digit' })}까지 답해 주세요.
              </p>
              <div className="aq-ai-approve-actions">
                <button type="button" className="ui-btn is-soft" disabled={busy} onClick={() => void decide(false)}>거절</button>
                <button type="button" className={`ui-btn is-primary${busy ? ' is-busy' : ''}`} disabled={busy} onClick={() => void decide(true)}>승인</button>
              </div>
            </>
          ) : (
            <div className={`ui-callout${r.status === 'DONE' ? ' is-success' : r.status === 'FAILED' ? ' is-danger' : ''}`} role="status">
              {r.connection_revoked && r.status === 'PENDING' ? STATUS_TEXT.REVOKED : STATUS_TEXT[r.status]}
              {r.status === 'FAILED' && r.error_code && <> {messageForCode(r.error_code)}</>}
            </div>
          )}
          <p className="ui-field-help"><Link to="/profile?open=ai">연결된 AI 앱 관리</Link></p>
        </section>
      )}
    </div>
  );
}
