// Brotli runs once during the build, rather than consuming request CPU — so it uses the
// highest quality: a few seconds more per build, ~8% fewer bytes on every first visit.
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { brotliCompressSync, constants } from 'node:zlib';

const TEXT = new Set(['.js', '.css', '.html', '.json', '.svg', '.wasm']);
export interface CompressStats { files: number; rawBytes: number; compressedBytes: number }
export function compressAssets(directory: string): CompressStats {
  let rawBytes = 0, compressedBytes = 0, files = 0;
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) {
      const nested = compressAssets(path);
      rawBytes += nested.rawBytes; compressedBytes += nested.compressedBytes; files += nested.files;
    } else if (entry.isFile() && TEXT.has(extname(entry.name))) {
      const raw = readFileSync(path);
      const br = brotliCompressSync(raw, {
        params: { [constants.BROTLI_PARAM_QUALITY]: constants.BROTLI_MAX_QUALITY, [constants.BROTLI_PARAM_SIZE_HINT]: raw.length },
      });
      writeFileSync(`${path}.br`, br);
      rawBytes += raw.length; compressedBytes += br.length; files++;
    }
  }
  return { files, rawBytes, compressedBytes };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  for (const directory of process.argv.slice(2)) console.log(JSON.stringify({ directory, ...compressAssets(resolve(directory)) }));
}
