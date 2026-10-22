// AI 앱이 보낸 승인 링크를 열었는데 요청이 안 보일 때 (/ai/approve/:id, /ai/actions/:id).
// 서버는 다른 계정의 요청을 "없음"으로만 답하므로, 흔한 이유를 차례로 보여 주고 다음 할 일을 알려 준다.
import { Link } from '../lib/router';
import { AiFlowFrame, AiFlowHero, AiFlowStatusIcon } from './AiFlow';

export function AiRequestMissing({ title, hours }: { title: string; hours: string }) {
  return (
    <AiFlowFrame>
      <div role="alert">
        <AiFlowHero mark={<AiFlowStatusIcon kind="warn" />} tone="off" kicker={title} title="요청을 찾지 못했어요" sub="이럴 때 이 화면이 보여요." />
        <ol className="aq-flow-reasons">
          <li>
            <strong>다른 계정으로 로그인했어요</strong>
            <span>AI 앱을 연결한 어데닉 계정으로 로그인해야 요청이 보여요.</span>
          </li>
          <li>
            <strong>요청 시간이 지났어요</strong>
            <span>요청은 {hours} 안에 답해야 해요.</span>
          </li>
          <li>
            <strong>이미 처리했거나 연결을 끊었어요</strong>
            <span>처리한 요청과 끊긴 연결의 요청은 다시 열 수 없어요.</span>
          </li>
        </ol>
        <p className="aq-flow-fine">AI 앱에 "다시 요청해 줘"라고 말하면 새 링크를 받을 수 있어요.</p>
      </div>
      <div className="step-actions">
        <Link id="wizardBack" className="ui-btn is-soft" to="/profile?open=ai">연결된 AI 앱 보기</Link>
        <Link id="wizardNext" className="ui-btn is-primary" to="/">스튜디오 홈</Link>
      </div>
      <p className="aq-connect-deny">
        <Link className="ui-btn is-text" to="/profile?open=ai">연결된 AI 앱 보기</Link>
      </p>
    </AiFlowFrame>
  );
}
