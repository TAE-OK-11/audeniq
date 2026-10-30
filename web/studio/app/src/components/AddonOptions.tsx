// 발매 신청서 '부가서비스' — 뮤직비디오 배급과 AI 가사 싱크를 고르면 바로 아래에 열리는 입력 (Upload.tsx OptionsSection).
// 접수하면 서버가 이 옵션으로 주문을 만들고(crates/core/src/addons.rs sync_from_submission), 신청 완료 화면에서 한 번에 결제한다.
// 곡은 신청서의 순서(1부터)로 가리킨다 — 신청서의 곡 ID는 화면 안에서만 쓰는 값이라서.
import { useEffect, useRef, useState } from 'react';
import { errorMessage } from '../api/errors';
import { money } from '../lib/format';
import { PLATFORM_NAMES } from '../lib/platformNames';
import { MAX_VIDEO_BYTES, MV_PLATFORMS, PRICES, RATING_LABEL, TIER, uploadVideo, type MvTier, type VideoChecks } from '../lib/addons';
import { CheckIcon } from './Check';
import { ChipPicker } from './ChipPicker';
import { FileCard } from './FileCard';
import { Glyph } from './Glyph';
import { Segmented } from './Segmented';
import { useToast } from './Toast';
import { IconTile } from '../ui';
import '../styles/split/Upload.css';

export interface AddonFields {
  mv: boolean; mvTier: MvTier; mvTrack: number;
  mvVideoFile: string; mvVideoName: string; mvVideoUrl: string;
  /** 서버가 올린 파일에서 읽은 값 — 보여 주기만 하고, 접수 때 서버가 다시 읽는다 */
  mvDuration: string; mvWidth: number; mvHeight: number;
  /** 서버가 파일을 검사한 결과(코덱·화질·소리·배급 규격) — 없으면 null */
  mvChecks: VideoChecks | null;
  mvTitle: string; mvDirector: string; mvProduction: string; mvCast: string; mvSynopsis: string;
  mvRating: string; mvRatingNo: string; mvPlatforms: string[]; mvDate: string;
  lyricSync: boolean; lyricTracks: number[];
}

export const EMPTY_ADDONS: AddonFields = {
  mv: false, mvTier: 'REVIEW', mvTrack: 1, mvVideoFile: '', mvVideoName: '', mvVideoUrl: '',
  mvDuration: '', mvWidth: 0, mvHeight: 0, mvChecks: null,
  mvTitle: '', mvDirector: '', mvProduction: '', mvCast: '', mvSynopsis: '',
  mvRating: '', mvRatingNo: '', mvPlatforms: [...MV_PLATFORMS], mvDate: '',
  lyricSync: false, lyricTracks: [],
};

interface TrackLite { title: string; lyrics: string; instrumental?: boolean }

/** 고른 부가서비스와 금액 (신청서 최종 확인·안내) */
export function addonLines(o: Partial<AddonFields> & { express?: boolean }): { label: string; amount: number }[] {
  const out: { label: string; amount: number }[] = [];
  if (o.express) out.push({ label: '신속 발매', amount: PRICES.express_release });
  if (o.mv) {
    const t = (o.mvTier ?? 'REVIEW') as MvTier;
    out.push({ label: `뮤직비디오 배급 · ${TIER[t].label}`, amount: PRICES.mv.find(x => x.tier === t)?.price ?? 0 });
  }
  if (o.lyricSync && o.lyricTracks?.length) out.push({ label: `AI 가사 싱크 · ${o.lyricTracks.length}곡`, amount: PRICES.lyric_sync * o.lyricTracks.length });
  return out;
}

/** 다음 단계로 가기 전 확인 — 빠진 칸의 안내와 그 칸 id, 다 됐으면 null */
export function addonProblem(o: AddonFields, tracks: TrackLite[]): [string, string] | null {
  if (o.mv) {
    if (!tracks[o.mvTrack - 1]) return ['뮤직비디오 곡을 골라 주세요.', 'aqMvTrack'];
    if (!o.mvVideoFile && !/^https:\/\/\S+$/i.test(o.mvVideoUrl.trim())) return ['뮤직비디오 영상 파일을 올리거나 https 링크를 적어 주세요.', 'aqMvVideo'];
    if (!o.mvTitle.trim()) return ['뮤직비디오 제목을 적어 주세요.', 'aqMvTitle'];
    if (!o.mvPlatforms.length) return ['뮤직비디오를 보낼 플랫폼을 하나 이상 골라 주세요.', 'aqMvPlatforms'];
    if (o.mvTier === 'BASIC' && (!o.mvRating || !o.mvRatingNo.trim())) return ['받은 등급과 등급분류번호를 적어 주세요.', 'aqMvRating'];
  }
  if (o.lyricSync) {
    if (!o.lyricTracks.length) return ['싱크 가사를 만들 곡을 골라 주세요.', 'aqLyricTracks'];
    if (o.lyricTracks.some(n => !tracks[n - 1]?.lyrics.trim())) return ['가사를 넣은 곡만 싱크 가사를 만들 수 있어요. 2단계에서 가사를 넣어 주세요.', 'aqLyricTracks'];
  }
  return null;
}

const VIDEO_MODES = [{ value: 'file', label: '파일 올리기' }, { value: 'link', label: '링크로 보내기' }] as const;
const platformName = (p: string) => PLATFORM_NAMES[p] ?? p;

/** 올린 파일 아래 줄 — 서버가 읽은 길이·화면 크기(검사했으면 코덱·프레임레이트·소리까지) */
function videoFacts(o: AddonFields): string {
  const facts = [o.mvDuration && `길이 ${o.mvDuration}`, o.mvChecks?.summary || (o.mvWidth > 0 && `${o.mvWidth}×${o.mvHeight}`)].filter(Boolean);
  return `${facts.length ? `${facts.join(' · ')} · ` : ''}다시 누르면 바꿀 수 있어요`;
}

/** 서버 검사에서 나온 문제 — 배급할 수 없는 것(error)과 반려될 수 있는 것(warning) */
function VideoIssues({ checks }: { checks: VideoChecks }) {
  const shown = checks.issues.filter(i => i.severity !== 'info');
  return (
    <div className="aq-option-note" role={checks.mv_ready ? undefined : 'alert'}>
      <strong>{checks.mv_ready ? '플랫폼에 따라 반려될 수 있어요. 원본이 있으면 바꿔 주세요.' : '이 파일은 그대로 배급할 수 없어요. 다시 내보내 올려 주세요.'}</strong>
      <ul>{shown.map(i => <li key={i.code}>{i.detail}</li>)}</ul>
      {checks.sound === 'pending' && <p>소리(음량·클리핑·무음)는 서버가 이어서 확인해요.</p>}
    </div>
  );
}

export function MvDetail({ o, set, tracks }: { o: AddonFields; set: (patch: Partial<AddonFields>) => void; tracks: TrackLite[] }) {
  const toast = useToast();
  const [pct, setPct] = useState<number | null>(null);
  const [mode, setMode] = useState<'file' | 'link'>(o.mvVideoUrl && !o.mvVideoFile ? 'link' : 'file');
  const abort = useRef<AbortController | null>(null);
  // 제목이 비어 있으면 고른 곡 이름으로 채워 둔다
  useEffect(() => {
    const t = tracks[o.mvTrack - 1]?.title.trim();
    if (!o.mvTitle.trim() && t) set({ mvTitle: `${t} MV` });
  }, [o.mvTrack]); // eslint-disable-line react-hooks/exhaustive-deps
  const pick = async (f: File) => {
    if (!/\.(mp4|mov)$/i.test(f.name) && !['video/mp4', 'video/quicktime'].includes(f.type)) { toast('MP4나 MOV 영상을 골라 주세요.', 'error'); return; }
    if (f.size > MAX_VIDEO_BYTES) { toast('8GB 이하 영상을 골라 주세요. 더 크면 링크로 보내 주세요.', 'error'); return; }
    abort.current = new AbortController();
    setPct(0);
    try {
      const v = await uploadVideo(f, r => setPct(r), abort.current.signal);
      set({ mvVideoFile: v.id, mvVideoName: f.name, mvVideoUrl: '', mvDuration: v.duration, mvWidth: v.width, mvHeight: v.height, mvChecks: v.checks ?? null });
      toast('영상을 올렸어요.', 'success');
    } catch (e) { toast(errorMessage(e), 'error'); } finally { setPct(null); }
  };
  const up = pct === null ? null : Math.round(pct * 100);
  return (
    <div className="aq-option-detail">
      <h3>뮤직비디오 배급</h3>
      <p className="aq-option-intro">멜론·지니·벅스·FLO·바이브·유튜브에 뮤직비디오를 보내요. 영상물등급위원회 등급이 아직 없으면 심의 신청도 대신해요.</p>
      <div className="ui-field">
        <span className="ui-field-label">상품 <span className="required">*</span></span>
        <div className="aq-pick-list" role="radiogroup" aria-label="뮤직비디오 상품">
          {PRICES.mv.map(t => {
            const on = o.mvTier === t.tier;
            return (
              <button key={t.tier} type="button" role="radio" aria-checked={on} className={`aq-pick${on ? ' is-on' : ''}`} onClick={() => set({ mvTier: t.tier })}>
                <span>{TIER[t.tier].label} · {money(t.price)}<small>{TIER[t.tier].desc}</small></span>
                <span className="aq-pick-mark is-radio" aria-hidden="true">{on && <CheckIcon size={12} />}</span>
              </button>
            );
          })}
        </div>
      </div>
      <div className="ui-field">
        <label htmlFor="aqMvTrack">뮤직비디오 곡 <span className="required">*</span></label>
        <select id="aqMvTrack" value={o.mvTrack} onChange={e => set({ mvTrack: Number(e.target.value) })}>
          {tracks.map((t, i) => <option key={i} value={i + 1}>{i + 1}. {t.title || '제목 없는 곡'}</option>)}
        </select>
      </div>
      <div className="ui-field" id="aqMvVideo">
        <strong>영상 <span className="required">*</span></strong>
        <Segmented className="aq-method" label="영상 보내는 방법" value={mode} onChange={setMode} options={VIDEO_MODES} />
        {mode === 'file' ? (
          <>
            <label className="sr-only" htmlFor="aqMvFile">영상 파일</label>
            <FileCard
              id="aqMvFile" busy={up !== null} fileName={o.mvVideoName || (o.mvVideoFile ? '올린 영상' : '')}
              accept="video/mp4,video/quicktime,.mp4,.mov" title="영상을 끌어다 놓거나 눌러서 선택"
              hint="MP4 · MOV · 원본 화질(1080p 이상) · 8GB까지"
              status={o.mvVideoFile ? videoFacts(o) : undefined}
              onFile={f => void pick(f)}
            />
            {up !== null ? (
              <div className="aq-upload-progress" role="progressbar" aria-valuenow={up} aria-valuemin={0} aria-valuemax={100} aria-label="영상 업로드 진행률">
                <span><i style={{ ['--p' as string]: `${Math.max(4, up)}%` }} /></span>
                <small>{up < 100 ? `업로드 중 ${up}%` : '서버에서 확인하는 중…'} · <button type="button" className="ui-btn is-text is-xs" onClick={() => abort.current?.abort()}>멈추기</button></small>
              </div>
            ) : o.mvVideoFile && o.mvChecks?.issues.some(i => i.severity !== 'info')
              ? <VideoIssues checks={o.mvChecks} />
              : o.mvVideoFile && !o.mvChecks && o.mvHeight > 0 && Math.min(o.mvWidth, o.mvHeight) < 1080
              ? <p className="aq-option-note">1080p보다 작은 영상이에요. 플랫폼에 따라 반려될 수 있으니 원본이 있으면 바꿔 주세요.</p>
              : <p className="ui-field-help">길이와 화질은 올리면 서버가 파일에서 직접 읽어요. 끊기면 같은 파일을 다시 고르면 이어서 올려요.</p>}
          </>
        ) : (
          <>
            <label className="sr-only" htmlFor="aqMvUrl">영상 링크</label>
            <input id="aqMvUrl" type="url" inputMode="url" maxLength={500} placeholder="https:// (구글 드라이브·드롭박스 등 내려받을 수 있는 링크)"
              value={o.mvVideoUrl} onChange={e => set({ mvVideoUrl: e.target.value, mvVideoFile: '', mvVideoName: '', mvDuration: '', mvWidth: 0, mvHeight: 0, mvChecks: null })} />
            <p className="ui-field-help">링크는 누구나 내려받을 수 있게 열어 두세요. 길이와 화질은 담당자가 내려받아 확인해요.</p>
          </>
        )}
      </div>
      <div className="ui-field">
        <label htmlFor="aqMvTitle">뮤직비디오 제목 <span className="required">*</span></label>
        <input id="aqMvTitle" maxLength={200} placeholder="예: 여름의 끝 MV" value={o.mvTitle} onChange={e => set({ mvTitle: e.target.value })} />
      </div>
      <div className="ui-field">
        <label htmlFor="aqMvDirector">감독<span className="aq-optional">선택</span></label>
        <input id="aqMvDirector" maxLength={200} value={o.mvDirector} onChange={e => set({ mvDirector: e.target.value })} />
      </div>
      <div className="ui-field">
        <label htmlFor="aqMvProduction">제작사<span className="aq-optional">선택</span></label>
        <input id="aqMvProduction" maxLength={200} value={o.mvProduction} onChange={e => set({ mvProduction: e.target.value })} />
      </div>
      {o.mvTier === 'BASIC' ? (
        <>
          <div className="ui-field" id="aqMvRating">
            <span className="ui-field-label">받은 등급 <span className="required">*</span></span>
            <ChipPicker id="aqMvRatingPick" single options={Object.values(RATING_LABEL)} value={o.mvRating ? [RATING_LABEL[o.mvRating]] : []}
              onChange={v => set({ mvRating: (Object.entries(RATING_LABEL).find(([, l]) => l === v[0])?.[0]) ?? '' })} />
          </div>
          <div className="ui-field">
            <label htmlFor="aqMvRatingNo">등급분류번호 <span className="required">*</span></label>
            <input id="aqMvRatingNo" maxLength={60} placeholder="영상물등급위원회 결정서의 번호" value={o.mvRatingNo} onChange={e => set({ mvRatingNo: e.target.value })} />
          </div>
        </>
      ) : (
        <>
          <div className="ui-field">
            <label htmlFor="aqMvCast">출연자<span className="aq-optional">선택</span></label>
            <input id="aqMvCast" maxLength={500} placeholder="심의 신청서에 들어가요" value={o.mvCast} onChange={e => set({ mvCast: e.target.value })} />
          </div>
          <div className="ui-field">
            <label htmlFor="aqMvSynopsis">영상 내용<span className="aq-optional">선택</span></label>
            <textarea id="aqMvSynopsis" rows={3} maxLength={2000} placeholder="어떤 장면이 나오는지 두세 문장으로 (심의에 도움이 돼요)" value={o.mvSynopsis} onChange={e => set({ mvSynopsis: e.target.value })} />
          </div>
        </>
      )}
      <div className="ui-field">
        <span className="ui-field-label">보낼 플랫폼 <span className="required">*</span></span>
        <ChipPicker id="aqMvPlatforms" options={MV_PLATFORMS.map(platformName)} value={o.mvPlatforms.map(platformName)}
          onChange={v => set({ mvPlatforms: MV_PLATFORMS.filter(p => v.includes(platformName(p))) })} />
      </div>
      <div className="ui-field">
        <label htmlFor="aqMvDate">공개 희망일<span className="aq-optional">선택</span></label>
        <input id="aqMvDate" type="date" value={o.mvDate} onChange={e => set({ mvDate: e.target.value })} />
        <p className="ui-field-help">{o.mvTier === 'BASIC' ? '보통 영업일 기준 5일 뒤부터 공개할 수 있어요.' : o.mvTier === 'REVIEW' ? '심의에 보통 1~2주 걸려요. 등급이 나오면 바로 보내요.' : '심의와 배급을 가장 먼저 처리해요.'}</p>
      </div>
    </div>
  );
}

export function LyricDetail({ o, set, tracks }: { o: AddonFields; set: (patch: Partial<AddonFields>) => void; tracks: TrackLite[] }) {
  // 처음 고르면 가사가 있는 곡을 모두 골라 둔다
  useEffect(() => {
    if (!o.lyricTracks.length) {
      const all = tracks.map((t, i) => (t.lyrics.trim() && !t.instrumental ? i + 1 : 0)).filter(Boolean);
      if (all.length) set({ lyricTracks: all });
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  return (
    <div className="aq-option-detail">
      <h3>AI 가사 싱크 · 곡당 {money(PRICES.lyric_sync)}</h3>
      <p className="aq-option-intro">2단계에서 넣은 가사에 AI가 노래 시간을 맞추고, 담당자가 확인해 Musixmatch(Spotify·Apple Music·Instagram 가사)와 국내 플랫폼에 등록해요.</p>
      <div className="ui-field">
        <span className="ui-field-label">싱크 가사를 만들 곡 <span className="required">*</span></span>
        <div className="aq-pick-list" id="aqLyricTracks" role="group" aria-label="싱크 가사를 만들 곡">
          {tracks.map((t, i) => {
            const n = i + 1;
            const has = !!t.lyrics.trim() && !t.instrumental;
            const on = o.lyricTracks.includes(n);
            return (
              <button key={n} type="button" className={`aq-pick${on ? ' is-on' : ''}`} aria-pressed={on} disabled={!has}
                onClick={() => set({ lyricTracks: on ? o.lyricTracks.filter(x => x !== n) : [...o.lyricTracks, n].sort((a, b) => a - b) })}>
                <span>{n}. {t.title || '제목 없는 곡'}<small>{has ? '가사 있음' : t.instrumental ? '연주곡' : '가사가 없어요 — 2단계에서 넣으면 고를 수 있어요'}</small></span>
                <span className="aq-pick-mark" aria-hidden="true">{on && <CheckIcon size={12} />}</span>
              </button>
            );
          })}
        </div>
        {o.lyricTracks.length > 0 && <p className="ui-field-help">{o.lyricTracks.length}곡 · {money(PRICES.lyric_sync * o.lyricTracks.length)}</p>}
      </div>
    </div>
  );
}

/** 최종 확인 — 고른 부가서비스와 합계. 접수하면 바로 다음 화면에서 한 번에 결제한다 */
export function AddonSummary({ options, onEdit }: { options: Partial<AddonFields> & { express?: boolean }; onEdit: () => void }) {
  const lines = addonLines(options);
  if (!lines.length) return null;
  const total = lines.reduce((n, l) => n + l.amount, 0);
  return (
    <section className="aq-addon-summary" aria-labelledby="aqAddonSumHead">
      <header className="aq-manage-head">
        <IconTile><Glyph name="layers" size={18} /></IconTile>
        <h2 id="aqAddonSumHead">고른 부가서비스</h2>
        <button type="button" className="ui-btn is-white is-sm" onClick={onEdit}>수정</button>
      </header>
      <ul className="aq-addon-lines">
        {lines.map(l => <li key={l.label}><span className="min-0"><strong>{l.label}</strong></span><b>{money(l.amount)}</b></li>)}
        <li className="is-total"><span>합계 (부가세 포함)</span><b>{money(total)}</b></li>
      </ul>
      <p className="aq-addon-note">접수하면 바로 다음 화면에서 정산금이나 카드로 한 번에 결제해요. 나중에 발매 상세에서 결제해도 돼요.</p>
    </section>
  );
}
