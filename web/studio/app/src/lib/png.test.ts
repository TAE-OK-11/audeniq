import { execFileSync } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { cleanPngDataUrl, encodePng, pngDataUrl, stripPngChunks } from './png';

const types = (png: Uint8Array) => {
  const view = new DataView(png.buffer, png.byteOffset, png.byteLength);
  const out: string[] = [];
  for (let pos = 8; pos + 12 <= png.length; pos += 12 + view.getUint32(pos)) out.push(String.fromCharCode(...png.subarray(pos + 4, pos + 8)));
  return out;
};

function signaturePixels(w: number, h: number) {
  const px = new Uint8Array(w * h * 4);
  for (let x = 0; x < w; x++) {
    const y = Math.round(h / 2 + Math.sin(x / 9) * (h / 3));
    for (let d = -2; d <= 2; d++) {
      const i = ((y + d) * w + x) * 4;
      px.set([17, 17, 17, 255], i);
    }
  }
  return px;
}

/** 부가 청크를 붙인 PNG (iOS Safari 인코더처럼 iCCP·eXIf가 들어간 경우) */
function withExtraChunks(png: Uint8Array): Uint8Array {
  const extra = (type: string, len: number) => {
    const c = new Uint8Array(12 + len);
    for (let i = 0; i < 4; i++) c[4 + i] = type.charCodeAt(i);
    new DataView(c.buffer).setUint32(0, len);
    return c;
  };
  const ihdrEnd = 8 + 25;
  const parts = [png.subarray(0, ihdrEnd), extra('iCCP', 20), extra('iDOT', 28), png.subarray(ihdrEnd)];
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let pos = 0;
  for (const p of parts) { out.set(p, pos); pos += p.length; }
  return out;
}

describe('서명 PNG', () => {
  it('직접 만든 PNG는 IHDR·IDAT·IEND만 가진다', async () => {
    const png = (await encodePng(420, 140, signaturePixels(420, 140)))!;
    expect(types(png)).toEqual(['IHDR', 'IDAT', 'IEND']);
    expect(png.length).toBeLessThan(45_000);
  });

  it('브라우저가 붙인 부가 청크를 뗀다', async () => {
    const png = (await encodePng(60, 20, signaturePixels(60, 20)))!;
    const dirty = withExtraChunks(png);
    expect(types(dirty)).toEqual(['IHDR', 'iCCP', 'iDOT', 'IDAT', 'IEND']);
    expect(Array.from(stripPngChunks(dirty))).toEqual(Array.from(png));
    expect(cleanPngDataUrl(pngDataUrl(dirty))).toBe(pngDataUrl(png));
    expect(cleanPngDataUrl('data:image/jpeg;base64,AAAA')).toBe('data:image/jpeg;base64,AAAA');
  });

  // 서버 검사기(audeniq-photo)가 있으면 실제로 통과하는지 확인
  it.skipIf(!process.env.AUDENIQ_PHOTO_BIN)('서버 서명 검사를 통과한다', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'sig-'));
    const run = (png: Uint8Array) => {
      writeFileSync(join(dir, 'in.png'), png);
      execFileSync(process.env.AUDENIQ_PHOTO_BIN!, ['sanitize', join(dir, 'in.png'), join(dir, 'out.png'), 'application/x-audeniq-signature']);
    };
    const png = (await encodePng(420, 140, signaturePixels(420, 140)))!;
    run(png);
    run(stripPngChunks(withExtraChunks(png)));
    expect(() => run(withExtraChunks(png))).toThrow();
  });
});
