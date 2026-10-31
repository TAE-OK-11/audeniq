// 발매 상세에서 곡을 누르면 열리는 곡 시트 하나 — 서버 트랙 + 신청서의 곡 정보(크레딧·가사·음원 규격),
// 그리고 크레딧 바로 아래 작사·작곡 저작권(참여자·지분·관리 상태·첨부 자료). 배급 정보와 저작권을 다른 창으로 나누지 않는다.
// ISRC는 바로 복사하고, 고칠 일은 상태에 맞는 곳(신청서 수정 · 정보 수정 요청)으로 보낸다.
import { Suspense, useEffect } from 'react';
import type { DraftTrack, Track } from '../api/types';
import { durationLabel } from '../lib/catalog';
import { CopyButton } from './CopyButton';
import { Modal } from './Modal';
import { Link } from '../lib/router';
import { pubApi, type Work } from '../api/publishing';
import { useAsync } from '../hooks/useAsync';
import { usePublishingUse } from '../hooks/usePublishingUse';
import type { PrepField } from '../lib/publishingPrep';
import { lazyComponent } from '../lib/lazyComponent';
import { Badge, IconTile } from '../ui';
import { Glyph } from './Glyph';
import '../styles/split/ReleaseDetail.css';

// 저작권 칸은 곡이 저작권 정보를 가졌을 때만 받는다 — 배급만 쓰는 사람의 곡 시트는 가볍게
const WorkRights = lazyComponent(() => import('./publishing/WorkSheet'), m => m.WorkRights);

const Empty = ({ children }: { children: string }) => <span className="aq-empty-val">{children}</span>;

export function TrackSheet({ track, draft, no, onClose, edit, releaseId, rightsFocus, onRightsSaved, toRights }: {
  track: Track; draft?: DraftTrack; no: number; onClose: () => void;
  /** 있으면 크레딧 아래에 이 곡의 작사·작곡 저작권 */
  releaseId?: string;
  /** 저작권 칸의 이 자리로 바로 (준비 점검에서 눌러 들어왔을 때) */
  rightsFocus?: { field: PrepField; contributorId?: string } | null;
  onRightsSaved?: (w: Work) => void;
  /** 저작권 칸이 보이게 내려서 연다 (저작권 관리 신청 칸에서 곡을 눌렀을 때) */
  toRights?: boolean;
  /** 고칠 곳 — 없으면 버튼을 그리지 않는다 */
  edit?: { label: string; go: () => void } | null;
}) {
  const t = track;
  const { data: works, reload } = useAsync(() => (releaseId ? pubApi.listWorks() : Promise.resolve(null)), [releaseId], { key: releaseId ? 'works' : undefined });
  const pub = usePublishingUse();
  const work = works?.find(w => w.recordings.some(r => r.trackId === t.id && (!releaseId || r.releaseId === releaseId)));
  const hasWork = !!work;
  useEffect(() => {
    if (!toRights || !hasWork || rightsFocus) return;
    const id = window.setTimeout(() => document.getElementById('trackRights')?.scrollIntoView({ behavior: 'smooth', block: 'start' }), 250);
    return () => window.clearTimeout(id);
  }, [toRights, hasWork, rightsFocus]);
  const composers = t.composers ?? draft?.composers ?? '';
  const lyricists = t.lyricists ?? draft?.lyricists ?? '';
  const lyrics = draft?.lyrics?.trim() ?? '';
  const credits: [string, string][] = [
    ['작곡', composers], ['작사', draft?.instrumental ? '연주곡' : lyricists],
    ['편곡', draft?.arrangers ?? ''], ['연주', draft?.performers ?? ''], ['프로듀서', draft?.producer ?? ''],
  ];
  return (
    <Modal title={`${no}. ${t.title || '곡명 없음'}`} onClose={onClose}>
      <div className="aq-track-sheet">
        <div className="aq-track-sheet-tags">
          {t.version && <Badge tone="gray">{t.version}</Badge>}
          {draft?.featuring && <Badge tone="blue">feat. {draft.featuring}</Badge>}
          {t.explicit && <Badge tone="red">Explicit</Badge>}
          {draft?.instrumental && <Badge tone="violet">연주곡</Badge>}
        </div>
        <dl className="ui-kv is-2">
          <div><dt>길이</dt><dd>{t.duration_ms ? durationLabel(t.duration_ms) : <Empty>확인 전</Empty>}</dd></div>
          <div><dt>미리듣기 시작</dt><dd>{draft?.previewStart ? `${draft.previewStart}초부터` : <Empty>처음부터</Empty>}</dd></div>
          <div className="is-wide">
            <dt>ISRC</dt>
            <dd className="aq-code-line">
              {t.isrc ? <><span className="is-mono">{t.isrc}</span><CopyButton text={t.isrc} className="ui-btn is-xs is-white" /></> : <Empty>승인되면 발급돼요</Empty>}
            </dd>
          </div>
          <div className="is-wide">
            <dt>음원 파일</dt>
            <dd className="break">{t.audioName ? <>{t.audioName}{draft?.audioSpec && <span className="aq-empty-val"> · {draft.audioSpec}</span>}</> : <Empty>올리기 전</Empty>}</dd>
          </div>
        </dl>
        <h3 className="aq-track-sheet-h">크레딧</h3>
        <dl className="ui-kv is-2">
          {credits.filter(([k, v]) => v || k === '작곡' || k === '작사').map(([k, v]) => (
            <div key={k}><dt>{k}</dt><dd>{v || <Empty>입력 전</Empty>}</dd></div>
          ))}
        </dl>
        {work ? (
          <Suspense fallback={null}><WorkRights work={work} all={works ?? []} focus={rightsFocus} releaseId={releaseId}
            onSaved={w => { reload(); onRightsSaved?.(w); }} onDone={onClose} /></Suspense>
        ) : releaseId && pub.started ? (
          // 저작권을 쓰는 사람에게만 — 아직 정리하지 않은 곡은 여기서 바로
          <Link to={`/works/new?release=${encodeURIComponent(releaseId)}&track=${encodeURIComponent(t.id)}`} className="ui-row">
            <IconTile size="sm"><Glyph name="scale" size={16} /></IconTile>
            <span className="min-0"><span className="ui-row-title">작사·작곡 저작권 정리</span><span className="ui-row-meta"><span>참여자와 지분을 적어 두면 저작권 관리를 신청할 수 있어요</span></span></span>
            <Glyph name="chevron-right" size={16} />
          </Link>
        ) : null}
        {!draft?.instrumental && (
          <>
            <h3 className="aq-track-sheet-h">가사</h3>
            {lyrics ? <p className="aq-track-sheet-lyrics">{lyrics}</p> : <p className="aq-empty-val">등록된 가사가 없어요.</p>}
          </>
        )}
        {edit && <button type="button" className="ui-btn is-soft is-block" onClick={edit.go}>{edit.label}</button>}
      </div>
    </Modal>
  );
}
