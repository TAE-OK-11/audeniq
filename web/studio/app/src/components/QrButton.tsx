// QR 코드 버튼 — 누르면 미리 보기 창, '이미지 저장'으로 PNG(1024px). 휴대폰은 공유 시트로 사진 앱에 바로 저장.
import { useEffect, useState } from 'react';
import { Modal } from './Modal';
import { Glyph } from './Glyph';
import { useToast } from './Toast';
import { qrPng, saveImage } from '../lib/qr';

export function QrButton({ url, title, filename, className = 'ui-btn is-sm' }: { url: string; title: string; filename: string; className?: string }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" className={className} onClick={() => setOpen(true)}><Glyph name="qr" size={14} />QR</button>
      {open && (
        <Modal title={title} onClose={() => setOpen(false)} modalClass="aq-set-modal aq-qr-modal">
          <QrPreview url={url} filename={filename} />
        </Modal>
      )}
    </>
  );
}

function QrPreview({ url, filename }: { url: string; filename: string }) {
  const toast = useToast();
  const [blob, setBlob] = useState<Blob | null>(null);
  const [src, setSrc] = useState('');
  useEffect(() => {
    let alive = true;
    let made = '';
    qrPng(url).then(b => { if (!alive) return; made = URL.createObjectURL(b); setBlob(b); setSrc(made); }, () => toast('QR 코드를 만들지 못했어요.', 'error'));
    return () => { alive = false; if (made) URL.revokeObjectURL(made); };
  }, [url]); // eslint-disable-line react-hooks/exhaustive-deps

  const save = async () => {
    if (!blob) return;
    const how = await saveImage(blob, filename);
    if (how === 'downloaded') toast('QR 이미지를 저장했어요.', 'success');
  };
  return (
    <div className="aq-qr">
      <div className="aq-qr-frame">{src ? <img src={src} alt={`${url} QR 코드`} /> : <span className="aq-skel" />}</div>
      <p className="aq-qr-url">{url.replace(/^https?:\/\//, '')}</p>
      <p className="ui-field-help">포스터·굿즈·공연장 화면에 넣으면 휴대폰 카메라로 바로 열려요. 인쇄해도 깨지지 않는 1024px 이미지예요.</p>
      <button type="button" className="ui-btn is-primary is-block" disabled={!blob} onClick={() => void save()}><Glyph name="download" size={16} className="aq-glyph-lead" />이미지 저장</button>
    </div>
  );
}
