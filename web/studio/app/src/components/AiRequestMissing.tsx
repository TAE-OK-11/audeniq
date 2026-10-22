// AI 앱이 보낸 승인 링크를 열었는데 요청이 안 보일 때 (/ai/approve/:id, /ai/actions/:id).
// 서버는 다른 계정의 요청을 "없음"으로만 답하므로, 흔한 이유를 차례로 보여 주고 다음 할 일을 알려 준다.
import { Link } from '../lib/router';
import { useAuth } from '../api/auth';
import { AiFlowFrame, AiFlowHero, AiSetCard, AiSetItem } from './AiFlow';

const REASONS = (hours: string, email?: string): [string, string][] => [
  ['다른 계정으로 로그인했어요', `${email ? `지금은 ${email}로 로그인했어요. ` : ''}AI 앱을 연결한 AUDENIQ 계정으로 로그인하면 요청이 보여요.`],
  ['답할 수 있는 시간이 지났어요', `요청은 ${hours} 동안만 답할 수 있어요.`],
  ['이미 답했거나 연결을 끊었어요', '답한 요청과 끊긴 연결의 요청은 다시 열 수 없어요.'],
];

export function AiRequestMissing({ title, hours }: { title: string; hours: string }) {
  const { user } = useAuth();
  return (
    <AiFlowFrame>
      <div role="alert">
        <AiFlowHero icon={{ kind: 'warn', tone: 'gray' }} kicker={title} title="요청을 찾지 못했어요" sub="이 링크로는 요청을 열 수 없어요. 이유는 보통 셋 중 하나예요." />
        <AiSetCard title="열리지 않는 이유">
          {REASONS(hours, user?.email).map(([t, d]) => <AiSetItem key={t} title={t}>{d}</AiSetItem>)}
        </AiSetCard>
        <p className="aq-flow-fine">AI 앱에 "다시 요청해 줘"라고 말하면 새 링크를 받을 수 있어요.</p>
      </div>
      <div className="step-actions">
        <Link id="wizardBack" className="ui-btn is-soft" to="/profile?open=ai">연결된 AI 앱 보기</Link>
        <Link id="wizardNext" className="ui-btn is-primary" to="/">홈으로</Link>
      </div>
      <p className="aq-connect-deny">
        <Link className="ui-btn is-text" to="/profile?open=ai">연결된 AI 앱 보기</Link>
      </p>
    </AiFlowFrame>
  );
}
