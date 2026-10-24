// 가사 카드 버튼 — 곡을 고르고 가사에서 몇 줄(최대 4줄)을 골라 스토리·피드 이미지로.
import { useMemo, useState } from 'react';
import { Modal } from './Modal';
import { Glyph } from './Glyph';
import { ImageMaker } from './PromoImageButton';
import { LYRIC_MAX_LINES, lyricLines, renderLyricCard } from '../lib/lyricCard';

export interface LyricTrack { title: string; lyrics: string }

export function LyricCardButton({ tracks, artist, url, cover, filename, className = 'ui-btn is-sm' }: {
  tracks: LyricTrack[]; artist: string; url: string; cover?: string; filename: string; className?: string;
}) {
  const [open, setOpen] = useState(false);
  if (!tracks.length) return null;
  return (
    <>
      <button type="button" className={className} onClick={() => setOpen(true)}><Glyph name="quote" size={14} />가사 카드</button>
      {open && (
        <Modal title="가사 카드" onClose={() => setOpen(false)} modalClass="aq-set-modal aq-promo-modal">
          <LyricCardMaker tracks={tracks} artist={artist} url={url} cover={cover} filename={filename} />
        </Modal>
      )}
    </>
  );
}

function LyricCardMaker({ tracks, artist, url, cover, filename }: { tracks: LyricTrack[]; artist: string; url: string; cover?: string; filename: string }) {
  const [ti, setTi] = useState(0);
  const lines = useMemo(() => lyricLines(tracks[ti]?.lyrics ?? ''), [tracks, ti]);
  // 처음엔 앞 두 줄 — 고른 순서가 아니라 가사 순서대로 넣는다
  const [picked, setPicked] = useState<number[]>([0, 1]);
  const chosen = picked.filter(i => i < lines.length).sort((a, b) => a - b).map(i => lines[i]);
  const toggle = (i: number) => setPicked(p => (p.includes(i) ? p.filter(x => x !== i) : p.length >= LYRIC_MAX_LINES ? p : [...p, i]));
  const title = tracks[ti]?.title ?? '';

  return (
    <ImageMaker
      render={f => renderLyricCard({ title, artist, lines: chosen.length ? chosen : lines.slice(0, 2), url, cover }, f)}
      deps={[ti, chosen.join('\n'), url, cover]}
      alt={`${title} 가사 카드 미리 보기`}
      filename={`${filename}-lyrics`}
      help={`가사에서 최대 ${LYRIC_MAX_LINES}줄을 골라 주세요. 아래 QR로 스마트링크가 열려요.`}
    >
      {tracks.length > 1 && (
        <div className="ui-field">
          <label className="ui-field-label" htmlFor="lyricTrack">곡</label>
          <select id="lyricTrack" className="ui-select" value={ti} onChange={e => { setTi(Number(e.target.value)); setPicked([0, 1]); }}>
            {tracks.map((t, i) => <option key={i} value={i}>{t.title}</option>)}
          </select>
        </div>
      )}
      <fieldset className="aq-lyric-pick">
        <legend className="ui-field-label">넣을 줄 <span className="muted">{chosen.length}/{LYRIC_MAX_LINES}</span></legend>
        <ul>
          {lines.map((l, i) => {
            const on = picked.includes(i);
            return (
              <li key={i}>
                <label className={on ? 'is-on' : undefined}>
                  <input type="checkbox" checked={on} disabled={!on && picked.length >= LYRIC_MAX_LINES} onChange={() => toggle(i)} />
                  <span>{l}</span>
                </label>
              </li>
            );
          })}
        </ul>
      </fieldset>
    </ImageMaker>
  );
}
