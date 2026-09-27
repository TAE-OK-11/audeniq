import { ErrorScreen } from '../components/ErrorScreen';
import { useLocation, useNavigate } from '../lib/router';

/** 없는 주소 — 스튜디오 헤더 아래에 404 화면 */
export function NotFound() {
  const nav = useNavigate();
  const loc = useLocation();
  return (
    <div id="view-not-found" className="view">
      <ErrorScreen
        kind="not-found" code="404"
        eyebrow="페이지 없음"
        title={<>찾는 페이지가<br /><em>여기에 없어요.</em></>}
        description={<>주소가 바뀌었거나 삭제된 페이지일 수 있어요.<br /><code className="aq-errscreen-path">{loc.pathname}</code></>}
        actions={[
          { label: '홈으로', onClick: () => nav('/'), primary: true },
          { label: '이전 화면', onClick: () => (window.history.length > 1 ? nav(-1) : nav('/')) },
        ]}
      />
    </div>
  );
}
