// 발매 상세 > 유튜브 Content ID — 담당자가 유튜브에서 확인한 상태(켜짐·수익 창출·제외 채널)와 요청 바로가기.
import { useEffect, useState } from 'react';
import { Link } from '../lib/router';
import { orgPath, req } from '../api/http';
import { MOCK } from '../lib/mode';
import { MONETIZATION_LABEL } from '../lib/platformRequests';
import { ManageHead } from './ManageHead';

interface State { enabled: boolean; monetization: string; allowlisted_channels: string[]; note: string; updated_at: string | null }

export function ContentIdCard({ releaseId }: { releaseId: string }) {
  const [st, setSt] = useState<State | null>(null);
  useEffect(() => {
    if (MOCK) return;
    req<State>(orgPath(`/releases/${encodeURIComponent(releaseId)}/content-id`), { quiet401: true }).then(setSt, () => setSt(null));
  }, [releaseId]);
  if (!st) return null;
  return (
    <section className="ui-card aq-manage-card aq-cid-card" aria-labelledby="cidHeading">
      <ManageHead id="cidHeading" icon="disc" title="유튜브 Content ID" status={st.enabled ? { tone: 'green', label: '켜짐' } : { tone: 'gray', label: '꺼짐' }} />
      <p className="ui-field-help">유튜브 영상에 이 발매의 음원이 쓰이면 소유권을 주장하고 수익을 받아요. 담당자가 유튜브에서 확인한 상태예요.</p>
      <dl className="ui-kv is-2">
        <div><dt>수익</dt><dd>{MONETIZATION_LABEL[st.monetization] ?? st.monetization}</dd></div>
        <div><dt>소유권 주장 제외 채널</dt><dd>{st.allowlisted_channels.length ? `${st.allowlisted_channels.length}개` : '없음'}</dd></div>
      </dl>
      {st.allowlisted_channels.length > 0 && (
        <p className="small">소유권 주장 제외 채널: {st.allowlisted_channels.map(c => c.replace(/^https:\/\/(www\.)?/, '')).join(', ')}</p>
      )}
      {st.note && <p className="small muted">{st.note}</p>}
      <div className="ui-actions">
        <Link className="ui-btn" to={`/inquiries?new=request&group=content_id&release=${encodeURIComponent(releaseId)}`}>채널 제외·이의제기 요청</Link>
      </div>
    </section>
  );
}
