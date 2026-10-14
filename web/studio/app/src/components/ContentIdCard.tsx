// 발매 상세 > 유튜브 Content ID — 담당자가 유튜브에서 확인한 상태(켜짐·수익 창출·제외 채널)와 요청 바로가기.
import { useEffect, useState } from 'react';
import { Link } from '../lib/router';
import { orgPath, req } from '../api/http';
import { MOCK } from '../lib/mode';
import { MONETIZATION_LABEL } from '../lib/platformRequests';

interface State { enabled: boolean; monetization: string; allowlisted_channels: string[]; note: string; updated_at: string | null }

export function ContentIdCard({ releaseId }: { releaseId: string }) {
  const [st, setSt] = useState<State | null>(null);
  useEffect(() => {
    if (MOCK) return;
    req<State>(orgPath(`/releases/${encodeURIComponent(releaseId)}/content-id`), { quiet401: true }).then(setSt, () => setSt(null));
  }, [releaseId]);
  if (!st) return null;
  return (
    <section className="ui-card aq-cid-card" aria-labelledby="cidHeading">
      <h2 id="cidHeading">유튜브 Content ID</h2>
      <dl className="ui-kv is-2">
        <div><dt>상태</dt><dd>{st.enabled ? '켜짐' : '꺼짐'}</dd></div>
        <div><dt>수익</dt><dd>{MONETIZATION_LABEL[st.monetization] ?? st.monetization}</dd></div>
      </dl>
      {st.allowlisted_channels.length > 0 && (
        <p className="small">소유권 주장 제외 채널: {st.allowlisted_channels.map(c => c.replace(/^https:\/\/(www\.)?/, '')).join(', ')}</p>
      )}
      {st.note && <p className="small muted">{st.note}</p>}
      <p className="small"><Link to="/platform-requests?tab=content_id">채널 제외·소유권 주장 해제·이의제기 요청하기</Link></p>
    </section>
  );
}
