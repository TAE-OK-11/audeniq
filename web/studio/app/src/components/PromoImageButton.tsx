// 홍보 이미지 버튼 — 스토리(9:16)·피드(4:5) 중 고르면 미리 보기, '이미지 저장'으로 PNG. 휴대폰은 공유 시트로 인스타그램에 바로.
import { useEffect, useState } from 'react';
import { Modal } from './Modal';
import { Glyph } from './Glyph';
import { useToast } from './Toast';
import { Segmented } from '../ui';
import { saveImage } from '../lib/qr';
import { PROMO_SIZE, renderPromo, type PromoFormat, type PromoInput } from '../lib/promoImage';

export function PromoImageButton({ input, filename }: { input: PromoInput; filename: string }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" className="ui-btn is-sm" onClick={() => setOpen(true)}><Glyph name="image" size={14} />홍보 이미지</button>
      {open && (
        <Modal title="홍보 이미지" onClose={() => setOpen(false)} modalClass="aq-set-modal aq-promo-modal">
          <PromoPreview input={input} filename={filename} />
        </Modal>
      )}
    </>
  );
}

function PromoPreview({ input, filename }: { input: PromoInput; filename: string }) {
  const toast = useToast();
  const [format, setFormat] = useState<PromoFormat>('story');
  const [blob, setBlob] = useState<Blob | null>(null);
  const [src, setSrc] = useState('');
  useEffect(() => {
    let alive = true;
    let made = '';
    setBlob(null);
    renderPromo(input, format).then(b => {
      if (!alive) return;
      made = URL.createObjectURL(b);
      setBlob(b); setSrc(made);
    }, () => toast('이미지를 만들지 못했어요. 잠시 뒤 다시 시도해 주세요.', 'error'));
    return () => { alive = false; if (made) URL.revokeObjectURL(made); };
  }, [format, input.title, input.artist, input.releaseDate, input.url, input.cover]); // eslint-disable-line react-hooks/exhaustive-deps

  const save = async () => {
    if (!blob) return;
    const how = await saveImage(blob, `${filename}-${format}.png`);
    if (how === 'downloaded') toast('홍보 이미지를 저장했어요.', 'success');
  };
  const { w, h } = PROMO_SIZE[format];
  return (
    <div className="aq-promo">
      <Segmented label="이미지 비율" value={format} onChange={setFormat}
        options={(Object.keys(PROMO_SIZE) as PromoFormat[]).map(k => ({ value: k, label: PROMO_SIZE[k].label }))} />
      <div className="aq-promo-frame" style={{ aspectRatio: `${w} / ${h}` }}>
        {src && blob ? <img src={src} alt={`${input.title} 홍보 이미지 미리 보기`} /> : <span className="aq-skel" />}
      </div>
      <p className="ui-field-help">커버·발매일·스마트링크 QR이 들어간 이미지예요. 인스타그램 스토리·피드에 바로 올릴 수 있어요.</p>
      <button type="button" className="ui-btn is-primary is-block" disabled={!blob} onClick={() => void save()}><Glyph name="download" size={16} className="aq-glyph-lead" />이미지 저장</button>
    </div>
  );
}
