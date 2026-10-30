// 발매 상세 '정보' 탭에서 곡을 누르면 열리는 곡 상세 — 서버 트랙 + 신청서의 곡 정보(크레딧·가사·음원 규격).
// ISRC는 바로 복사하고, 고칠 일은 상태에 맞는 곳(신청서 수정 · 정보 수정 요청)으로 보낸다.
import type { DraftTrack, Track } from '../api/types';
import { durationLabel } from '../lib/catalog';
import { Badge } from '../ui';
import { CopyButton } from './CopyButton';
import { Modal } from './Modal';
import { TrackWork } from './publishing/TrackWork';
import '../styles/split/ReleaseDetail.css';

const Empty = ({ children }: { children: string }) => <span className="aq-empty-val">{children}</span>;

export function TrackSheet({ track, draft, no, onClose, edit, releaseId }: {
  track: Track; draft?: DraftTrack; no: number; onClose: () => void;
  /** 있으면 '작품·권리'(저작물 연결)를 함께 보여 준다 */
  releaseId?: string;
  /** 고칠 곳 — 없으면 버튼을 그리지 않는다 */
  edit?: { label: string; go: () => void } | null;
}) {
  const t = track;
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
        {releaseId && <TrackWork releaseId={releaseId} trackId={t.id} />}
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
