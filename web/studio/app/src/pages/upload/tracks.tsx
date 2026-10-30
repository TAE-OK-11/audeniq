// 발매 신청서 — 트랙 입력 (곡 하나씩, memo로 다른 곡은 다시 그리지 않는다)
import { memo } from 'react';
import { FilePicker } from '../../components/FilePicker';
import { MOCK } from '../../api/client';
import { fileSize } from '../../lib/format';
import { Glyph } from '../../components/Glyph';
import { Badge } from '../../ui';
import { TrackChecks } from '../../components/TrackChecks';
import { addCredit, creditChips, type CreditHistory } from '../../lib/credits';
import { type Track, type UploadState } from './model';
import '../../styles/split/Upload.css';


/** 업로드 진행 표시 */
export function UploadStatus({ upload, idle }: { upload?: UploadState; idle: string }) {
  if (!upload || upload.state === 'done' || upload.state === 'received') return <p className="ui-field-help">{idle}</p>;
  if (upload.state === 'error') return <p className="ui-field-help aq-help-error" role="alert">{upload.message || '파일을 올리지 못했어요. 다시 선택해 주세요.'}</p>;
  const pct = Math.round(upload.pct * 100);
  return (
    <div className="aq-upload-progress" role="progressbar" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100} aria-label="업로드 진행률">
      <span><i style={{ ['--p' as string]: `${Math.max(4, pct)}%` }} /></span>
      <small>{pct < 100 ? `업로드 중 ${pct}%` : '거의 다 올렸어요…'}</small>
    </div>
  );
}

// 트랙 입력 행 — memo로 감싸 한 곡 입력 시 다른 곡이 리렌더되지 않도록 분리
/** 크레딧 칸 아래 — 예전 발매에 쓴 이름을 눌러서 덧붙인다 (칸에 이미 있는 이름은 빼고 다섯 개까지) */
export function CreditChips({ names, value, onAdd, label }: { names?: string[]; value: string; onAdd: (v: string) => void; label: string }) {
  const chips = names ? creditChips(names, value) : [];
  if (!chips.length) return null;
  return (
    <div className="aq-credit-chips" role="group" aria-label={`${label} — 예전에 쓴 이름`}>
      {chips.map(n => (
        <button key={n} type="button" className="aq-credit-chip" onClick={() => onAdd(addCredit(value, n))}>
          <Glyph name="plus" size={12} />{n}
        </button>
      ))}
    </div>
  );
}

export const TrackEditor = memo(function TrackEditor({
  track: t, index: i, expanded, canDelete, upload, collapsed = false, complete = false, onOpen, onCollapse,
  onToggleExpand, onDeleteTrack, setTrack, onTrackAudio, credits, forcedExplicit = null,
}: {
  track: Track; index: number; expanded: boolean; canDelete: boolean;
  /** 예전 발매에 쓴 크레딧 이름 — 칸 아래 제안 칩 */
  credits?: CreditHistory;
  /** 곡이 여럿일 때 지금 고치는 곡만 펼치고 나머지는 한 줄 요약으로 접는다 */
  collapsed?: boolean; complete?: boolean; onOpen?: (id: string) => void; onCollapse?: () => void;
  onToggleExpand: (id: string) => void; onDeleteTrack: (id: string) => void;
  setTrack: (id: string, key: keyof Track, value: string | boolean | number) => void;
  onTrackAudio: (id: string, e: React.ChangeEvent<HTMLInputElement>) => void;
  upload?: UploadState;
  /** 가사·곡 제목·앨범 제목에 욕설이 있어 19세 이용가로 정해진 곡 (lib/profanity — 서버가 저장할 때도 같게 정한다) */
  forcedExplicit?: 'track' | 'release' | null;
}) {
  if (collapsed) {
    const meta = [t.composers.trim() && `작곡 ${t.composers.trim()}`, t.audioName ? (t.duration || '음원 등록') : '음원 없음'].filter(Boolean).join(' · ');
    return (
      <article className="track-editor is-collapsed">
        <button type="button" className="aq-track-summary" aria-expanded={false} onClick={() => onOpen?.(t.id)}>
          <span className="aq-track-summary-no" aria-hidden="true">{i + 1}</span>
          <span className="min-0">
            <strong>{t.title.trim() || '곡 제목 없음'}</strong>
            <small>{meta}</small>
          </span>
          <Badge tone={complete ? 'green' : 'amber'}>{complete ? '입력 완료' : '입력 필요'}</Badge>
          <Glyph name="chevron-down" size={18} className="aq-track-summary-chev" />
        </button>
      </article>
    );
  }
  return (
    <article className="track-editor">
      <div className="minor-actions">
        <h3>트랙 {i + 1}</h3>
        <span className="ui-actions">
          {onCollapse && <button type="button" className="ui-btn is-text is-sm is-muted" onClick={onCollapse}>접기</button>}
          {canDelete && (
            // 접기와 같은 글자 버튼 — 지우는 일은 빨간 글자로만 (흰 알약에 파란 글자는 주 동작처럼 보였다)
            <button type="button" className="ui-btn is-text is-sm aq-danger-text" onClick={() => onDeleteTrack(t.id)}>삭제</button>
          )}
        </span>
      </div>
      <div className="ui-field track-title-field">
        <label htmlFor={`tr-${i}-title`}>곡 제목 <span className="required">*</span></label>
        <input id={`tr-${i}-title`} value={t.title} onChange={e => setTrack(t.id, 'title', e.target.value)} placeholder="곡명만 입력 (피처링·버전 표기 제외)" maxLength={200} />
        {/* 19세 표시 칸은 '상세 정보' 안에 있어 접혀 있을 때도 보이게 제목 아래에 한 줄 */}
        {forcedExplicit && !expanded && (
          <small className="aq-explicit-auto" role="status">
            {forcedExplicit === 'release' ? '앨범 제목' : '가사나 곡 제목'}에 욕설이 있어 이 곡은 19세 이용가로 표시돼요.
          </small>
        )}
      </div>
      <div className="ui-field">
        <label htmlFor={`tr-${i}-featuring`}>피처링 아티스트</label>
        <input id={`tr-${i}-featuring`} value={t.featuring} onChange={e => setTrack(t.id, 'featuring', e.target.value)} placeholder="없으면 비워 두세요 · 여러 명은 쉼표로 구분" maxLength={200} />
      </div>
      {/* 작사 칸보다 먼저 — 다 적은 뒤에야 '연주곡'을 보게 되면 작사를 비울 수 있다는 걸 몰랐다 */}
      <label className="ui-check is-plain aq-inst-line">
        <input type="checkbox" checked={t.instrumental} onChange={e => setTrack(t.id, 'instrumental', e.target.checked)} />
        <span><strong>가사 없는 연주곡이에요</strong><small>보컬·가사가 없으면 작사와 가사를 입력하지 않아도 돼요.</small></span>
      </label>
      <div className="form-grid">
        <div className="ui-field">
          <label htmlFor={`tr-${i}-composers`}>작곡 <span className="required">*</span></label>
          <input id={`tr-${i}-composers`} value={t.composers} onChange={e => setTrack(t.id, 'composers', e.target.value)} placeholder="실명 또는 활동명, 쉼표로 구분" maxLength={200} />
          <CreditChips names={credits?.composers} value={t.composers} label="작곡" onAdd={v => setTrack(t.id, 'composers', v)} />
        </div>
        <div className="ui-field">
          <label htmlFor={`tr-${i}-lyricists`}>작사 {!t.instrumental && <span className="required">*</span>}</label>
          <input
            id={`tr-${i}-lyricists`} value={t.instrumental ? '' : t.lyricists} disabled={t.instrumental}
            onChange={e => setTrack(t.id, 'lyricists', e.target.value)}
            placeholder={t.instrumental ? '연주곡은 입력하지 않아요' : '실명 또는 활동명, 쉼표로 구분'} maxLength={200}
          />
          {!t.instrumental && <CreditChips names={credits?.lyricists} value={t.lyricists} label="작사" onAdd={v => setTrack(t.id, 'lyricists', v)} />}
        </div>
      </div>
      <div className="ui-field">
        <label htmlFor={`trackFile-${i}`}>음원 파일 <span className="required">*</span></label>
        <FilePicker
          id={`trackFile-${i}`} fileName={t.audioName}
          accept=".wav,.flac,.m4a,.aif,.aiff,.aifc,.wv,.tta,audio/wav,audio/flac,audio/mp4,audio/aiff,audio/wavpack,audio/tta"
          onChange={e => onTrackAudio(t.id, e)} placeholder="음원 파일을 선택해 주세요"
        />
        <UploadStatus upload={upload} idle={t.audioName
          ? `${t.audioName}${t.audioSpec ? ` · ${t.audioSpec}` : t.audioSize ? ` · ${fileSize(t.audioSize)}` : ''}${t.assetId || upload?.state === 'received' ? ' · 올렸어요' : MOCK ? '' : ' · 업로드되지 않았어요. 파일을 다시 선택해 주세요.'}`
          : '무손실 원본 WAV·FLAC·ALAC(.m4a)·AIFF·WavPack(.wv)·TTA · 44.1kHz 이상, 16bit 이상, 스테레오'} />
        {!MOCK && t.assetId && upload?.state !== 'uploading' && <TrackChecks assetId={t.assetId} />}
      </div>
      <div className="track-duration-label" aria-live="polite">
        {t.duration ? `곡 길이 · ${t.duration}` : '음원을 선택하면 곡 길이를 자동으로 확인해요.'}
      </div>
      <button
        type="button" className="track-detail-toggle"
        onClick={() => onToggleExpand(t.id)}
        aria-expanded={expanded}
      >
        <span>상세 정보 {t.isrc ? `· ${t.isrc}` : ''}</span>
        <span className={`toggle-arrow${expanded ? ' open' : ''}`}><Glyph name="chevron-right" size={16} /></span>
      </button>
      {expanded && (
        <div className="track-detail-body">
          <div className="form-grid">
            <div className="ui-field">
              <label htmlFor={`tr-${i}-version`}>버전 / 부제</label>
              <input id={`tr-${i}-version`} value={t.version} onChange={e => setTrack(t.id, 'version', e.target.value)} placeholder="예: Acoustic Version" maxLength={200} />
            </div>
            <div className="ui-field">
              <label htmlFor={`tr-${i}-isrc`}>ISRC <span className="aq-optional">선택</span></label>
              <input id={`tr-${i}-isrc`} value={t.isrc} onChange={e => setTrack(t.id, 'isrc', e.target.value)} placeholder="예: KR-ABC-26-00001" maxLength={200} />
            </div>
            <div className="ui-field">
              <label htmlFor={`tr-${i}-arrangers`}>편곡</label>
              <input id={`tr-${i}-arrangers`} value={t.arrangers} onChange={e => setTrack(t.id, 'arrangers', e.target.value)} placeholder="참여자 이름" maxLength={200} />
              <CreditChips names={credits?.arrangers} value={t.arrangers} label="편곡" onAdd={v => setTrack(t.id, 'arrangers', v)} />
            </div>
            <div className="ui-field">
              <label htmlFor={`tr-${i}-performers`}>실연자 (보컬·연주)</label>
              <input id={`tr-${i}-performers`} value={t.performers} onChange={e => setTrack(t.id, 'performers', e.target.value)} placeholder="참여자 이름" maxLength={200} />
            </div>
            <div className="ui-field">
              <label htmlFor={`tr-${i}-producer`}>프로듀서</label>
              <input id={`tr-${i}-producer`} value={t.producer} onChange={e => setTrack(t.id, 'producer', e.target.value)} placeholder="프로듀서 이름" maxLength={200} />
            </div>
          </div>
          {!t.instrumental && <div className="ui-field">
            <label htmlFor={`tr-${i}-lyrics`}>가사 전문</label>
            <textarea
              id={`tr-${i}-lyrics`} value={t.lyrics}
              onChange={e => setTrack(t.id, 'lyrics', e.target.value)}
              rows={4} maxLength={10000} placeholder="가사 전체를 입력해 주세요 (국내 플랫폼 가사 노출·심의에 쓰여요)"
            />
          </div>}
          <label className="ui-check is-plain">
            <input
              type="checkbox" checked={t.explicit || !!forcedExplicit} disabled={!!forcedExplicit}
              onChange={e => setTrack(t.id, 'explicit', e.target.checked)}
            />
            <span>
              청소년 이용불가 / Explicit 가사가 포함돼요.
              {forcedExplicit && (
                <small className="aq-explicit-auto" role="status">
                  {forcedExplicit === 'release' ? '앨범 제목' : '가사나 곡 제목'}에 욕설이 있어 19세 이용가로 표시했어요. 그 부분을 고치면 표시가 풀려요.
                </small>
              )}
            </span>
          </label>
        </div>
      )}
    </article>
  );
});
