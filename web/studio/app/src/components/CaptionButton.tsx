// 홍보 문구 버튼 — 발매 예고·당일·발매 후 글을 만들어 고쳐 쓰고 복사. 올릴 채널을 고르면 그 채널 표시가 붙은 링크가 들어간다.
import { useEffect, useState } from 'react';
import { Modal } from './Modal';
import { Glyph } from './Glyph';
import { CopyButton } from './CopyButton';
import { Segmented } from '../ui';
import { CAPTION_STAGES, caption, defaultStage, type CaptionInput, type CaptionStage } from '../lib/captions';
import { CHANNELS, channelUrl } from '../lib/channels';

export function CaptionButton({ input, className = 'ui-btn is-sm' }: { input: CaptionInput; className?: string }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" className={className} onClick={() => setOpen(true)}><Glyph name="pencil" size={14} />홍보 문구</button>
      {open && (
        <Modal title="홍보 문구" onClose={() => setOpen(false)} modalClass="aq-set-modal aq-caption-modal">
          <CaptionMaker input={input} />
        </Modal>
      )}
    </>
  );
}

function CaptionMaker({ input }: { input: CaptionInput }) {
  const [stage, setStage] = useState<CaptionStage>(() => defaultStage(input.releaseDate));
  const [channel, setChannel] = useState('instagram');
  const [text, setText] = useState('');
  // 단계·채널을 바꾸면 새로 쓴다 (고친 글은 그 전까지 그대로)
  useEffect(() => { setText(caption({ ...input, url: channelUrl(input.url, channel) }, stage)); }, [stage, channel]); // eslint-disable-line react-hooks/exhaustive-deps
  const stages = input.releaseDate ? CAPTION_STAGES : CAPTION_STAGES.filter(s => s.value !== 'teaser');
  return (
    <div className="aq-caption">
      <Segmented label="언제 올릴 글" value={stage} onChange={setStage} options={stages} fill />
      <div className="ui-field">
        <label className="ui-field-label" htmlFor="captionChannel">올릴 곳</label>
        <select id="captionChannel" className="ui-select" value={channel} onChange={e => setChannel(e.target.value)}>
          {CHANNELS.filter(c => c.id !== 'qr' && c.id !== 'poster').map(c => <option key={c.id} value={c.id}>{c.label}</option>)}
        </select>
      </div>
      <label className="ui-field-label" htmlFor="captionText">문구</label>
      <textarea id="captionText" className="ui-input aq-caption-text" rows={8} value={text} onChange={e => setText(e.target.value)} />
      <p className="ui-field-help">링크에 채널 표시가 붙어 있어 스마트링크 카드의 ‘채널별 링크’에서 어디서 많이 들어왔는지 볼 수 있어요.</p>
      <CopyButton text={text} label="문구 복사" className="ui-btn is-primary is-block" />
    </div>
  );
}
