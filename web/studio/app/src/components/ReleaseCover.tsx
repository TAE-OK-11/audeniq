import { useEffect, useState } from 'react';
import { gradientFor } from '../lib/catalog';
import { Glyph } from './Glyph';

/** 이미지를 못 불러오면(서버가 아직 주소를 모르거나, 지워졌거나, 연결이 끊김) false가 된다.
 *  깨진 이미지 아이콘 대신 기본 커버를 보여 주는 데 쓴다. 주소가 바뀌면 다시 시도한다. */
export function useImageOk(src?: string): [boolean, () => void] {
  const [failed, setFailed] = useState<string | undefined>();
  useEffect(() => { setFailed(undefined); }, [src]);
  return [!!src && failed !== src, () => setFailed(src)];
}

/** 크게 보여 줄 커버 — 먼저 작은 미리보기(240px)를 보여 주고, 원본을 다 받으면 원본으로 바꾼다.
 *  미리보기를 그대로 키우면 휴대폰(3배 화면)에서 뭉개져 보인다. 원본을 못 받으면 미리보기를 그대로 둔다. */
export function useSharpImage(preview?: string, full?: string): string | undefined {
  const [loaded, setLoaded] = useState<string | undefined>();
  useEffect(() => {
    if (!full || full === preview) return;
    let live = true;
    const img = new Image();
    img.decoding = 'async';
    img.onload = () => { if (live) setLoaded(full); };
    img.src = full;
    return () => { live = false; img.onload = null; };
  }, [full, preview]);
  return loaded && loaded === full ? full : preview;
}

/** 발매 커버 — 등록한 커버 이미지가 있으면 이미지, 없거나 못 불러오면 ID 기반 그라디언트 */
export function ReleaseCover({ id, src, className = 'cover aq-cover', size }: {
  id: string;
  src?: string;
  className?: string;
  size?: number;
}) {
  const style = size ? { width: size, height: size, flexBasis: size } : undefined;
  const [ok, onError] = useImageOk(src);
  if (ok) {
    return (
      <span className={`${className} has-image`} aria-hidden="true" style={style}>
        <img src={src} alt="" loading="lazy" decoding="async" onError={onError} />
      </span>
    );
  }
  return (
    <span className={className} aria-hidden="true" style={{ ...style, background: gradientFor(id) }}><Glyph name="music" className="aq-cover-glyph" /></span>
  );
}
