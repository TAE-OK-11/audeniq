// 전자서명 PNG — 서버(audeniq-music photo, validate_signature)는 IHDR·IDAT·IEND와 몇 가지 정적 청크만
// 받는다. 브라우저 인코더는 iCCP·iDOT·eXIf 같은 청크를 붙이기도 해서(iOS Safari 등) 같은 서명이
// 기기에 따라 SIGNATURE_INVALID가 됐다. 그래서 픽셀로 직접 최소 PNG를 만들고, 그럴 수 없으면
// 허용되지 않는 부가 청크만 떼어 낸다.

const PNG_MAGIC = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
const KEEP = new Set(['IHDR', 'IDAT', 'IEND', 'pHYs', 'sRGB', 'gAMA', 'cHRM']);

let crcTable: Uint32Array | null = null;
function crc32(bytes: Uint8Array): number {
  if (!crcTable) {
    crcTable = new Uint32Array(256);
    for (let n = 0; n < 256; n++) {
      let c = n;
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      crcTable[n] = c >>> 0;
    }
  }
  let c = 0xffffffff;
  for (const b of bytes) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type: string, body: Uint8Array): Uint8Array {
  const out = new Uint8Array(12 + body.length);
  const view = new DataView(out.buffer);
  view.setUint32(0, body.length);
  for (let i = 0; i < 4; i++) out[4 + i] = type.charCodeAt(i);
  out.set(body, 8);
  view.setUint32(8 + body.length, crc32(out.subarray(4, 8 + body.length)));
  return out;
}

function concat(parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let pos = 0;
  for (const p of parts) { out.set(p, pos); pos += p.length; }
  return out;
}

async function zlib(raw: Uint8Array<ArrayBuffer>): Promise<Uint8Array> {
  const stream = new Response(raw).body!.pipeThrough(new CompressionStream('deflate'));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

/** RGBA 8bit 픽셀 → IHDR·IDAT·IEND만 있는 PNG (CompressionStream이 없으면 null) */
export async function encodePng(width: number, height: number, rgba: Uint8Array | Uint8ClampedArray): Promise<Uint8Array | null> {
  if (typeof CompressionStream === 'undefined' || rgba.length !== width * height * 4) return null;
  const stride = width * 4;
  const raw = new Uint8Array(height * (stride + 1));
  for (let y = 0; y < height; y++) {
    raw[y * (stride + 1)] = 0; // 필터 없음
    raw.set(rgba.subarray(y * stride, (y + 1) * stride), y * (stride + 1) + 1);
  }
  const ihdr = new Uint8Array(13);
  const view = new DataView(ihdr.buffer);
  view.setUint32(0, width);
  view.setUint32(4, height);
  ihdr.set([8, 6, 0, 0, 0], 8); // 8bit, RGBA, deflate, 기본 필터, 인터레이스 없음
  return concat([Uint8Array.from(PNG_MAGIC), chunk('IHDR', ihdr), chunk('IDAT', await zlib(raw)), chunk('IEND', new Uint8Array())]);
}

/** 브라우저가 만든 PNG에서 서버가 받지 않는 부가 청크를 뗀다 (청크 CRC는 그대로 유효) */
export function stripPngChunks(png: Uint8Array): Uint8Array {
  if (png.length < 8 || PNG_MAGIC.some((b, i) => png[i] !== b)) return png;
  const view = new DataView(png.buffer, png.byteOffset, png.byteLength);
  const parts: Uint8Array[] = [png.subarray(0, 8)];
  const seen = new Set<string>();
  let pos = 8;
  while (pos + 12 <= png.length) {
    const len = view.getUint32(pos);
    const end = pos + 12 + len;
    if (end > png.length) return png;
    const type = String.fromCharCode(...png.subarray(pos + 4, pos + 8));
    // 정적 청크는 IDAT 앞에서 한 번씩만 허용된다
    const meta = type !== 'IHDR' && type !== 'IDAT' && type !== 'IEND';
    if (KEEP.has(type) && !(meta && (seen.has(type) || seen.has('IDAT')))) parts.push(png.subarray(pos, end));
    seen.add(type);
    pos = end;
    if (type === 'IEND') break;
  }
  return concat(parts);
}

const PREFIX = 'data:image/png;base64,';

function toBase64(bytes: Uint8Array): string {
  let s = '';
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

function fromBase64(b64: string): Uint8Array {
  const s = atob(b64);
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out;
}

export const pngDataUrl = (png: Uint8Array) => PREFIX + toBase64(png);

/** 이미 data URL로 받은 PNG 서명을 서버 규칙에 맞게 정리 (PNG가 아니면 그대로) */
export function cleanPngDataUrl(dataUrl: string): string {
  if (!dataUrl.startsWith(PREFIX)) return dataUrl;
  try {
    return pngDataUrl(stripPngChunks(fromBase64(dataUrl.slice(PREFIX.length))));
  } catch {
    return dataUrl;
  }
}
