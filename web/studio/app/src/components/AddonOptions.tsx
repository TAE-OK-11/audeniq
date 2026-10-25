// 발매 신청서 '부가서비스' — 뮤직비디오 배급과 AI 가사 싱크를 고르면 바로 아래에 열리는 입력 (Upload.tsx OptionsSection).
// 접수하면 서버가 이 옵션으로 주문을 만들고(crates/core/src/addons.rs sync_from_submission), 신청 완료 화면에서 한 번에 결제한다.
// 곡은 신청서의 순서(1부터)로 가리킨다 — 신청서의 곡 ID는 화면 안에서만 쓰는 값이라서.
import { useEffect, useRef, useState } from 'react';
import { errorMessage } from '../api/errors';
import { money } from '../lib/format';
import { PLATFORM_NAMES } from '../lib/platformNames';
import { MAX_VIDEO_BYTES, MV_PLATFORMS, PRICES, RATING_LABEL, TIER, uploadVideo, type MvTier } from '../lib/addons';
import { CheckIcon } from './Check';
import { Glyph } from './Glyph';
import { useToast } from './Toast';

export interface AddonFields {
  mv: boolean; mvTier: MvTier; mvTrack: number;
  mvVideoFile: string; mvVideoName: string; mvVideoUrl: string;
  mvTitle: string; mvDuration: string; mvDirector: string; mvProduction: string; mvCast: string; mvSynopsis: string;
  mvRating: string; mvRatingNo: string; mvPlatforms: string[]; mvDate: string;
  lyricSync: boolean; lyricTracks: number[];
}

export const EMPTY_ADDONS: AddonFields = {
  mv: false, mvTier: 'REVIEW', mvTrack: 1, mvVideoFile: '', mvVideoName: '', mvVideoUrl: '',
  mvTitle: '', mvDuration: '', mvDirector: '', mvProduction: '', mvCast: '', mvSynopsis: '',
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
    if (!/^\d{1,2}:[0-5]\d$/.test(o.mvDuration.trim())) return ['영상 길이를 3:45처럼 적어 주세요.', 'aqMvDuration'];
    if (!o.mvPlatforms.length) return ['뮤직비디오를 보낼 플랫폼을 하나 이상 골라 주세요.', 'aqMvPlatforms'];
    if (o.mvTier === 'BASIC' && (!o.mvRating || !o.mvRatingNo.trim())) return ['받은 등급과 등급분류번호를 적어 주세요.', 'aqMvRating'];
  }
  if (o.lyricSync) {
    if (!o.lyricTracks.length) return ['싱크 가사를 만들 곡을 골라 주세요.', 'aqLyricTracks'];
    if (o.lyricTracks.some(n => !tracks[n - 1]?.lyrics.trim())) return ['가사를 넣은 곡만 싱크 가사를 만들 수 있어요. 2단계에서 가사를 넣어 주세요.', 'aqLyricTracks'];
  }
  return null;
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
  const pick = async (f: File | undefined) => {
    if (!f) return;
    if (!/\.(mp4|mov)$/i.test(f.name) && !['video/mp4', 'video/quicktime'].includes(f.type)) { toast('MP4나 MOV 영상을 골라 주세요.', 'error'); return; }
    if (f.size > MAX_VIDEO_BYTES) { toast('8GB 이하 영상을 골라 주세요. 더 크면 링크로 보내 주세요.', 'error'); return; }
    abort.current = new AbortController();
    setPct(0);
    try {
      const id = await uploadVideo(f, r => setPct(r), abort.current.signal);
      set({ mvVideoFile: id, mvVideoName: f.name, mvVideoUrl: '' });
      toast('영상을 올렸어요.', 'success');
    } catch (e) { toast(errorMessage(e), 'error'); } finally { setPct(null); }
  };
  return (
    <div className="aq-option-detail aq-addon-detail">
      <h3>뮤직비디오 배급</h3>
      <p className="aq-option-intro">멜론·지니·벅스·FLO·바이브·유튜브에 뮤직비디오를 보내요. 영상물등급위원회 등급이 아직 없으면 심의 신청도 대신해요.</p>
      <div className="ui-choices" role="radiogroup" aria-label="뮤직비디오 상품">
        {PRICES.mv.map(t => (
          <button key={t.tier} type="button" role="radio" aria-checked={o.mvTier === t.tier} className="ui-choice aq-addon-pay" onClick={() => set({ mvTier: t.tier })}>
            <span>
              <b>{TIER[t.tier].label} · {money(t.price)}</b>
              <small>{TIER[t.tier].desc}</small>
            </span>
            <span className="ui-choice-mark" aria-hidden="true">{o.mvTier === t.tier && <CheckIcon size={11} />}</span>
          </button>
        ))}
      </div>
      <div className="ui-field">
        <label htmlFor="aqMvTrack">뮤직비디오 곡</label>
        <select id="aqMvTrack" value={o.mvTrack} onChange={e => set({ mvTrack: Number(e.target.value) })}>
          {tracks.map((t, i) => <option key={i} value={i + 1}>{i + 1}. {t.title || '제목 없는 곡'}</option>)}
        </select>
      </div>
      <div className="ui-field" id="aqMvVideo">
        <span className="ui-field-label">영상</span>
        <div className="aq-yesno" role="radiogroup" aria-label="영상 보내는 방법">
          <button type="button" role="radio" aria-checked={mode === 'file'} className={`ui-chip${mode === 'file' ? ' is-on' : ''}`} onClick={() => setMode('file')}>파일 올리기</button>
          <button type="button" role="radio" aria-checked={mode === 'link'} className={`ui-chip${mode === 'link' ? ' is-on' : ''}`} onClick={() => setMode('link')}>링크로 보내기</button>
        </div>
        {mode === 'file' ? (
          pct !== null ? (
            <div className="aq-addon-upload" role="status">
              <span>영상 올리는 중 {Math.round(pct * 100)}%</span>
              <span className="aq-addon-bar" aria-hidden="true"><i style={{ width: `${Math.round(pct * 100)}%` }} /></span>
              <button type="button" className="ui-btn is-text is-sm" onClick={() => abort.current?.abort()}>멈추기</button>
            </div>
          ) : o.mvVideoFile ? (
            <div className="aq-addon-file-row">
              <Glyph name="release" size={16} />
              <span className="min-0">{o.mvVideoName || '올린 영상'}</span>
              <label className="ui-btn is-soft is-sm">
                <input type="file" accept="video/mp4,video/quicktime,.mp4,.mov" className="sr-only" onChange={e => void pick(e.target.files?.[0])} />바꾸기
              </label>
            </div>
          ) : (
            <label className="aq-addon-drop">
              <input type="file" accept="video/mp4,video/quicktime,.mp4,.mov" className="sr-only" onChange={e => void pick(e.target.files?.[0])} />
              <Glyph name="download" size={18} />
              <span><b>영상 파일 고르기</b><small>MP4·MOV, 원본 화질(1080p 이상), 8GB까지. 끊기면 같은 파일을 다시 고르면 이어서 올려요.</small></span>
            </label>
          )
        ) : (
          <>
            <input id="aqMvUrl" type="url" inputMode="url" maxLength={500} placeholder="https:// (구글 드라이브·드롭박스 등 내려받을 수 있는 링크)"
              value={o.mvVideoUrl} onChange={e => set({ mvVideoUrl: e.target.value, mvVideoFile: '', mvVideoName: '' })} />
            <small className="ui-field-help">링크는 누구나 내려받을 수 있게 열어 두세요.</small>
          </>
        )}
      </div>
      <div className="form-grid">
        <div className="ui-field">
          <label htmlFor="aqMvTitle">뮤직비디오 제목</label>
          <input id="aqMvTitle" maxLength={200} placeholder="예: 여름의 끝 MV" value={o.mvTitle} onChange={e => set({ mvTitle: e.target.value })} />
        </div>
        <div className="ui-field">
          <label htmlFor="aqMvDuration">영상 길이</label>
          <input id="aqMvDuration" inputMode="numeric" maxLength={5} placeholder="3:45" value={o.mvDuration} onChange={e => set({ mvDuration: e.target.value })} />
        </div>
        <div className="ui-field">
          <label htmlFor="aqMvDirector">감독<span className="aq-optional">선택</span></label>
          <input id="aqMvDirector" maxLength={200} value={o.mvDirector} onChange={e => set({ mvDirector: e.target.value })} />
        </div>
        <div className="ui-field">
          <label htmlFor="aqMvProduction">제작사<span className="aq-optional">선택</span></label>
          <input id="aqMvProduction" maxLength={200} value={o.mvProduction} onChange={e => set({ mvProduction: e.target.value })} />
        </div>
      </div>
      {o.mvTier === 'BASIC' ? (
        <div className="form-grid" id="aqMvRating">
          <div className="ui-field">
            <label htmlFor="aqMvRatingSel">받은 등급</label>
            <select id="aqMvRatingSel" value={o.mvRating} onChange={e => set({ mvRating: e.target.value })}>
              <option value="">고르기</option>
              {Object.entries(RATING_LABEL).map(([k, v]) => <option key={k} value={k}>{v}</option>)}
            </select>
          </div>
          <div className="ui-field">
            <label htmlFor="aqMvRatingNo">등급분류번호</label>
            <input id="aqMvRatingNo" maxLength={60} placeholder="영상물등급위원회 결정서의 번호" value={o.mvRatingNo} onChange={e => set({ mvRatingNo: e.target.value })} />
          </div>
        </div>
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
      <div className="ui-field" id="aqMvPlatforms">
        <span className="ui-field-label">보낼 플랫폼</span>
        <div className="aq-addon-platforms">
          {MV_PLATFORMS.map(p => (
            <label key={p} className="ui-check">
              <input type="checkbox" checked={o.mvPlatforms.includes(p)} onChange={e => set({ mvPlatforms: e.target.checked ? [...o.mvPlatforms, p] : o.mvPlatforms.filter(x => x !== p) })} />
              <span>{PLATFORM_NAMES[p] ?? p}</span>
            </label>
          ))}
        </div>
      </div>
      <div className="ui-field">
        <label htmlFor="aqMvDate">공개 희망일<span className="aq-optional">선택</span></label>
        <input id="aqMvDate" type="date" value={o.mvDate} onChange={e => set({ mvDate: e.target.value })} />
        <small className="ui-field-help">{o.mvTier === 'BASIC' ? '보통 영업일 기준 5일 뒤부터 공개할 수 있어요.' : o.mvTier === 'REVIEW' ? '심의에 보통 1~2주 걸려요. 등급이 나오면 바로 보내요.' : '심의와 배급을 가장 먼저 처리해요.'}</small>
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
    <div className="aq-option-detail aq-addon-detail">
      <h3>AI 가사 싱크</h3>
      <p className="aq-option-intro">2단계에서 넣은 가사에 AI가 노래 시간을 맞추고, 담당자가 확인해 Musixmatch(Spotify·Apple Music·Instagram 가사)와 국내 플랫폼에 등록해요. 곡당 {money(PRICES.lyric_sync)}.</p>
      <div className="aq-addon-tracks" id="aqLyricTracks" role="group" aria-label="싱크 가사를 만들 곡">
        {tracks.map((t, i) => {
          const n = i + 1;
          const has = !!t.lyrics.trim() && !t.instrumental;
          return (
            <label key={n} className={`ui-check${has ? '' : ' is-disabled'}`}>
              <input type="checkbox" disabled={!has} checked={o.lyricTracks.includes(n)}
                onChange={e => set({ lyricTracks: e.target.checked ? [...o.lyricTracks, n].sort((a, b) => a - b) : o.lyricTracks.filter(x => x !== n) })} />
              <span className="min-0">
                {n}. {t.title || '제목 없는 곡'}
                <small>{has ? '가사 있음' : t.instrumental ? '연주곡' : '가사가 없어요 — 2단계에서 넣으면 고를 수 있어요'}</small>
              </span>
            </label>
          );
        })}
      </div>
      {o.lyricTracks.length > 0 && <p className="aq-addon-sum">{o.lyricTracks.length}곡 · {money(PRICES.lyric_sync * o.lyricTracks.length)}</p>}
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
      <div className="aq-addon-checkout-head">
        <h2 id="aqAddonSumHead">고른 부가서비스</h2>
        <button type="button" className="ui-btn is-white is-xs" onClick={onEdit}>수정</button>
      </div>
      <ul className="aq-addon-lines">
        {lines.map(l => <li key={l.label}><span className="min-0"><strong>{l.label}</strong></span><b>{money(l.amount)}</b></li>)}
        <li className="is-total"><span>합계 (부가세 포함)</span><b>{money(total)}</b></li>
      </ul>
      <p className="aq-addon-note">접수하면 바로 다음 화면에서 정산금이나 카드로 한 번에 결제해요. 나중에 발매 상세에서 결제해도 돼요.</p>
    </section>
  );
}
