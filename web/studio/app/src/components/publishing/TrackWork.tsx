// 곡 상세의 '곡·권리' — 이 녹음이 어떤 곡인지, 권리 관리가 어디까지 왔는지 (기획안 4.2 흐름 3~5).
// 곡 상세의 다른 칸과 같은 ui-kv · ui-row. 연결된 곡이 없으면 '곡 연결 또는 생성'으로 —
// 발매 정보를 미리 채운 곡 정보 만들기 화면에서 기존 곡 연결도 고를 수 있다
import { Link } from '../../lib/router';
import { pubApi } from '../../api/publishing';
import { useAsync } from '../../hooks/useAsync';
import { pct } from '../../lib/publishing';
import { Glyph } from '../Glyph';
import { IconTile } from '../../ui';
import { RegBadge, roleText } from './parts';

export function TrackWork({ releaseId, trackId }: { releaseId: string; trackId: string }) {
  const { data: work, loading } = useAsync(() => pubApi.workByTrack(trackId), [trackId], { key: `work-by-track:${trackId}` });
  return (
    <>
      <h3 className="aq-track-sheet-h">곡·권리</h3>
      {loading && work === undefined ? (
        <p className="aq-empty-val">불러오는 중…</p>
      ) : work ? (
        <Link to={`/works/${work.id}`} className="ui-row">
          <IconTile size="sm"><Glyph name="scale" size={16} /></IconTile>
          <span className="min-0">
            <span className="ui-row-title">{work.title}</span>
            <span className="ui-row-meta">{work.contributors.map(c => <span key={c.id}>{c.name} {roleText(c)} {pct(c.share)}</span>)}</span>
          </span>
          <span className="ui-row-end"><RegBadge status={work.registration} /><small>{work.iswc ?? work.internalCode}</small></span>
        </Link>
      ) : (
        <>
          <p className="aq-empty-val">아직 곡과 연결되지 않았어요. 작사·작곡 지분을 정리하고, 원하면 저작권 관리까지 맡길 수 있어요.</p>
          <Link to={`/works/new?release=${encodeURIComponent(releaseId)}&track=${encodeURIComponent(trackId)}`} className="ui-btn is-soft is-block">
            곡 연결 또는 생성
          </Link>
        </>
      )}
    </>
  );
}
