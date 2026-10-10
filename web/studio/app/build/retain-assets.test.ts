// @vitest-environment node
import { mkdtempSync, readFileSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { build } from 'vite';
import { expect, it } from 'vitest';
import { retainAssets } from './retain-assets';

it('keeps old HTML imports and lazy modules across deployment, and bounds retained builds', async () => {
  const root = mkdtempSync(join(tmpdir(), 'aq-asset-retention-'));
  const files: string[] = [];
  try {
    writeFileSync(join(root, 'index.html'), '<html><body><script type="module" src="/main.js"></script></body></html>');
    for (let version = 0; version < 5; version++) {
      writeFileSync(join(root, 'main.js'), `import './style.css';document.body.dataset.version='${version}';window.load=()=>import('./lazy.js');`);
      writeFileSync(join(root, 'lazy.js'), `export const version=${version};`);
      writeFileSync(join(root, 'style.css'), `body{color:rgb(${version},0,0)}`);
      await build({ root, configFile: false, plugins: [retainAssets()], logLevel: 'silent', build: { minify: false, modulePreload: false } });
      if (version === 0) {
        const html = readFileSync(join(root, 'dist/index.html'), 'utf8');
        files.push(...[...html.matchAll(/\/assets\/([^" ]+)/g)].map(m => m[1]));
        const entry = files.find(f => f.endsWith('.js'))!;
        const js = readFileSync(join(root, 'dist/assets', entry), 'utf8');
        files.push(/\.\/(lazy-[\w-]+\.js)/.exec(js)![1]);
      }
      if (version === 1 || version === 3) for (const file of files) expect(existsSync(join(root, 'dist/assets', file)), file).toBe(true);
    }
    for (const file of files) expect(existsSync(join(root, 'dist/assets', file)), file).toBe(false);
    const manifest = JSON.parse(readFileSync(join(root, 'dist/assets/compat-entries.json'), 'utf8'));
    expect(manifest).toHaveLength(3);
    // Rebuilding the same source must not consume another retention slot.
    await build({ root, configFile: false, plugins: [retainAssets()], logLevel: 'silent', build: { minify: false, modulePreload: false } });
    expect(JSON.parse(readFileSync(join(root, 'dist/assets/compat-entries.json'), 'utf8'))).toEqual(manifest);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
